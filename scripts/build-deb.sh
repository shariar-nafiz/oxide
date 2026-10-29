#!/usr/bin/env bash
set -e

echo "Building Oxide release binaries..."
cargo build --release -p oxide-cli -p oxide-gui -p oxide-nmh

echo "Staging Debian package hierarchy..."
PKG_DIR="dist/deb-pkg"
rm -rf "$PKG_DIR"
mkdir -p "$PKG_DIR/DEBIAN" \
         "$PKG_DIR/usr/bin" \
         "$PKG_DIR/usr/share/applications" \
         "$PKG_DIR/usr/share/icons/hicolor/scalable/apps"

cat << 'EOF' > "$PKG_DIR/DEBIAN/control"
Package: oxide-dl
Version: 0.1.0
Section: net
Priority: optional
Architecture: amd64
Maintainer: Md Shariar <shariarnafiz.go@gmail.com>
Depends: libc6 (>= 2.34), libgtk-3-0, libayatana-appindicator3-1
Description: High-performance dynamic multi-segment download manager
 Oxide is a blazing-fast, ultra-lightweight download manager built 100% in Rust.
 Features dynamic in-flight work-stealing, zero post-download file stitching delay,
 universal web browser integration, and media stream grabbing.
 Homepage: https://github.com/shariar-nafiz/oxide
EOF

cp scripts/postinst.sh "$PKG_DIR/DEBIAN/postinst"
chmod 755 "$PKG_DIR/DEBIAN/postinst"

cp target/release/oxide-dl "$PKG_DIR/usr/bin/oxide-dl"
cp target/release/oxide-cli "$PKG_DIR/usr/bin/oxide-cli"
cp target/release/oxide-nmh "$PKG_DIR/usr/bin/oxide-nmh"
chmod 755 "$PKG_DIR/usr/bin/"*

cp scripts/oxide-dl.desktop "$PKG_DIR/usr/share/applications/oxide-dl.desktop"
cp scripts/oxide-dl.svg "$PKG_DIR/usr/share/icons/hicolor/scalable/apps/oxide-dl.svg"

DEB_NAME="dist/oxide-dl_0.1.0_amd64.deb"
echo "Packaging $DEB_NAME..."
dpkg-deb --build --root-owner-group "$PKG_DIR" "$DEB_NAME"

echo "Package created successfully: $DEB_NAME"
ls -lh "$DEB_NAME"
