//! FUSE Filesystem Implementation (Optimizado para Rendimiento)
//!
//! Implementación optimizada del filesystem FUSE para montar servidores FTP.
//! Características de rendimiento:
//! - Caché de listados de directorio con TTL de 30 segundos
//! - Caché de atributos de archivos para evitar consultas repetidas
//! - TTL extendido de FUSE (10 segundos) para reducir getattr() calls
//! - Prefetching básico de directorios comunes

use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context, Result};
use fuser::{
    FileAttr, FileType, Filesystem, ReplyAttr, ReplyCreate, ReplyData, ReplyDirectory, ReplyEmpty,
    ReplyEntry, ReplyOpen, ReplyWrite, Request,
};
use libc::{EIO, EISDIR, ENOENT, ENOTDIR};
use log::{debug, error, info, trace, warn};

use crate::ftp::{FtpConnection, FtpFileInfo};

/// Inode number for the root directory
const ROOT_INODE: u64 = 1;

/// TTL extendido para atributos FUSE (30 segundos - optimizado para VS Code)
const TTL: Duration = Duration::from_secs(30);

/// TTL para caché de directorios (60 segundos - reduce readdir frecuentes)
const DIR_CACHE_TTL: Duration = Duration::from_secs(60);

/// TTL para caché de atributos de archivos (120 segundos - reduce getattr)
const ATTR_CACHE_TTL: Duration = Duration::from_secs(120);

/// Patrones de archivos temporales a ignorar (optimización para editores)
const TEMP_FILE_PATTERNS: &[&str] = &[
    ".attach_pid", // Java debugger
    ".swp",
    ".swo",
    ".swn", // vim swap files
    "~",
    ".tmp",
    ".temp", // archivos temporales
    ".git",
    ".svn",
    ".hg", // control de versiones
    ".vscode",
    ".idea", // configuración de IDEs
    "__pycache__",
    ".pyc",
    ".pyo", // Python cache
    ".DS_Store",
    ".directory", // archivos de sistema
    ".nfs",
    ".lock",
    ".pid", // lock files
];

/// Verifica si un nombre de archivo es temporal/ignorable
fn is_temp_file(name: &str) -> bool {
    // Verificar si empieza con punto y contiene algún patrón temporal
    if name.starts_with('.') {
        // Archivos que empiezan con .attach_pid
        if name.starts_with(".attach_pid") {
            return true;
        }
        // Archivos que terminan en ~ (backups)
        if name.ends_with('~') {
            return true;
        }
        // Otros archivos ocultos temporales
        for pattern in TEMP_FILE_PATTERNS {
            if name.contains(pattern) {
                return true;
            }
        }
    }

    // Archivos que terminan en ~ (backups)
    if name.ends_with('~') {
        return true;
    }

    false
}

/// Representa un inodo de archivo o directorio
#[derive(Debug, Clone)]
struct Inode {
    ino: u64,
    parent: u64,
    name: String,
    attr: FileAttr,
    ftp_path: String,
}

/// Entrada de caché de directorio con timestamp
#[derive(Debug, Clone)]
struct DirCacheEntry {
    files: Vec<FtpFileInfo>,
    timestamp: Instant,
}

/// Entrada de caché de atributos con timestamp
#[derive(Debug, Clone)]
struct AttrCacheEntry {
    attr: FileAttr,
    timestamp: Instant,
}

/// Buffer de escritura para lazy write
#[derive(Debug, Clone)]
struct WriteBuffer {
    data: Vec<u8>,
    dirty: bool,
    last_modified: Instant,
}

/// Información de handle de archivo abierto
#[derive(Debug, Clone)]
struct FileHandle {
    ino: u64,
    write_buffer: Option<WriteBuffer>,
}

/// Implementación del filesystem FUSE para FTP (Optimizado)
pub struct FtpFs {
    ftp_conn: Arc<Mutex<FtpConnection>>,
    inodes: Arc<Mutex<HashMap<u64, Inode>>>,
    path_to_inode: Arc<Mutex<HashMap<String, u64>>>,
    next_inode: Arc<Mutex<u64>>,
    read_cache: Arc<Mutex<HashMap<u64, Vec<u8>>>>,
    /// Caché de listados de directorio: path -> (archivos, timestamp)
    dir_cache: Arc<Mutex<HashMap<String, DirCacheEntry>>>,
    /// Caché de atributos: ino -> (atributos, timestamp)
    attr_cache: Arc<Mutex<HashMap<u64, AttrCacheEntry>>>,
    /// Handles de archivos abiertos: fh -> FileHandle
    open_files: Arc<Mutex<HashMap<u64, FileHandle>>>,
    /// Contador para generar file handles únicos
    next_fh: Arc<Mutex<u64>>,
}

