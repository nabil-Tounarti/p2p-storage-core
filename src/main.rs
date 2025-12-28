/*
use fuser::{Filesystem, Request, ReplyAttr, ReplyData, ReplyDirectory, ReplyEntry, FileAttr, FileType, MountOption};
use libc::ENOENT;
use std::ffi::OsStr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::path::Path;
use std::collections::HashMap;

const TTL: Duration = Duration::from_secs(1);               // 1 second
const HELLO_CONTENT: &str = "Hello from Rust FUSE!\n";

fn now() -> SystemTime {
    SystemTime::now()
}

fn ts() -> i64 {
    now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64
}

fn file_attr(ino: u64, size: u64, kind: FileType) -> FileAttr {
    FileAttr {
        ino,
        size,
        blocks: 1,
        atime: now(),
        mtime: now(),
        ctime: now(),
        crtime: now(),
        kind,
        perm: if kind == FileType::Directory { 0o755 } else { 0o644 },
        nlink: 1,
        uid: 0,
        gid: 0,
        rdev: 0,
        blksize: 512,   // <-- REQUIRED
        flags: 0,
    }

}

struct HelloFS {
    // no state for this example; real FS may store maps etc.
    inodes: HashMap<u64, String>,
}

impl HelloFS {
    fn new() -> Self {
        let mut inodes = HashMap::new();
        inodes.insert(2, String::from("hello.txt")); // inode 2 -> hello.txt
        Self { inodes }
    }
}

impl Filesystem for HelloFS {
    fn lookup(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEntry) {
        // root inode is 1
        if parent == 1 && name == OsStr::new("hello.txt") {
            let attr = file_attr(2, HELLO_CONTENT.len() as u64, FileType::RegularFile);
            reply.entry(&TTL, &attr, 0);
        } else {
            reply.error(ENOENT);
        }
    }

    fn getattr(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        _fh: Option<u64>,
        reply: ReplyAttr
    ) {
        if ino == 1 {
            let attr = file_attr(1, 0, FileType::Directory);
            reply.attr(&TTL, &attr);
        } else if ino == 2 {
            let attr = file_attr(2, HELLO_CONTENT.len() as u64, FileType::RegularFile);
            reply.attr(&TTL, &attr);
        } else {
            reply.error(ENOENT);
        }
    }


    fn readdir(&mut self, _req: &Request<'_>, ino: u64, _fh: u64, offset: i64, mut reply: ReplyDirectory) {
        if ino != 1 {
            reply.error(ENOENT);
            return;
        }

        // offset 0 -> entries: ., .., hello.txt
        if offset == 0 {
            reply.add(1, 1, FileType::Directory, ".");
            reply.add(1, 2, FileType::Directory, "..");
            reply.add(2, 3, FileType::RegularFile, "hello.txt");
        }
        reply.ok();
    }

    fn read(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        _fh: u64,
        offset: i64,
        size: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: ReplyData,
    ) {
        if ino != 2 {
            reply.error(libc::ENOENT);
            return;
        }

        let data = HELLO_CONTENT.as_bytes();
        let start = offset as usize;

        if start >= data.len() {
            reply.data(&[]);
            return;
        }

        let end = std::cmp::min(start + size as usize, data.len());
        reply.data(&data[start..end]);
    }


    // other methods can be left unimplemented for this simple test
}
*/

use p2p_core::identity;
fn main() {
    let identity = identity::Identity::get_or_create_peer_id().unwrap();

    println!("you peer Id is: {0}", identity.peer_id);
}
