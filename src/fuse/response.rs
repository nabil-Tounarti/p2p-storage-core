use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub enum FsResponse {
    DirEntries(Vec<FsDirEntry>),
    FileAttr(FsFileAttr),
    Data(Vec<u8>),
    Error(String),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FsDirEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FsFileAttr {
    /// Unique inode number (stable per peer)
    pub ino: u64,

    /// File size in bytes
    pub size: u64,

    /// Number of hard links
    pub nlink: u32,

    /// Unix permission bits (e.g. 0o755)
    pub perm: u16,

    /// File type
    pub kind: FsFileType,

    /// Last access time (unix seconds)
    pub atime: i64,

    /// Last modification time (unix seconds)
    pub mtime: i64,

    /// Last status change time (unix seconds)
    pub ctime: i64,

    /// Owner user id
    pub uid: u32,

    /// Owner group id
    pub gid: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FsFileType {
    File,
    Directory,
    Symlink,
}