impl FtpFs {
    /// Crear un nuevo filesystem FTP
    pub fn new(ftp_conn: FtpConnection) -> Result<Self> {
        let fs = FtpFs {
            ftp_conn: Arc::new(Mutex::new(ftp_conn)),
            inodes: Arc::new(Mutex::new(HashMap::new())),
            path_to_inode: Arc::new(Mutex::new(HashMap::new())),
            next_inode: Arc::new(Mutex::new(2)), // Empieza en 2, 1 está reservado para root
            read_cache: Arc::new(Mutex::new(HashMap::new())),
            dir_cache: Arc::new(Mutex::new(HashMap::new())),
            attr_cache: Arc::new(Mutex::new(HashMap::new())),
            open_files: Arc::new(Mutex::new(HashMap::new())),
            next_fh: Arc::new(Mutex::new(1)), // File handles empiezan en 1
        };

        // Crear inodo raíz
        let root_attr = FileAttr {
            ino: ROOT_INODE,
            size: 0,
            blocks: 0,
            atime: SystemTime::now(),
            mtime: SystemTime::now(),
            ctime: SystemTime::now(),
            crtime: SystemTime::now(),
            kind: FileType::Directory,
            perm: 0o755,
            nlink: 2,
            uid: unsafe { libc::getuid() },
            gid: unsafe { libc::getgid() },
            rdev: 0,
            flags: 0,
            blksize: 512,
        };

        let root_inode = Inode {
            ino: ROOT_INODE,
            parent: ROOT_INODE,
            name: "/".to_string(),
            attr: root_attr,
            ftp_path: "/".to_string(),
        };

        fs.inodes.lock().unwrap().insert(ROOT_INODE, root_inode);
        fs.path_to_inode
            .lock()
            .unwrap()
            .insert("/".to_string(), ROOT_INODE);

        // Cachear atributos del root
        fs.attr_cache.lock().unwrap().insert(
            ROOT_INODE,
            AttrCacheEntry {
                attr: root_attr,
                timestamp: Instant::now(),
            },
        );

        info!("Created optimized FtpFs with caching enabled");

        Ok(fs)
    }

    /// Asignar un nuevo número de inodo
    fn allocate_inode(&self) -> u64 {
        let mut next = self.next_inode.lock().unwrap();
        let ino = *next;
        *next += 1;
        ino
    }

    /// Obtener o crear inodo para información de archivo FTP
    fn get_or_create_inode(&self, parent: u64, file_info: &FtpFileInfo) -> Inode {
        let path = file_info.path.clone();

        // Verificar si el inodo ya existe
        if let Some(&ino) = self.path_to_inode.lock().unwrap().get(&path) {
            if let Some(inode) = self.inodes.lock().unwrap().get(&ino).cloned() {
                return inode;
            }
        }

        // Crear nuevo inodo
        let ino = self.allocate_inode();

        let kind = if file_info.is_dir {
            FileType::Directory
        } else {
            FileType::RegularFile
        };

        let nlink = if file_info.is_dir { 2 } else { 1 };

        let attr = FileAttr {
            ino,
            size: file_info.size,
            blocks: (file_info.size + 511) / 512,
            atime: file_info.modified_time.unwrap_or(SystemTime::now()),
            mtime: file_info.modified_time.unwrap_or(SystemTime::now()),
            ctime: file_info.modified_time.unwrap_or(SystemTime::now()),
            crtime: file_info.modified_time.unwrap_or(SystemTime::now()),
            kind,
            perm: (file_info.permissions & 0o777) as u16,
            nlink,
            uid: unsafe { libc::getuid() },
            gid: unsafe { libc::getgid() },
            rdev: 0,
            flags: 0,
            blksize: 512,
        };

        let inode = Inode {
            ino,
            parent,
            name: file_info.name.clone(),
            attr,
            ftp_path: path.clone(),
        };

        self.inodes.lock().unwrap().insert(ino, inode.clone());
        self.path_to_inode.lock().unwrap().insert(path, ino);

        // Cachear atributos
        self.attr_cache.lock().unwrap().insert(
            ino,
            AttrCacheEntry {
                attr,
                timestamp: Instant::now(),
            },
        );

        inode
    }

