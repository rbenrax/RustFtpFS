//! FTP Connection Module
//!
//! Handles FTP connections and operations using the suppaftp crate.

use std::io::{self, Read, Write};
use std::path::Path;
use std::time::SystemTime;

use anyhow::{Context, Result};
use log::{debug, error, info, warn};
use suppaftp::native_tls::TlsConnector;
use suppaftp::types::{FileType, Mode};
use suppaftp::{FtpStream, NativeTlsConnector, NativeTlsFtpStream};

/// Information about a file or directory on the FTP server
#[derive(Debug, Clone)]
pub struct FtpFileInfo {
    pub name: String,
    pub path: String,
    pub size: u64,
    pub is_dir: bool,
    pub permissions: u32,
    pub modified_time: Option<SystemTime>,
}

/// FTP Connection wrapper supporting both plain FTP and FTPS
pub struct FtpConnection {
    stream: FtpStreamVariant,
    server: String,
    username: String,
    password: String,
    use_tls: bool,
    port: u16,
    current_dir: String,
}

/// Enum to handle both plain and TLS FTP streams
enum FtpStreamVariant {
    Plain(FtpStream),
    Tls(NativeTlsFtpStream),
}

impl FtpConnection {
    /// Create a new FTP connection
    pub fn new(
        server: String,
        username: String,
        password: String,
        use_tls: bool,
        port: Option<u16>,
    ) -> Result<Self> {
        let port = port.unwrap_or(21);
        let addr = format!("{}:{}", server, port);

        info!("Connecting to FTP server at {}", addr);

        let stream = if use_tls {
            // Create TLS connector
            let connector = TlsConnector::builder()
                .danger_accept_invalid_certs(true) // For development; should be configurable
                .build()
                .context("Failed to create TLS connector")?;
            let native_connector = NativeTlsConnector::from(connector);

            // Connect with TLS
            let ftp_stream =
                NativeTlsFtpStream::connect(&addr).context("Failed to connect to FTPS server")?;
            let mut ftp_stream = ftp_stream
                .into_secure(native_connector, &server)
                .context("Failed to establish TLS connection")?;

            ftp_stream
                .login(&username, &password)
                .context("Failed to login to FTPS server")?;

            FtpStreamVariant::Tls(ftp_stream)
        } else {
            // Connect without TLS
            let mut ftp_stream =
                FtpStream::connect(&addr).context("Failed to connect to FTP server")?;

            ftp_stream
                .login(&username, &password)
                .context("Failed to login to FTP server")?;

            FtpStreamVariant::Plain(ftp_stream)
        };

        info!("Successfully connected to FTP server");

        let mut conn = FtpConnection {
            stream,
            server,
            username,
            password,
            use_tls,
            port,
            current_dir: "/".to_string(),
        };

        // Set transfer type to binary
        conn.set_transfer_type(FileType::Binary)?;

        // Set passive mode
        conn.set_mode(Mode::Passive)?;

        Ok(conn)
    }

    /// Reconnect to the FTP server (useful after connection loss)
    pub fn reconnect(&mut self) -> Result<()> {
        info!("Reconnecting to FTP server...");

        let new_conn = Self::new(
            self.server.clone(),
            self.username.clone(),
            self.password.clone(),
            self.use_tls,
            Some(self.port),
        )?;

        self.stream = new_conn.stream;
        self.current_dir = new_conn.current_dir;

        info!("Reconnected successfully");
        Ok(())
    }

    /// Set FTP mode (Passive, Active, ExtendedPassive)
    fn set_mode(&mut self, mode: Mode) -> Result<()> {
        match &mut self.stream {
            FtpStreamVariant::Plain(stream) => {
                stream.set_mode(mode);
            }
            FtpStreamVariant::Tls(stream) => {
                stream.set_mode(mode);
            }
        }
        Ok(())
    }

    /// Set transfer type (Binary or ASCII)
    fn set_transfer_type(&mut self, file_type: FileType) -> Result<()> {
        match &mut self.stream {
            FtpStreamVariant::Plain(stream) => {
                stream
                    .transfer_type(file_type)
                    .context("Failed to set transfer type")?;
            }
            FtpStreamVariant::Tls(stream) => {
                stream
                    .transfer_type(file_type)
                    .context("Failed to set transfer type")?;
            }
        }
        Ok(())
    }

    /// Get current working directory
    pub fn pwd(&mut self) -> Result<String> {
        let path = match &mut self.stream {
            FtpStreamVariant::Plain(stream) => {
                stream.pwd().context("Failed to get current directory")?
            }
            FtpStreamVariant::Tls(stream) => {
                stream.pwd().context("Failed to get current directory")?
            }
        };
        self.current_dir = path.clone();
        Ok(path)
    }

