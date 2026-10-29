use byteorder::{NativeEndian, ReadBytesExt, WriteBytesExt};
use oxide_ipc::{IpcClient, IpcRequest, IpcResponse};
use std::io::{self, Read, Write};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let stdin = io::stdin();
    let mut stdin_lock = stdin.lock();

    let stdout = io::stdout();
    let mut stdout_lock = stdout.lock();

    let client = IpcClient::new();

    loop {
        // Native Messaging protocol: 32-bit length in native endianness followed by UTF-8 JSON
        let msg_len = match stdin_lock.read_u32::<NativeEndian>() {
            Ok(len) => len as usize,
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(e) => {
                eprintln!("oxide-nmh read length error: {}", e);
                break;
            }
        };

        let mut buffer = vec![0u8; msg_len];
        if let Err(e) = stdin_lock.read_exact(&mut buffer) {
            eprintln!("oxide-nmh read payload error: {}", e);
            break;
        }

        let response = match serde_json::from_slice::<IpcRequest>(&buffer) {
            Ok(request) => match client.send_request(&request).await {
                Ok(resp) => resp,
                Err(e) => IpcResponse::Error {
                    message: format!("Failed to reach Oxide daemon: {}. Is Oxide running?", e),
                },
            },
            Err(e) => IpcResponse::Error {
                message: format!("Invalid Native Messaging JSON payload: {}", e),
            },
        };

        let resp_bytes = serde_json::to_vec(&response)?;
        stdout_lock.write_u32::<NativeEndian>(resp_bytes.len() as u32)?;
        stdout_lock.write_all(&resp_bytes)?;
        stdout_lock.flush()?;
    }

    Ok(())
}