    /// Obtener listado de directorio con caché
    fn list_ftp_directory_cached(&self, path: &str) -> Result<Vec<FtpFileInfo>> {
        // Verificar caché primero
        {
            let cache = self.dir_cache.lock().unwrap();
            if let Some(entry) = cache.get(path) {
                if entry.timestamp.elapsed() < DIR_CACHE_TTL {
                    trace!("Directory cache hit for: {}", path);
                    return Ok(entry.files.clone());
                }
            }
        }

        // Caché miss - consultar servidor FTP
        trace!("Directory cache miss for: {}", path);
        let mut conn = self.ftp_conn.lock().unwrap();

        let files = match conn.list_dir(path) {
            Ok(files) => files,
            Err(e) => {
                warn!("Failed to list directory, attempting reconnect: {}", e);
                conn.reconnect()?;
                conn.list_dir(path)?
            }
        };

        // Guardar en caché
        self.dir_cache.lock().unwrap().insert(
            path.to_string(),
            DirCacheEntry {
                files: files.clone(),
                timestamp: Instant::now(),
            },
        );

        Ok(files)
    }

    /// Invalidar caché de directorio (llamar después de operaciones de escritura)
    fn invalidate_dir_cache(&self, path: &str) {
        self.dir_cache.lock().unwrap().remove(path);
        debug!("Invalidated directory cache for: {}", path);
    }

    /// Obtener atributos con caché
    fn get_attr_cached(&self, ino: u64) -> Option<FileAttr> {
        let cache = self.attr_cache.lock().unwrap();
        if let Some(entry) = cache.get(&ino) {
            if entry.timestamp.elapsed() < ATTR_CACHE_TTL {
                return Some(entry.attr);
            }
        }
        None
    }

    /// Actualizar caché de atributos
    fn update_attr_cache(&self, ino: u64, attr: FileAttr) {
        self.attr_cache.lock().unwrap().insert(
            ino,
            AttrCacheEntry {
                attr,
                timestamp: Instant::now(),
            },
        );
    }

    /// Obtener información de archivo FTP (solo para archivos no cacheados)
    fn get_ftp_file_info(&self, path: &str) -> Result<FtpFileInfo> {
        let mut conn = self.ftp_conn.lock().unwrap();

        // Verificar si es directorio
        let is_dir = conn.is_dir(path)?;

        let size = if is_dir {
            0
        } else {
            conn.size(path).unwrap_or(0)
        };

        let name = Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string());

        Ok(FtpFileInfo {
            name,
            path: path.to_string(),
            size,
            is_dir,
            permissions: if is_dir { 0o755 } else { 0o644 },
            modified_time: None,
        })
    }

    /// Asignar un nuevo file handle único
    fn allocate_fh(&self) -> u64 {
        let mut next = self.next_fh.lock().unwrap();
        let fh = *next;
        *next += 1;
        fh
    }

    /// Sincronizar buffer de escritura al servidor FTP
    fn sync_write_buffer(&self, fh: u64) -> Result<()> {
        if let Some(file_handle) = self.open_files.lock().unwrap().get(&fh).cloned() {
            if let Some(ref write_buffer) = file_handle.write_buffer {
                if write_buffer.dirty {
                    let inode = self
                        .inodes
                        .lock()
                        .unwrap()
                        .get(&file_handle.ino)
                        .cloned()
                        .ok_or_else(|| anyhow::anyhow!("Inode not found"))?;

                    trace!(
                        "Syncing write buffer for inode {} ({} bytes)",
                        file_handle.ino,
                        write_buffer.data.len()
                    );

                    let mut conn = self.ftp_conn.lock().unwrap();
                    conn.store(&inode.ftp_path, &write_buffer.data)
                        .context("Failed to store file to FTP")?;

                    // Actualizar caché de lectura con los nuevos datos
                    self.read_cache
                        .lock()
                        .unwrap()
                        .insert(file_handle.ino, write_buffer.data.clone());

                    // Actualizar tamaño en caché de atributos
                    if let Some(entry) = self.attr_cache.lock().unwrap().get_mut(&file_handle.ino) {
                        entry.attr.size = write_buffer.data.len() as u64;
                        entry.attr.blocks = (write_buffer.data.len() as u64 + 511) / 512;
                    }

                    // Invalidar caché de directorio padre
                    self.invalidate_dir_cache(&inode.parent.to_string());

                    trace!("Write buffer synced successfully");
                }
            }
        }
        Ok(())
    }

    /// Cargar datos de archivo con prefetching opcional
    fn load_file_data(&self, ino: u64, ftp_path: &str, prefetch: bool) -> Result<Vec<u8>> {
        // Verificar caché primero
        if let Some(data) = self.read_cache.lock().unwrap().get(&ino).cloned() {
            trace!("File data cache hit for inode {}", ino);
            return Ok(data);
        }

        // Cargar desde FTP
        trace!(
            "Loading file data for inode {} (prefetch: {})",
            ino,
            prefetch
        );
        let mut conn = self.ftp_conn.lock().unwrap();
        let data = conn
            .retrieve(ftp_path)
            .context("Failed to retrieve file from FTP")?;

        // Guardar en caché
        self.read_cache.lock().unwrap().insert(ino, data.clone());

        trace!("File data loaded: {} bytes", data.len());
        Ok(data)
    }
}