    /// Change working directory
    pub fn cwd(&mut self, path: &str) -> Result<()> {
        debug!("Changing directory to: {}", path);

        match &mut self.stream {
            FtpStreamVariant::Plain(stream) => stream
                .cwd(path)
                .context(format!("Failed to change directory to {}", path))?,
            FtpStreamVariant::Tls(stream) => stream
                .cwd(path)
                .context(format!("Failed to change directory to {}", path))?,
        }

        self.current_dir = path.to_string();
        Ok(())
    }

    /// Change to parent directory
    pub fn cdup(&mut self) -> Result<()> {
        debug!("Changing to parent directory");

        match &mut self.stream {
            FtpStreamVariant::Plain(stream) => stream
                .cdup()
                .context("Failed to change to parent directory")?,
            FtpStreamVariant::Tls(stream) => stream
                .cdup()
                .context("Failed to change to parent directory")?,
        }

        // Update current directory
        let _ = self.pwd();
        Ok(())
    }

    /// List files in current directory
    pub fn list(&mut self) -> Result<Vec<FtpFileInfo>> {
        debug!("Listing directory contents");

        let list = match &mut self.stream {
            FtpStreamVariant::Plain(stream) => {
                stream.list(None).context("Failed to list directory")?
            }
            FtpStreamVariant::Tls(stream) => {
                stream.list(None).context("Failed to list directory")?
            }
        };

        let mut files = Vec::new();
        for entry in list {
            if let Ok(file_info) = self.parse_list_line(&entry) {
                files.push(file_info);
            } else {
                debug!("Failed to parse line: {}", entry);
            }
        }

        Ok(files)
    }

    /// List files in a specific directory
    pub fn list_dir(&mut self, path: &str) -> Result<Vec<FtpFileInfo>> {
        let original_dir = self.pwd()?;
        self.cwd(path)?;
        let files = self.list()?;
        self.cwd(&original_dir)?;
        Ok(files)
    }

    /// Get file size
    pub fn size(&mut self, path: &str) -> Result<u64> {
        let size = match &mut self.stream {
            FtpStreamVariant::Plain(stream) => stream
                .size(path)
                .context(format!("Failed to get size of {}", path))?,
            FtpStreamVariant::Tls(stream) => stream
                .size(path)
                .context(format!("Failed to get size of {}", path))?,
        };

        Ok(size as u64)
    }

    /// Download file contents
    pub fn retrieve(&mut self, path: &str) -> Result<Vec<u8>> {
        debug!("Retrieving file: {}", path);

        let data = match &mut self.stream {
            FtpStreamVariant::Plain(stream) => {
                let mut reader = stream
                    .retr_as_buffer(path)
                    .context(format!("Failed to retrieve file {}", path))?;
                let mut data = Vec::new();
                reader
                    .read_to_end(&mut data)
                    .context("Failed to read file data")?;
                data
            }
            FtpStreamVariant::Tls(stream) => {
                let mut reader = stream
                    .retr_as_buffer(path)
                    .context(format!("Failed to retrieve file {}", path))?;
                let mut data = Vec::new();
                reader
                    .read_to_end(&mut data)
                    .context("Failed to read file data")?;
                data
            }
        };

        debug!("Retrieved {} bytes from {}", data.len(), path);
        Ok(data)
    }

    /// Upload file contents
    pub fn store(&mut self, path: &str, data: &[u8]) -> Result<()> {
        debug!("Storing file: {} ({} bytes)", path, data.len());

        match &mut self.stream {
            FtpStreamVariant::Plain(stream) => {
                let mut reader = io::Cursor::new(data);
                stream
                    .put_file(path, &mut reader)
                    .context(format!("Failed to store file {}", path))?;
            }
            FtpStreamVariant::Tls(stream) => {
                let mut reader = io::Cursor::new(data);
                stream
                    .put_file(path, &mut reader)
                    .context(format!("Failed to store file {}", path))?;
            }
        }

        Ok(())
    }

    /// Delete a file
    pub fn delete(&mut self, path: &str) -> Result<()> {
        debug!("Deleting file: {}", path);

        match &mut self.stream {
            FtpStreamVariant::Plain(stream) => stream
                .rm(path)
                .context(format!("Failed to delete file {}", path))?,
            FtpStreamVariant::Tls(stream) => stream
                .rm(path)
                .context(format!("Failed to delete file {}", path))?,
        }

        Ok(())
    }

    /// Create a directory
    pub fn mkdir(&mut self, path: &str) -> Result<()> {
        debug!("Creating directory: {}", path);

        match &mut self.stream {
            FtpStreamVariant::Plain(stream) => stream
                .mkdir(path)
                .context(format!("Failed to create directory {}", path))?,
            FtpStreamVariant::Tls(stream) => stream
                .mkdir(path)
                .context(format!("Failed to create directory {}", path))?,
        }

        Ok(())
    }

