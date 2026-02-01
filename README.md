# RustFTPFS

A FTP filesystem implementation in Rust that allows you to mount FTP servers as local directories, similar to the curlftpfs utility.

## Features

- Mount FTP servers as local filesystems using FUSE
- Support for FTP over TLS/SSL (FTPS)
- Read and write file operations
- Directory listing and navigation
- Create, delete, and rename files and directories
- Automatic reconnection on connection failures
- Configurable mount options
- Cross-platform support (Linux, macOS, FreeBSD)

## Installation

### Requirements

- Rust 1.70 or later
- FUSE development libraries

#### Installing FUSE libraries

**Ubuntu/Debian:**
```bash
sudo apt-get install libfuse3-dev pkg-config
```

**CentOS/RHEL/Fedora:**
```bash
sudo yum install fuse-devel pkgconfig
# or for newer systems:
sudo dnf install fuse-devel pkgconf-pkg-config
```

**macOS:**
```bash
brew install macfuse pkgconf
```

**FreeBSD:**
```bash
pkg install fusefs-libs pkgconf
```

### Building from source

```bash
git clone https://github.com/yourusername/rustftpfs.git
cd rustftpfs
cargo build --release
```

The binary will be available at `target/release/rustftpfs`.

## Usage

### Basic Usage

Mount an FTP server:
```bash
rustftpfs ftp://username:password@ftp.example.com /mnt/ftp
```

Mount with explicit credentials:
```bash
rustftpfs ftp://ftp.example.com /mnt/ftp --user myuser --password mypass
```

Mount with custom port:
```bash
rustftpfs ftp://ftp.example.com:2121 /mnt/ftp --user myuser --password mypass
```

### Command Line Options

```
Usage: rustftpfs [OPTIONS] <FTP_URL> <MOUNTPOINT>

Arguments:
  <FTP_URL>      FTP URL in format ftp://[user[:password]@]host[:port][/path]
  <MOUNTPOINT>   Local directory to mount the FTP filesystem

Options:
  -u, --user <USERNAME>        Username for FTP authentication
  -p, --password <PASSWORD>    Password for FTP authentication
  -P, --port <PORT>            FTP port (default: 21)
      --tls                    Use TLS/SSL encryption
  -r, --read-only              Mount filesystem as read-only
  -f, --foreground             Run in foreground mode
  -d, --debug                  Enable debug output
      --allow-other            Allow other users to access the mount
      --uid <UID>              Set file owner UID
      --gid <GID>              Set file group GID
      --umask <UMASK>          Set file permissions umask
  -h, --help                   Print help information
  -V, --version                Print version information
```

### Mount Options

- `-r, --read-only`: Mount the filesystem in read-only mode
- `-f, --foreground`: Run the program in foreground (don't daemonize)
- `-d, --debug`: Enable debug logging
- `--allow-other`: Allow other users to access the mounted filesystem
- `--tls`: Use TLS/SSL encryption for FTP connection

### Examples

#### Mount with read-only access
```bash
rustftpfs --read-only ftp://ftp.gnu.org /mnt/gnu
```

#### Mount with TLS encryption
```bash
rustftpfs --tls ftp://secure.example.com /mnt/secureftp --user myuser
```

#### Allow other users to access
```bash
rustftpfs --allow-other ftp://ftp.example.com /mnt/ftp --user myuser
```

#### Run in foreground with debug output
```bash
rustftpfs --foreground --debug ftp://ftp.example.com /mnt/ftp --user myuser
```

### Unmounting

To unmount the filesystem:
```bash
fusermount -u /mnt/ftp
# or
umount /mnt/ftp
```

## Environment Variables

- `RUST_LOG`: Set logging level (e.g., `RUST_LOG=debug`)

## Architecture

The project consists of two main modules:

1. **ftp.rs**: Handles FTP connections and operations using the `suppaftp` crate
2. **filesystem.rs**: Implements the FUSE filesystem interface using the `fuser` crate

### Key Components

- `FtpConnection`: Manages FTP connections with automatic reconnection
- `FtpFs`: Implements the FUSE filesystem operations
- Inode management for tracking files and directories
- Read caching for improved performance

## Comparison with curlftpfs

| Feature | RustFTPFS | curlftpfs |
|---------|-----------|-----------|
| Language | Rust | C |
| FTP Library | suppaftp | libcurl |
| FUSE Library | fuser | libfuse |
| TLS Support | Yes | Yes |
| Auto-reconnect | Yes | Yes |
| Proxy Support | Planned | Yes |
| Symlinks | Basic | Advanced |
| Performance | Good | Good |
| Memory Safety | High | Medium |

## Development

### Project Structure

```
rustftpfs/
├── src/
│   ├── main.rs      # CLI and entry point
│   ├── lib.rs       # Library exports
│   ├── ftp.rs       # FTP connection handling
│   └── filesystem.rs # FUSE filesystem implementation
├── Cargo.toml       # Dependencies and metadata
└── README.md        # This file
```

### Running Tests

```bash
cargo test
```

### Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Add tests if applicable
5. Submit a pull request

## License

This project is licensed under the MIT License - see the LICENSE file for details.

## Acknowledgments

- Inspired by the curlftpfs project
- Built with [fuser](https://github.com/cberner/fuser) and [suppaftp](https://github.com/veeso/suppaftp) crates
- Thanks to all contributors to the Rust FUSE ecosystem

## Troubleshooting

### Permission Denied

If you get permission errors:
1. Make sure you're in the `fuse` group: `sudo usermod -a -G fuse $USER`
2. Log out and log back in
3. Check mountpoint permissions

### Connection Issues

1. Verify FTP server address and credentials
2. Check if TLS is required by the server
3. Try using passive mode (default behavior)

### Mount Failures

1. Ensure the mountpoint directory exists
2. Check if the directory is already mounted
3. Verify FUSE is installed and working: `lsmod | grep fuse`

## Safety

This program uses unsafe code only for:
- Getting current user ID and group ID
- FUSE filesystem operations (through the fuser crate)

The core FTP and filesystem logic is implemented in safe Rust.# RustFtpFS