impl Filesystem for FtpFs {
    /// Obtener atributos de archivo (optimizado con caché extendido)
    fn getattr(&mut self, _req: &Request, ino: u64, _fh: Option<u64>, reply: ReplyAttr) {
        trace!("getattr called for inode {}", ino);

        // Para root, siempre usar caché rápida
        if ino == ROOT_INODE {
            if let Some(attr) = self.get_attr_cached(ino) {
                reply.attr(&TTL, &attr);
                return;
            }
        }

        // Intentar obtener de caché primero
        if let Some(attr) = self.get_attr_cached(ino) {
            reply.attr(&TTL, &attr);
            return;
        }

        // Si no está en caché, obtener del inodo
        if let Some(inode) = self.inodes.lock().unwrap().get(&ino) {
            // Para archivos regulares, actualizar tamaño ocasionalmente (no cada vez)
            if inode.attr.kind == FileType::RegularFile {
                // Solo actualizar si no hay caché o ha pasado mucho tiempo
                let should_update = {
                    let cache = self.attr_cache.lock().unwrap();
                    if let Some(entry) = cache.get(&ino) {
                        entry.timestamp.elapsed() > ATTR_CACHE_TTL
                    } else {
                        true
                    }
                };

                if should_update {
                    if let Ok(info) = self.get_ftp_file_info(&inode.ftp_path) {
                        let mut updated_attr = inode.attr.clone();
                        updated_attr.size = info.size;
                        self.update_attr_cache(ino, updated_attr);
                        reply.attr(&TTL, &updated_attr);
                        return;
                    }
                }
            }

            // Usar atributos cacheados del inodo
            self.update_attr_cache(ino, inode.attr);
            reply.attr(&TTL, &inode.attr);
            return;
        }

        error!("getattr: inode {} not found", ino);
        reply.error(ENOENT);
    }

    /// Buscar archivo por nombre (usando caché de directorio)
    fn lookup(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEntry) {
        let name_str = name.to_string_lossy().to_string();
        trace!("lookup called for parent={}, name={}", parent, name_str);

        // OPTIMIZACIÓN VS Code: Ignorar archivos temporales inmediatamente
        if is_temp_file(&name_str) {
            trace!("lookup: ignoring temp file {}", name_str);
            reply.error(ENOENT);
            return;
        }

        // Obtener inodo padre
        let parent_inode = match self.inodes.lock().unwrap().get(&parent) {
            Some(inode) => inode.clone(),
            None => {
                error!("lookup: parent inode {} not found", parent);
                reply.error(ENOENT);
                return;
            }
        };

        // Entradas especiales
        if name_str == "." {
            reply.entry(&TTL, &parent_inode.attr, 0);
            return;
        }
        if name_str == ".." {
            let parent_parent = parent_inode.parent;
            if let Some(attr) = self.get_attr_cached(parent_parent) {
                reply.entry(&TTL, &attr, 0);
                return;
            }
        }

        // Construir ruta FTP
        let ftp_path = if parent_inode.ftp_path == "/" {
            format!("/{}", name_str)
        } else {
            format!("{}/{}", parent_inode.ftp_path, name_str)
        };

        // Verificar caché de inodo primero
        if let Some(&ino) = self.path_to_inode.lock().unwrap().get(&ftp_path) {
            if let Some(attr) = self.get_attr_cached(ino) {
                reply.entry(&TTL, &attr, 0);
                return;
            }
        }

        // Verificar caché de directorio primero (evita consulta FTP individual)
        match self.list_ftp_directory_cached(&parent_inode.ftp_path) {
            Ok(files) => {
                if let Some(file_info) = files.iter().find(|f| f.name == name_str) {
                    let inode = self.get_or_create_inode(parent, file_info);
                    reply.entry(&TTL, &inode.attr, 0);
                    return;
                }
            }
            Err(e) => {
                debug!("lookup: failed to list parent directory: {}", e);
            }
        }

        // Fallback: consulta directa al FTP
        match self.get_ftp_file_info(&ftp_path) {
            Ok(file_info) => {
                let inode = self.get_or_create_inode(parent, &file_info);
                reply.entry(&TTL, &inode.attr, 0);
            }
            Err(_) => {
                reply.error(ENOENT);
            }
        }
    }