    /// Remove a directory
    pub fn rmdir(&mut self, path: &str) -> Result<()> {
        debug!("Removing directory: {}", path);

        match &mut self.stream {
            FtpStreamVariant::Plain(stream) => stream
                .rmdir(path)
                .context(format!("Failed to remove directory {}", path))?,
            FtpStreamVariant::Tls(stream) => stream
                .rmdir(path)
                .context(format!("Failed to remove directory {}", path))?,
        }

        Ok(())
    }

    /// Rename a file or directory
    pub fn rename(&mut self, from: &str, to: &str) -> Result<()> {
        debug!("Renaming {} to {}", from, to);

        match &mut self.stream {
            FtpStreamVariant::Plain(stream) => stream
                .rename(from, to)
                .context(format!("Failed to rename {} to {}", from, to))?,
            FtpStreamVariant::Tls(stream) => stream
                .rename(from, to)
                .context(format!("Failed to rename {} to {}", from, to))?,
        }

        Ok(())
    }

    /// Check if path is a directory
    pub fn is_dir(&mut self, path: &str) -> Result<bool> {
        // Try to change to the directory - if it succeeds, it's a directory
        let original_dir = self.pwd()?;

        match self.cwd(path) {
            Ok(_) => {
                self.cwd(&original_dir)?;
                Ok(true)
            }
            Err(_) => Ok(false),
        }
    }

    /// Check if file exists
    pub fn exists(&mut self, path: &str) -> Result<bool> {
        match self.size(path) {
            Ok(_) => Ok(true),
            Err(_) => {
                // Check if it's a directory
                self.is_dir(path)
            }
        }
    }

    /// Parse a directory listing line (UNIX format)
    fn parse_list_line(&self, line: &str) -> Result<FtpFileInfo> {
        // Parse UNIX ls -l format:
        // drwxr-xr-x 2 user group 4096 Jan 01 00:00 filename
        // -rw-r--r-- 1 user group 1234 Jan 01 00:00 filename

        let parts: Vec<&str> = line.split_whitespace().collect();

        if parts.len() < 9 {
            return Err(anyhow::anyhow!("Invalid listing format"));
        }

        let permissions_str = parts[0];
        let is_dir = permissions_str.starts_with('d');

        // Parse size (5th field)
        let size = parts[4].parse::<u64>().unwrap_or(0);

        // Parse date (fields 5-7) and filename (rest)
        let name_parts = &parts[8..];
        let name = name_parts.join(" ");

        // Build full path
        let path = if self.current_dir.ends_with('/') {
            format!("{}{}", self.current_dir, name)
        } else {
            format!("{}/{}", self.current_dir, name)
        };

        // Parse permissions
        let permissions = Self::parse_permissions(permissions_str);

        Ok(FtpFileInfo {
            name,
            path,
            size,
            is_dir,
            permissions,
            modified_time: None, // Parsing time is complex and may vary by server
        })
    }

    /// Parse UNIX permission string to numeric mode
    fn parse_permissions(perm_str: &str) -> u32 {
        let mut mode: u32 = 0;

        if perm_str.len() >= 10 {
            // Owner permissions
            if perm_str.chars().nth(1) == Some('r') {
                mode |= 0o400;
            }
            if perm_str.chars().nth(2) == Some('w') {
                mode |= 0o200;
            }
            if perm_str.chars().nth(3) == Some('x') {
                mode |= 0o100;
            }

            // Group permissions
            if perm_str.chars().nth(4) == Some('r') {
                mode |= 0o040;
            }
            if perm_str.chars().nth(5) == Some('w') {
                mode |= 0o020;
            }
            if perm_str.chars().nth(6) == Some('x') {
                mode |= 0o010;
            }

            // Other permissions
            if perm_str.chars().nth(7) == Some('r') {
                mode |= 0o004;
            }
            if perm_str.chars().nth(8) == Some('w') {
                mode |= 0o002;
            }
            if perm_str.chars().nth(9) == Some('x') {
                mode |= 0o001;
            }

            // Directory flag
            if perm_str.starts_with('d') {
                mode |= 0o040000;
            }
        }

        mode
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_permissions() {
        let perm = FtpConnection::parse_permissions("drwxr-xr-x");
        assert_eq!(perm, 0o040755);

        let perm = FtpConnection::parse_permissions("-rw-r--r--");
        assert_eq!(perm, 0o0644);

        let perm = FtpConnection::parse_permissions("-rwxrwxrwx");
        assert_eq!(perm, 0o777);
    }
}
