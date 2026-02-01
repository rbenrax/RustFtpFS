# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2024-01-25

### Added
- Initial release of RustFTPFS
- FTP connection management with automatic reconnection
- FUSE filesystem implementation
- Basic file operations: read, write, create, delete, rename
- Directory operations: list, create, remove
- TLS/SSL support for secure FTP connections
- Command-line interface with clap
- Logging support with env_logger
- Read caching for improved performance
- Inode management for tracking files and directories
- Support for foreground and background mounting
- Read-only mount option
- Allow-other mount option
- Custom UID/GID/umask support
- Example scripts for mounting and unmounting
- Example fstab configuration
- Comprehensive error handling with anyhow and thiserror
- Unit tests for FTP URL parsing
- MIT License

### Features
- Mount FTP servers as local directories
- Support for standard FTP operations
- Automatic reconnection on connection failures
- Configurable mount options
- Cross-platform support (Linux, macOS, FreeBSD)
- Memory-safe implementation in Rust

### Technical Details
- Built with Rust 2021 edition
- Uses fuser crate for FUSE bindings
- Uses suppaftp crate for FTP operations
- Implements Filesystem trait from fuser
- Supports FUSE mount options
- Thread-safe FTP connection handling