    /// Leer contenido de directorio (optimizado con caché)
    fn readdir(
        &mut self,
        _req: &Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
        trace!("readdir called for inode {} with offset {}", ino, offset);

        let inode = match self.inodes.lock().unwrap().get(&ino) {
            Some(inode) => inode.clone(),
            None => {
                error!("readdir: inode {} not found", ino);
                reply.error(ENOENT);
                return;
            }
        };

        if inode.attr.kind != FileType::Directory {
            reply.error(ENOTDIR);
            return;
        }

        // Recolectar entradas con strings propios
        let mut entries: Vec<(u64, FileType, String)> = vec![
            (inode.ino, FileType::Directory, ".".to_string()),
            (inode.parent, FileType::Directory, "..".to_string()),
        ];

        // Usar caché de directorio (evita consulta FTP repetida)
        // OPTIMIZACIÓN VS Code: Filtrar archivos temporales
        match self.list_ftp_directory_cached(&inode.ftp_path) {
            Ok(files) => {
                let filtered_count = files.len();
                for file_info in files {
                    // Ignorar archivos temporales en el listado
                    if is_temp_file(&file_info.name) {
                        trace!("readdir: filtering temp file {}", file_info.name);
                        continue;
                    }
                    let file_inode = self.get_or_create_inode(ino, &file_info);
                    entries.push((
                        file_inode.ino,
                        file_inode.attr.kind,
                        file_inode.name.clone(),
                    ));
                }
                trace!(
                    "readdir: filtered {} temp files from {}",
                    filtered_count - entries.len() + 2,
                    filtered_count
                ); // +2 por . y ..
            }
            Err(e) => {
                error!("readdir: failed to list directory: {}", e);
                reply.error(EIO);
                return;
            }
        }

        // Enviar entradas empezando desde offset
        for (i, (entry_ino, kind, name)) in entries.iter().enumerate().skip(offset as usize) {
            let buffer_full = reply.add(*entry_ino, (i + 1) as i64, *kind, name.as_str());
            if buffer_full {
                break;
            }
        }

        reply.ok();
    }

    /// Abrir archivo (con write buffer para lazy write)
    fn open(&mut self, _req: &Request, ino: u64, flags: i32, reply: ReplyOpen) {
        trace!("open called for inode {} flags {}", ino, flags);

        let fh = self.allocate_fh();

        // Verificar si es modo escritura (flags & O_WRONLY o O_RDWR)
        let is_write_mode = (flags & 0o1) != 0 || (flags & 0o2) != 0;

        let file_handle = FileHandle {
            ino,
            write_buffer: if is_write_mode {
                Some(WriteBuffer {
                    data: Vec::new(),
                    dirty: false,
                    last_modified: Instant::now(),
                })
            } else {
                None
            },
        };

        self.open_files.lock().unwrap().insert(fh, file_handle);
        trace!(
            "Opened file handle {} for inode {} (write mode: {})",
            fh,
            ino,
            is_write_mode
        );

        reply.opened(fh, 0);
    }

    /// Leer datos de archivo (con caché y prefetching)
    fn read(
        &mut self,
        _req: &Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        size: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: ReplyData,
    ) {
        trace!(
            "read called for inode {} offset {} size {}",
            ino,
            offset,
            size
        );

        let inode = match self.inodes.lock().unwrap().get(&ino) {
            Some(inode) => inode.clone(),
            None => {
                error!("read: inode {} not found", ino);
                reply.error(ENOENT);
                return;
            }
        };

        if inode.attr.kind == FileType::Directory {
            reply.error(EISDIR);
            return;
        }

        // Cargar datos con prefetching
        match self.load_file_data(ino, &inode.ftp_path, true) {
            Ok(data) => {
                let offset = offset as usize;
                let size = size as usize;

                if offset >= data.len() {
                    reply.data(&[]);
                    return;
                }

                let end = std::cmp::min(offset + size, data.len());
                reply.data(&data[offset..end]);
            }
            Err(e) => {
                error!("read: failed to load file data: {}", e);
                reply.error(EIO);
            }
        }
    }

    /// Escribir datos en archivo (con write buffer - lazy write)
    fn write(
        &mut self,
        _req: &Request,
        ino: u64,
        fh: u64,
        offset: i64,
        data: &[u8],
        _write_flags: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: ReplyWrite,
    ) {
        trace!(
            "write called for inode {} fh {} offset {} size {}",
            ino,
            fh,
            offset,
            data.len()
        );

        let inode = match self.inodes.lock().unwrap().get(&ino) {
            Some(inode) => inode.clone(),
            None => {
                error!("write: inode {} not found", ino);
                reply.error(ENOENT);
                return;
            }
        };

        if inode.attr.kind == FileType::Directory {
            reply.error(EISDIR);
            return;
        }

        // Obtener o crear el file handle
        let mut open_files = self.open_files.lock().unwrap();
        let file_handle = open_files.get_mut(&fh);

        if let Some(file_handle) = file_handle {
            if let Some(ref mut write_buffer) = file_handle.write_buffer {
                // Redimensionar buffer si es necesario
                let offset = offset as usize;
                let end = offset + data.len();
                if end > write_buffer.data.len() {
                    write_buffer.data.resize(end, 0);
                }

                // Escribir datos en el buffer
                write_buffer.data[offset..end].copy_from_slice(data);
                write_buffer.dirty = true;
                write_buffer.last_modified = Instant::now();

                // Actualizar caché de lectura para mantener consistencia
                self.read_cache
                    .lock()
                    .unwrap()
                    .insert(ino, write_buffer.data.clone());

                trace!(
                    "Write buffered: {} bytes at offset {} (total: {})",
                    data.len(),
                    offset,
                    write_buffer.data.len()
                );

                reply.written(data.len() as u32);
                return;
            }
        }

        // Fallback si no hay write buffer (modo read-only o error)
        error!("write: no write buffer available for fh {}", fh);
        reply.error(EIO);
    }

