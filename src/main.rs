//! Main entry point for rustftpfs - FTP Filesystem in Userspace
//!
//! This program mounts FTP servers as local directories using FUSE.

use std::env;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Arg, ArgAction, Command};
use env_logger::Env;
use fuser::MountOption;
use log::{debug, error, info};
use url::Url;

use rustftpfs::filesystem::FtpFs;
use rustftpfs::ftp::FtpConnection;

fn main() -> Result<()> {
    // Initialize logger
    env_logger::Builder::from_env(Env::default().default_filter_or("info"))
        .format_timestamp(None)
        .init();

    let matches = Command::new("rustftpfs")
        .version("0.1.0")
        .author("Kimi AI")
        .about("Mount FTP hosts as local directories using FUSE")
        .arg(
            Arg::new("ftp_url")
                .help("FTP URL in format ftp://[user[:password]@]host[:port][/path]")
                .required(true)
                .index(1),
        )
        .arg(
            Arg::new("mountpoint")
                .help("Local directory to mount the FTP filesystem")
                .required(true)
                .index(2),
        )
        .arg(
            Arg::new("user")
                .short('u')
                .long("user")
                .help("Username for FTP authentication")
                .value_name("USERNAME"),
        )
        .arg(
            Arg::new("password")
                .short('p')
                .long("password")
                .help("Password for FTP authentication")
                .value_name("PASSWORD"),
        )
        .arg(
            Arg::new("port")
                .short('P')
                .long("port")
                .help("FTP port (default: 21)")
                .value_name("PORT")
                .value_parser(clap::value_parser!(u16)),
        )
        .arg(
            Arg::new("tls")
                .long("tls")
                .help("Use TLS/SSL encryption")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("read_only")
                .short('r')
                .long("read-only")
                .help("Mount filesystem as read-only")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("foreground")
                .short('f')
                .long("foreground")
                .help("Run in foreground mode")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("debug")
                .short('d')
                .long("debug")
                .help("Enable debug output")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("allow_other")
                .long("allow-other")
                .help("Allow other users to access the mount")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("uid")
                .long("uid")
                .help("Set file owner UID")
                .value_name("UID")
                .value_parser(clap::value_parser!(u32)),
        )
        .arg(
            Arg::new("gid")
                .long("gid")
                .help("Set file group GID")
                .value_name("GID")
                .value_parser(clap::value_parser!(u32)),
        )
        .arg(
            Arg::new("umask")
                .long("umask")
                .help("Set file permissions umask")
                .value_name("UMASK")
                .value_parser(clap::value_parser!(u16)),
        )
        .get_matches();

    // Reinitialize logger if debug flag is set
    if matches.get_flag("debug") {
        env_logger::Builder::from_env(Env::default().default_filter_or("debug"))
            .format_timestamp(None)
            .init();
    }

    let ftp_url_str = matches.get_one::<String>("ftp_url").unwrap();
    let mountpoint_str = matches.get_one::<String>("mountpoint").unwrap();

    debug!("FTP URL: {}", ftp_url_str);
    debug!("Mountpoint: {}", mountpoint_str);

    // Parse FTP URL
    let (server, username, password, port, path) = parse_ftp_url(ftp_url_str)?;

    // Override with command line arguments if provided
    let username = matches
        .get_one::<String>("user")
        .map(|s| s.to_string())
        .or(username);
    let password = matches
        .get_one::<String>("password")
        .map(|s| s.to_string())
        .or(password);
    let port = matches.get_one::<u16>("port").copied().or(port);
    let use_tls = matches.get_flag("tls");

    // Validate username
    if username.is_none() {
        return Err(anyhow::anyhow!(
            "Username is required. Use --user flag or include in FTP URL"
        ));
    }

    let username = username.unwrap();
    let password = password.unwrap_or_else(|| "".to_string());

    info!("Connecting to FTP server: {}", server);
    info!("Username: {}", username);
    info!("Port: {:?}", port);
    info!("TLS: {}", use_tls);
    info!("Path: {:?}", path);

    // Create FTP connection
    let ftp_conn = FtpConnection::new(
        server.clone(),
        username.clone(),
        password.clone(),
        use_tls,
        port,
    )
    .context("Failed to connect to FTP server")?;

    // Setup mountpoint
    let mountpoint = PathBuf::from(mountpoint_str);

    if !mountpoint.exists() {
        std::fs::create_dir_all(&mountpoint)
            .context(format!("Failed to create mountpoint: {:?}", mountpoint))?;
        info!("Created mountpoint: {:?}", mountpoint);
    }

    // Create filesystem
    let ftpfs = FtpFs::new(ftp_conn).context("Failed to create FTP filesystem")?;

    // Configure mount options
    let mut options = vec![
        MountOption::FSName(format!("rustftpfs@{}:{}", server, port.unwrap_or(21))),
        MountOption::AutoUnmount,
    ];

    if matches.get_flag("read_only") {
        options.push(MountOption::RO);
    }

    if matches.get_flag("allow_other") {
        options.push(MountOption::AllowOther);
    }

    // Note: Foreground mode is the default behavior of fuser::mount2
    // The --foreground flag is kept for CLI compatibility but doesn't need special handling

    info!("Mounting FTP filesystem...");
    info!("Mountpoint: {:?}", mountpoint);
    info!("Options: {:?}", options);

    // Mount filesystem
    let result = fuser::mount2(ftpfs, &mountpoint, &options);

    match result {
        Ok(()) => {
            info!("FTP filesystem mounted successfully");
            Ok(())
        }
        Err(e) => {
            error!("Failed to mount FTP filesystem: {}", e);
            Err(anyhow::anyhow!("Failed to mount FTP filesystem: {}", e))
        }
    }
}

/// Parse FTP URL into components
fn parse_ftp_url(
    url_str: &str,
) -> Result<(
    String,
    Option<String>,
    Option<String>,
    Option<u16>,
    Option<String>,
)> {
    // Ensure URL has protocol prefix
    let url_str = if !url_str.contains("://") {
        format!("ftp://{}", url_str)
    } else {
        url_str.to_string()
    };

    let url = Url::parse(&url_str).context("Failed to parse FTP URL")?;

    // Validate scheme
    if url.scheme() != "ftp" && url.scheme() != "ftps" {
        return Err(anyhow::anyhow!("URL scheme must be 'ftp://' or 'ftps://'"));
    }

    // Extract host
    let host = url
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("FTP URL must contain a host"))?
        .to_string();

    // Extract port
    let port = url.port();

    // Extract credentials
    let username = if !url.username().is_empty() {
        Some(url.username().to_string())
    } else {
        None
    };

    let password = url.password().map(|p| p.to_string());

    // Extract path (without leading slash for FTP)
    let path = if url.path().is_empty() || url.path() == "/" {
        None
    } else {
        Some(url.path().to_string())
    };

    Ok((host, username, password, port, path))
}
