//! RustFTPFS - A FTP filesystem implementation in Rust
//!
//! This crate provides functionality to mount FTP servers as local filesystems
//! using FUSE (Filesystem in Userspace), similar to the curlftpfs utility.

pub mod ftp;
pub mod filesystem;

pub use ftp::{FtpConnection, FtpFileInfo};
pub use filesystem::FtpFs;