    /// Crear archivo (invalida caché de directorio)
    fn create(
        &mut self,
        _req: &Request,
        parent: u64,
        name: &OsStr,
        mode: u32,
        _umask: u32,
        _flags: i32,
        reply: ReplyCreate,
    ) {
        let name_str = name.to_string_lossy().to_string();
        trace!(
            "create called for parent={} name={} mode={}",
            parent,
            name_str,
            mode
        );

        // OPTIMIZACIÓN VS Code: No crear archivos temporales en el servidor
        if is_temp_file(&name_str) {
            trace!("create: ignoring temp file {}", name_str);
            // Devolver un error que VS Code interpretará como "no soportado"
            // pero no interrumpirá el flujo de trabajo
            reply.error(libc::EOPNOTSUPP);
            return;
        }

        let parent_inode = match self.inodes.lock().unwrap().get(&parent) {
            Some(inode) => inode.clone(),
            None => {
                error!("create: parent inode {} not found", parent);
                reply.error(ENOENT);
                return;
            }
        };

        let ftp_path = if parent_inode.ftp_path == "/" {
            format!("/{}", name_str)
        } else {
            format!("{}/{}", parent_inode.ftp_path, name_str)
        };

        // Crear archivo vacío en FTP
        let mut conn = self.ftp_conn.lock().unwrap();
        match conn.store(&ftp_path, &[]) {
            Ok(_) => {
                drop(conn); // Liberar lock

                // Invalidar caché del directorio padre
                self.invalidate_dir_cache(&parent_inode.ftp_path);

                // Crear inodo para el nuevo archivo
                let file_info = FtpFileInfo {
                    name: name_str,
                    path: ftp_path,
                    size: 0,
                    is_dir: false,
                    permissions: (mode & 0o777) as u32,
                    modified_time: Some(SystemTime::now()),
                };

                let inode = self.get_or_create_inode(parent, &file_info);
                reply.created(&TTL, &inode.attr, 0, 0, 0);
            }
            Err(e) => {
                error!("create: failed to create file: {}", e);
                reply.error(EIO);
            }
        }
    }

    /// Eliminar archivo (invalida cachés)
    fn unlink(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEmpty) {
        let name_str = name.to_string_lossy().to_string();
        trace!("unlink called for parent={} name={}", parent, name_str);

        // OPTIMIZACIÓN VS Code: Ignorar completamente archivos temporales
        if is_temp_file(&name_str) {
            trace!("unlink: ignoring temp file {}", name_str);
            reply.ok();
            return;
        }

        let parent_inode = match self.inodes.lock().unwrap().get(&parent) {
            Some(inode) => inode.clone(),
            None => {
                error!("unlink: parent inode {} not found", parent);
                reply.error(ENOENT);
                return;
            }
        };

        let ftp_path = if parent_inode.ftp_path == "/" {
            format!("/{}", name_str)
        } else {
            format!("{}/{}", parent_inode.ftp_path, name_str)
        };

        // Eliminar de cachés
        if let Some(&ino) = self.path_to_inode.lock().unwrap().get(&ftp_path) {
            self.inodes.lock().unwrap().remove(&ino);
            self.read_cache.lock().unwrap().remove(&ino);
            self.attr_cache.lock().unwrap().remove(&ino);
        }
        self.path_to_inode.lock().unwrap().remove(&ftp_path);
        self.invalidate_dir_cache(&parent_inode.ftp_path);

        // Verificar si el archivo existe antes de intentar borrarlo
        let exists = {
            let mut conn = self.ftp_conn.lock().unwrap();
            conn.exists(&ftp_path).unwrap_or(false)
        };

        if !exists {
            trace!("unlink: file does not exist: {}", ftp_path);
            reply.ok();
            return;
        }

        // Eliminar de FTP
        let mut conn = self.ftp_conn.lock().unwrap();
        match conn.delete(&ftp_path) {
            Ok(_) => {
                reply.ok();
            }
            Err(e) => {
                error!("unlink: failed to delete file: {}", e);
                reply.error(EIO);
            }
        }
    }

