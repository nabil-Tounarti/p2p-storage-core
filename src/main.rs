use fuser::{
    FileAttr, FileType, Filesystem, MountOption, ReplyAttr, ReplyData, ReplyDirectory,
    ReplyEntry, Request,
};
use futures::StreamExt;
use libp2p::{
    identify, identity, request_response, swarm::NetworkBehaviour, swarm::SwarmEvent, Multiaddr,
    PeerId, StreamProtocol,
};
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::ffi::OsStr;
use std::time::{Duration, UNIX_EPOCH};
use tokio::sync::{mpsc, oneshot};
use std::path::{Path, PathBuf};
use std::io::{Read, Seek, SeekFrom};

// --- 1. Protocol Definitions ---

#[derive(Debug, Serialize, Deserialize)]
pub enum FsRequest {
    ListDir { path: String },
    Stat { path: String },
    Read { path: String, offset: u64, length: u32 },
}

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
    pub ino: u64,
    pub size: u64,
    pub nlink: u32,
    pub perm: u16,
    pub kind: FsFileType,
    pub atime: i64,
    pub mtime: i64,
    pub ctime: i64,
    pub uid: u32,
    pub gid: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FsFileType { File, Directory, Symlink }

struct Command {
    peer_id: PeerId,
    request: FsRequest,
    resp_sender: oneshot::Sender<FsResponse>,
}

// --- 2. The FUSE Filesystem Implementation (Client Side) ---

struct P2pFs {
    cmd_tx: mpsc::Sender<Command>,
    target_peer: PeerId,
}

impl P2pFs {
    fn remote_call(&self, req: FsRequest) -> Option<FsResponse> {
        let (tx, rx) = oneshot::channel();
        let cmd = Command {
            peer_id: self.target_peer,
            request: req,
            resp_sender: tx,
        };
        if let Err(_) = self.cmd_tx.blocking_send(cmd) { return None; }
        rx.blocking_recv().ok()
    }
}

impl Filesystem for P2pFs {
    fn lookup(&mut self, _req: &Request, _parent: u64, name: &OsStr, reply: ReplyEntry) {
        let name_str = name.to_string_lossy().to_string();
        if let Some(FsResponse::FileAttr(attr)) = self.remote_call(FsRequest::Stat { path: name_str }) {
            reply.entry(&Duration::from_secs(1), &map_attr(attr), 0);
        } else {
            reply.error(libc::ENOENT);
        }
    }

    fn getattr(&mut self, _req: &Request, ino: u64, _fh: Option<u64>, reply: ReplyAttr) {
        let path = if ino == 1 { "/".to_string() } else { format!("file_{}", ino) };
        if let Some(FsResponse::FileAttr(attr)) = self.remote_call(FsRequest::Stat { path }) {
            reply.attr(&Duration::from_secs(1), &map_attr(attr));
        } else {
            reply.error(libc::ENOENT);
        }
    }

    fn readdir(&mut self, _req: &Request, ino: u64, _fh: u64, offset: i64, mut reply: ReplyDirectory) {
        if ino != 1 { reply.error(libc::ENOENT); return; }
        if offset == 0 {
            let _ = reply.add(1, 0, FileType::Directory, ".");
            let _ = reply.add(1, 1, FileType::Directory, "..");
            if let Some(FsResponse::DirEntries(entries)) = self.remote_call(FsRequest::ListDir { path: "/".to_string() }) {
                for (i, entry) in entries.into_iter().enumerate() {
                    let kind = if entry.is_dir { FileType::Directory } else { FileType::RegularFile };
                    // We use the index as a fake inode for simplicity in this demo
                    if reply.add(2 + i as u64, (i + 2) as i64, kind, entry.name) { break; }
                }
            }
        }
        reply.ok();
    }

    fn read(&mut self, _req: &Request, _ino: u64, _fh: u64, offset: i64, size: u32, _flags: i32, _lock: Option<u64>, reply: ReplyData) {
        // In this simple demo, we map any read back to the provider logic
        // For a real app, you'd track which inode maps to which filename
        if let Some(FsResponse::Data(data)) = self.remote_call(FsRequest::Read { 
            path: "READ_REPLY".to_string(), // In real app, send actual filename
            offset: offset as u64, 
            length: size 
        }) {
            reply.data(&data);
        } else {
            reply.error(libc::EIO);
        }
    }
}

fn map_attr(a: FsFileAttr) -> FileAttr {
    FileAttr {
        ino: a.ino, size: a.size, blocks: 1,
        atime: UNIX_EPOCH + Duration::from_secs(a.atime as u64),
        mtime: UNIX_EPOCH + Duration::from_secs(a.mtime as u64),
        ctime: UNIX_EPOCH + Duration::from_secs(a.ctime as u64),
        crtime: UNIX_EPOCH + Duration::from_secs(a.ctime as u64),
        kind: match a.kind { FsFileType::Directory => FileType::Directory, _ => FileType::RegularFile },
        perm: a.perm, nlink: a.nlink, uid: a.uid, gid: a.gid, rdev: 0, blksize: 512, flags: 0,
    }
}

// --- 3. Network Behaviour ---

#[derive(NetworkBehaviour)]
pub struct P2pCoreBehaviour {
    identify: identify::Behaviour,
    fs_rpc: request_response::cbor::Behaviour<FsRequest, FsResponse>,
}

