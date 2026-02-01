# Quick Start Guide - RustFTPFS

## Prerequisites

Install FUSE development libraries:

**Ubuntu/Debian:**
```bash
sudo apt-get update
sudo apt-get install libfuse3-dev pkg-config
```

**CentOS/RHEL:**
```bash
sudo yum install fuse-devel pkgconfig
```

**macOS:**
```bash
brew install macfuse pkgconf
```

## Building

```bash
cargo build --release
```

The binary will be at: `target/release/rustftpfs`

## Basic Usage

### 1. Simple Mount
```bash
# Mount with URL credentials
./target/release/rustftpfs ftp://user:password@ftp.example.com /mnt/ftp

# Mount with separate credentials
./target/release/rustftpfs ftp://ftp.example.com /mnt/ftp --user myuser --password mypass
```

### 2. Common Options
```bash
# Mount as read-only
./target/release/rustftpfs --read-only ftp://ftp.gnu.org /mnt/gnu

# Use TLS encryption
./target/release/rustftpfs --tls ftp://secure.example.com /mnt/secure --user myuser

# Allow other users
./target/release/rustftpfs --allow-other ftp://example.com /mnt/ftp --user myuser

# Run in foreground with debug
./target/release/rustftpfs --foreground --debug ftp://example.com /mnt/ftp --user myuser
```

### 3. Custom Port
```bash
./target/release/rustftpfs --port 2121 ftp://example.com:2121 /mnt/ftp --user myuser
```

## Unmounting

```bash
# Method 1
fusermount -u /mnt/ftp

# Method 2
umount /mnt/ftp
```

## Examples

Try with a public FTP server:
```bash
# Create mountpoint
mkdir -p /tmp/ftp

# Mount GNU FTP (read-only recommended)
./target/release/rustftpfs --read-only ftp://ftp.gnu.org /tmp/ftp

# List files
ls -la /tmp/ftp

# Unmount when done
fusermount -u /tmp/ftp
```

## Troubleshooting

### "fuse: device not found"
- Install FUSE: `sudo apt-get install fuse3`
- Load module: `sudo modprobe fuse`

### "Permission denied"
- Add user to fuse group: `sudo usermod -a -G fuse $USER`
- Log out and back in

### "Failed to connect"
- Check FTP server address
- Verify credentials
- Try with `--debug` flag for more info

### Connection timeouts
- Some servers need passive mode (default behavior)
- Check firewall settings

## Environment Variables

```bash
# Enable debug logging
RUST_LOG=debug ./target/release/rustftpfs ftp://example.com /mnt/ftp
```

## systemd Integration

1. Copy service file:
```bash
sudo cp examples/rustftpfs@.service /etc/systemd/system/
sudo systemctl daemon-reload
```

2. Create config:
```bash
sudo mkdir -p /etc/rustftpfs
sudo cp examples/myservice.conf /etc/rustftpfs/myftp.conf
# Edit the config with your settings
sudo nano /etc/rustftpfs/myftp.conf
```

3. Enable and start:
```bash
sudo systemctl enable rustftpfs@myftp
sudo systemctl start rustftpfs@myftp
```

## fstab Integration

Add to `/etc/fstab`:
```
rustftpfs#ftp://user:pass@example.com /mnt/ftp fuse rw,_netdev,allow_other 0 0
```

Then mount:
```bash
sudo mount /mnt/ftp
```

## Security Notes

- Never store passwords in scripts
- Use environment variables or config files with restricted permissions
- Consider using TLS (`--tls` flag) for encrypted connections
- Use read-only mode (`--read-only`) when possible

## Getting Help

```bash
# Show help
./target/release/rustftpfs --help

# Check version
./target/release/rustftpfs --version

# Enable debug mode for troubleshooting
./target/release/rustftpfs --debug ftp://example.com /mnt/ftp
```