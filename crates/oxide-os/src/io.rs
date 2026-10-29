use std::fs::File;
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum IoError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Unsupported platform operation: {0}")]
    Unsupported(String),
}

/// A handle to a preallocated file that allows multi-worker concurrent positional writes
/// without locking or file-offset race conditions.
#[derive(Debug)]
pub struct PositionalWriter {
    file: File,
}

impl PositionalWriter {
    /// Opens or creates a file at the given path and pre-allocates `total_bytes` on disk.
    /// This prevents file fragmentation and eliminates the need for post-download stitching.
    pub fn create_preallocated(path: &Path, total_bytes: u64) -> Result<Self, IoError> {
        let file = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)?;

        preallocate_file(&file, total_bytes)?;

        Ok(Self { file })
    }

    /// Opens an existing file for resuming positional writes.
    pub fn open_existing(path: &Path) -> Result<Self, IoError> {
        let file = File::options().read(true).write(true).open(path)?;
        Ok(Self { file })
    }

    /// Writes a slice of bytes directly to the exact target byte offset.
    /// Thread-safe: can be called concurrently across multiple threads/tasks with different offsets.
    pub fn write_at(&self, offset: u64, buf: &[u8]) -> Result<usize, IoError> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::FileExt;
            Ok(self.file.write_at(buf, offset)?)
        }

        #[cfg(windows)]
        {
            use std::os::windows::fs::FileExt;
            Ok(self.file.seek_write(buf, offset)?)
        }

        #[cfg(not(any(unix, windows)))]
        {
            Err(IoError::Unsupported("Positional writing unsupported on this OS".into()))
        }
    }

    /// Flushes OS page caches to persistent physical storage.
    pub fn sync_all(&self) -> Result<(), IoError> {
        Ok(self.file.sync_all()?)
    }

    /// Exposes a reference to the underlying `File`.
    pub fn file(&self) -> &File {
        &self.file
    }
}

/// Pre-allocates disk blocks for a file.
/// - Linux: uses `fallocate` or sets length via truncate fallback
/// - macOS: uses `fcntl(F_PREALLOCATE)`
/// - Windows: sets file length via `set_len` / `SetFileInformationByHandle`
fn preallocate_file(file: &File, total_bytes: u64) -> Result<(), IoError> {
    if total_bytes == 0 {
        return Ok(());
    }

    #[cfg(target_os = "linux")]
    {
        use std::os::unix::io::AsRawFd;
        let fd = file.as_raw_fd();
        // fallocate mode 0: allocate disk blocks directly
        let res = unsafe { libc::fallocate(fd, 0, 0, total_bytes as libc::off_t) };
        if res != 0 {
            // Fallback to setting file length if filesystem doesn't support fallocate
            file.set_len(total_bytes)?;
        }
        Ok(())
    }

    #[cfg(target_os = "macos")]
    {
        use std::os::unix::io::AsRawFd;
        let fd = file.as_raw_fd();
        #[repr(C)]
        struct fstore_t {
            fst_flags: u32,
            fst_posmode: i32,
            fst_offset: i64,
            fst_length: i64,
            fst_bytesalloc: i64,
        }
        let mut store = fstore_t {
            fst_flags: 0x0001, // F_ALLOCATECONTIG
            fst_posmode: 0,    // F_PEOFPOSMODE
            fst_offset: 0,
            fst_length: total_bytes as i64,
            fst_bytesalloc: 0,
        };
        let res = unsafe { libc::fcntl(fd, libc::F_PREALLOCATE, &mut store) };
        if res != 0 {
            // Retry non-contiguous allocation
            store.fst_flags = 0x0002; // F_ALLOCATEALL
            let _ = unsafe { libc::fcntl(fd, libc::F_PREALLOCATE, &mut store) };
        }
        file.set_len(total_bytes)?;
        return Ok(());
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        file.set_len(total_bytes)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn test_positional_writer_concurrent_blocks() {
        let temp_dir = std::env::temp_dir();
        let test_file = temp_dir.join("oxide_test_positional.bin");

        let writer = PositionalWriter::create_preallocated(&test_file, 1024).expect("create file");

        // Write out-of-order blocks
        writer.write_at(512, b"SECOND_HALF").expect("write block 2");
        writer.write_at(0, b"FIRST_HALF").expect("write block 1");
        writer.sync_all().expect("sync");

        let mut read_back = File::open(&test_file).expect("open to read");
        let mut buf = vec![0u8; 1024];
        read_back.read_exact(&mut buf).expect("read");

        assert_eq!(&buf[0..10], b"FIRST_HALF");
        assert_eq!(&buf[512..523], b"SECOND_HALF");

        let _ = std::fs::remove_file(test_file);
    }
}