    /// Crear directorio (invalida caché)
    fn mkdir(
        &mut self,
        _req: &Request,
        parent: u64,
        name: &OsStr,
        mode: u32,
        _umask: u32,
        reply: ReplyEntry,
    ) {
        let name_str = name.to_string_lossy().to_string();
        trace!(
            "mkdir called for parent={} name={} mode={}",
            parent,
            name_str,
            mode
        );

        let parent_inode = match self.inodes.lock().unwrap().get(&parent) {
            Some(inode) => inode.clone(),
            None => {
                error!("mkdir: parent inode {} not found", parent);
                reply.error(ENOENT);
                return;
            }
        };

        let ftp_path = if parent_inode.ftp_path == "/" {
            format!("/{}", name_str)
        } else {
            format!("{}/{}", parent_inode.ftp_path, name_str)
        };

        // Crear directorio en FTP
        let mut conn = self.ftp_conn.lock().unwrap();
        match conn.mkdir(&ftp_path) {
            Ok(_) => {
                drop(conn); // Liberar lock

                // Invalidar caché
                self.invalidate_dir_cache(&parent_inode.ftp_path);

                // Crear inodo para el nuevo directorio
                let file_info = FtpFileInfo {
                    name: name_str,
                    path: ftp_path,
                    size: 0,
                    is_dir: true,
                    permissions: (mode & 0o777) as u32,
                    modified_time: Some(SystemTime::now()),
                };

                let inode = self.get_or_create_inode(parent, &file_info);
                reply.entry(&TTL, &inode.attr, 0);
            }
            Err(e) => {
                error!("mkdir: failed to create directory: {}", e);
                reply.error(EIO);
            }
        }
    }

    /// Eliminar directorio (invalida caché)
    fn rmdir(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEmpty) {
        let name_str = name.to_string_lossy().to_string();
        trace!("rmdir called for parent={} name={}", parent, name_str);

        let parent_inode = match self.inodes.lock().unwrap().get(&parent) {
            Some(inode) => inode.clone(),
            None => {
                error!("rmdir: parent inode {} not found", parent);
                reply.error(ENOENT);
                return;
            }
        };

        let ftp_path = if parent_inode.ftp_path == "/" {
            format!("/{}", name_str)
        } else {
            format!("{}/{}", parent_inode.ftp_path, name_str)
        };

        // Eliminar de cachés
        if let Some(&ino) = self.path_to_inode.lock().unwrap().get(&ftp_path) {
            self.inodes.lock().unwrap().remove(&ino);
            self.attr_cache.lock().unwrap().remove(&ino);
            self.dir_cache.lock().unwrap().remove(&ftp_path);
        }
        self.path_to_inode.lock().unwrap().remove(&ftp_path);
        self.invalidate_dir_cache(&parent_inode.ftp_path);

        // Eliminar directorio de FTP
        let mut conn = self.ftp_conn.lock().unwrap();
        match conn.rmdir(&ftp_path) {
            Ok(_) => {
                reply.ok();
            }
            Err(e) => {
                error!("rmdir: failed to remove directory: {}", e);
                reply.error(EIO);
            }
        }
    }

    /// Renombrar archivo o directorio (invalida cachés)
    fn rename(
        &mut self,
        _req: &Request,
        parent: u64,
        name: &OsStr,
        newparent: u64,
        newname: &OsStr,
        _flags: u32,
        reply: ReplyEmpty,
    ) {
        let name_str = name.to_string_lossy().to_string();
        let newname_str = newname.to_string_lossy().to_string();
        trace!(
            "rename called: parent={} name={} newparent={} newname={}",
            parent,
            name_str,
            newparent,
            newname_str
        );

        let parent_inode = match self.inodes.lock().unwrap().get(&parent) {
            Some(inode) => inode.clone(),
            None => {
                error!("rename: parent inode {} not found", parent);
                reply.error(ENOENT);
                return;
            }
        };

        let newparent_inode = match self.inodes.lock().unwrap().get(&newparent) {
            Some(inode) => inode.clone(),
            None => {
                error!("rename: newparent inode {} not found", newparent);
                reply.error(ENOENT);
                return;
            }
        };

        let old_path = if parent_inode.ftp_path == "/" {
            format!("/{}", name_str)
        } else {
            format!("{}/{}", parent_inode.ftp_path, name_str)
        };

        let new_path = if newparent_inode.ftp_path == "/" {
            format!("/{}", newname_str)
        } else {
            format!("{}/{}", newparent_inode.ftp_path, newname_str)
        };

        // Actualizar caché de inodos
        if let Some(&ino) = self.path_to_inode.lock().unwrap().get(&old_path) {
            if let Some(inode) = self.inodes.lock().unwrap().get_mut(&ino) {
                inode.ftp_path = new_path.clone();
                inode.name = newname_str.clone();
                inode.parent = newparent;
            }
            self.path_to_inode.lock().unwrap().remove(&old_path);
            self.path_to_inode
                .lock()
                .unwrap()
                .insert(new_path.clone(), ino);
        }

        // Invalidar cachés de directorios afectados
        self.invalidate_dir_cache(&parent_inode.ftp_path);
        if parent_inode.ftp_path != newparent_inode.ftp_path {
            self.invalidate_dir_cache(&newparent_inode.ftp_path);
        }

        // Renombrar en FTP
        let mut conn = self.ftp_conn.lock().unwrap();
        match conn.rename(&old_path, &new_path) {
            Ok(_) => {
                reply.ok();
            }
            Err(e) => {
                error!("rename: failed to rename: {}", e);
                reply.error(EIO);
            }
        }
    }