// --- 4. Main Loop ---

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let _ = std::process::Command::new("fusermount3").args(["-u", "./p2p_test"]).output();
    let shared_dir = PathBuf::from("./shared");
    if !shared_dir.exists() { std::fs::create_dir(&shared_dir)?; }

    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());
    let (cmd_tx, mut cmd_rx) = mpsc::channel::<Command>(32);
    let mut pending_requests = std::collections::HashMap::new();

    let mut swarm = libp2p::SwarmBuilder::with_existing_identity(local_key)
        .with_tokio()
        .with_tcp(Default::default(), libp2p::noise::Config::new, libp2p::yamux::Config::default)?
        .with_behaviour(|key| {
            Ok(P2pCoreBehaviour {
                identify: identify::Behaviour::new(identify::Config::new("/proto/1.0.0".into(), key.public())),
                fs_rpc: request_response::cbor::Behaviour::new(
                    [(StreamProtocol::new("/fs-rpc/1.0.0"), request_response::ProtocolSupport::Full)],
                    request_response::Config::default(),
                ),
            })
        })?
        .build();

    swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;

    if let Some(addr_str) = std::env::args().nth(1) {
        let remote: Multiaddr = addr_str.parse()?;
        let peer_id = remote.iter().find_map(|p| {
            if let libp2p::multiaddr::Protocol::P2p(peer_id) = p { Some(peer_id) } else { None }
        }).expect("Address must contain PeerID");

        swarm.dial(remote)?;
        let fs = P2pFs { cmd_tx: cmd_tx.clone(), target_peer: peer_id };
        let mountpoint = "./p2p_test";
        let _ = std::fs::create_dir_all(mountpoint);
        println!(">>> Mounting at {}", mountpoint);
        std::thread::spawn(move || {
            let _ = fuser::mount2(fs, mountpoint, &[MountOption::AutoUnmount, MountOption::AllowOther]);
        });
    }

    loop {
        tokio::select! {
            Some(cmd) = cmd_rx.recv() => {
                let rid = swarm.behaviour_mut().fs_rpc.send_request(&cmd.peer_id, cmd.request);
                pending_requests.insert(rid, cmd.resp_sender);
            }
            event = swarm.select_next_some() => match event {
                SwarmEvent::NewListenAddr { address, .. } => {
                    println!(">>> Node reachable at: {}/p2p/{}", address, local_peer_id);
                }
                SwarmEvent::Behaviour(P2pCoreBehaviourEvent::FsRpc(request_response::Event::Message { message, .. })) => {
                    match message {
                        request_response::Message::Request { request, channel, .. } => {
                            // --- PROVIDER LOGIC: RESTRICTED TO ./shared ---
                            let resp = match request {
                                FsRequest::ListDir { .. } => {
                                    let mut entries = Vec::new();
                                    if let Ok(dir) = std::fs::read_dir("./shared") {
                                        for entry in dir.flatten() {
                                            if let Ok(meta) = entry.metadata() {
                                                entries.push(FsDirEntry {
                                                    name: entry.file_name().to_string_lossy().into_owned(),
                                                    is_dir: meta.is_dir(),
                                                    size: meta.len(),
                                                });
                                            }
                                        }
                                    }
                                    FsResponse::DirEntries(entries)
                                }
                                FsRequest::Stat { path } => {
                                    if path == "/" {
                                        FsResponse::FileAttr(FsFileAttr {
                                            ino: 1, size: 0, nlink: 2, perm: 0o755, kind: FsFileType::Directory,
                                            atime: 0, mtime: 0, ctime: 0, uid: 1000, gid: 1000
                                        })
                                    } else {
                                        // Try to find the file inside ./shared
                                        let full_path = Path::new("./shared").join(path.trim_start_matches('/'));
                                        if let Ok(meta) = std::fs::metadata(&full_path) {
                                            FsResponse::FileAttr(FsFileAttr {
                                                ino: 2, size: meta.len(), nlink: 1, perm: 0o644,
                                                kind: if meta.is_dir() { FsFileType::Directory } else { FsFileType::File },
                                                atime: 0, mtime: 0, ctime: 0, uid: 1000, gid: 1000
                                            })
                                        } else { FsResponse::Error("Not found".into()) }
                                    }
                                }
                                FsRequest::Read { offset, length, .. } => {
                                    // Simplified: Read the first file found in shared for the demo
                                    if let Ok(mut dir) = std::fs::read_dir("./shared") {
                                        if let Some(Ok(entry)) = dir.next() {
                                            if let Ok(mut file) = std::fs::File::open(entry.path()) {
                                                let mut buf = vec![0; length as usize];
                                                let _ = file.seek(SeekFrom::Start(offset));
                                                let n = file.read(&mut buf).unwrap_or(0);
                                                buf.truncate(n);
                                                FsResponse::Data(buf)
                                            } else { FsResponse::Error("Read fail".into()) }
                                        } else { FsResponse::Error("No files".into()) }
                                    } else { FsResponse::Error("Dir fail".into()) }
                                }
                            };
                            let _ = swarm.behaviour_mut().fs_rpc.send_response(channel, resp);
                        }
                        request_response::Message::Response { request_id, response, .. } => {
                            if let Some(tx) = pending_requests.remove(&request_id) { let _ = tx.send(response); }
                        }
                    }
                }
                _ => {}
            }
        }
    }
}