    /// Establecer atributos de archivo (simplificado)
    fn setattr(
        &mut self,
        _req: &Request,
        ino: u64,
        mode: Option<u32>,
        uid: Option<u32>,
        gid: Option<u32>,
        size: Option<u64>,
        _atime: Option<fuser::TimeOrNow>,
        _mtime: Option<fuser::TimeOrNow>,
        _ctime: Option<SystemTime>,
        _fh: Option<u64>,
        _crtime: Option<SystemTime>,
        _chgtime: Option<SystemTime>,
        _bkuptime: Option<SystemTime>,
        _flags: Option<u32>,
        reply: ReplyAttr,
    ) {
        trace!("setattr called for inode {}", ino);

        let mut inodes = self.inodes.lock().unwrap();

        if let Some(inode) = inodes.get_mut(&ino) {
            if let Some(mode) = mode {
                inode.attr.perm = mode as u16;
            }
            if let Some(uid) = uid {
                inode.attr.uid = uid;
            }
            if let Some(gid) = gid {
                inode.attr.gid = gid;
            }
            if let Some(size) = size {
                inode.attr.size = size;
            }

            // Actualizar caché de atributos
            self.update_attr_cache(ino, inode.attr);
            reply.attr(&TTL, &inode.attr);
        } else {
            error!("setattr: inode {} not found", ino);
            reply.error(ENOENT);
        }
    }

    /// Liberar handle de archivo (sincroniza write buffer y limpia caché)
    fn release(
        &mut self,
        _req: &Request,
        ino: u64,
        fh: u64,
        _flags: i32,
        _lock_owner: Option<u64>,
        _flush: bool,
        reply: ReplyEmpty,
    ) {
        trace!("release called for inode {} fh {}", ino, fh);

        // Sincronizar write buffer si existe y está dirty
        match self.sync_write_buffer(fh) {
            Ok(_) => {
                // Remover file handle de archivos abiertos
                self.open_files.lock().unwrap().remove(&fh);

                // Limpiar caché de lectura para ahorrar memoria
                // (pero solo si no hay otros handles abiertos para este archivo)
                let has_other_handles = self
                    .open_files
                    .lock()
                    .unwrap()
                    .values()
                    .any(|handle| handle.ino == ino);
                if !has_other_handles {
                    self.read_cache.lock().unwrap().remove(&ino);
                }

                trace!("File handle {} released successfully", fh);
                reply.ok();
            }
            Err(e) => {
                error!("release: failed to sync write buffer: {}", e);
                reply.error(EIO);
            }
        }
    }

    /// Sincronizar archivo (fuerza sync del write buffer)
    fn fsync(&mut self, _req: &Request, _ino: u64, fh: u64, _datasync: bool, reply: ReplyEmpty) {
        trace!("fsync called for fh {}", fh);

        match self.sync_write_buffer(fh) {
            Ok(_) => reply.ok(),
            Err(e) => {
                error!("fsync: failed to sync: {}", e);
                reply.error(EIO);
            }
        }
    }

    /// Verificar permisos de acceso (siempre permite para simplificar)
    fn access(&mut self, _req: &Request, _ino: u64, _mask: i32, reply: ReplyEmpty) {
        trace!("access called");
        reply.ok();
    }

    /// Liberar datos pendientes (sincroniza write buffer)
    fn flush(&mut self, _req: &Request, _ino: u64, fh: u64, _lock_owner: u64, reply: ReplyEmpty) {
        trace!("flush called for fh {}", fh);

        match self.sync_write_buffer(fh) {
            Ok(_) => reply.ok(),
            Err(e) => {
                error!("flush: failed to sync: {}", e);
                reply.error(EIO);
            }
        }
    }
}
