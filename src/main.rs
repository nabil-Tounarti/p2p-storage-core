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

// --- 2. The FUSE Filesystem Implementation ---

struct P2pFs {
    cmd_tx: mpsc::Sender<Command>,
    target_peer: PeerId,
}

impl P2pFs {
    fn remote_call(&self, req: FsRequest) -> Option<FsResponse> {
        println!("  [FUSE -> Network] Request: {:?}", req);
        let (tx, rx) = oneshot::channel();
        let cmd = Command {
            peer_id: self.target_peer,
            request: req,
            resp_sender: tx,
        };
        
        if let Err(_) = self.cmd_tx.blocking_send(cmd) { return None; }

        match rx.blocking_recv() {
            Ok(res) => {
                println!("  [Network -> FUSE] Success: {:?}", res);
                Some(res)
            }
            Err(_) => {
                eprintln!("  [FUSE] Network Timeout");
                None
            }
        }
    }
}

impl Filesystem for P2pFs {
    fn lookup(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEntry) {
        let name_str = name.to_string_lossy();
        // Map lookup to a path string for the provider
        let path = if parent == 1 { format!("/{}", name_str) } else { format!("/unknown") };
        
        if let Some(FsResponse::FileAttr(attr)) = self.remote_call(FsRequest::Stat { path }) {
            reply.entry(&Duration::from_secs(1), &map_attr(attr), 0);
        } else {
            reply.error(libc::ENOENT);
        }
    }

    fn getattr(&mut self, _req: &Request, ino: u64, _fh: Option<u64>, reply: ReplyAttr) {
        let path = if ino == 1 { "/".to_string() } else { "/hello.txt".to_string() };
        if let Some(FsResponse::FileAttr(attr)) = self.remote_call(FsRequest::Stat { path }) {
            reply.attr(&Duration::from_secs(1), &map_attr(attr));
        } else {
            reply.error(libc::ENOENT);
        }
    }

    fn readdir(&mut self, _req: &Request, ino: u64, _fh: u64, offset: i64, mut reply: ReplyDirectory) {
        if ino != 1 {
            reply.error(libc::ENOENT);
            return;
        }
        if offset == 0 {
            let _ = reply.add(1, 0, FileType::Directory, ".");
            let _ = reply.add(1, 1, FileType::Directory, "..");
            if let Some(FsResponse::DirEntries(entries)) = self.remote_call(FsRequest::ListDir { path: "/".to_string() }) {
                for (i, entry) in entries.into_iter().enumerate() {
                    let kind = if entry.is_dir { FileType::Directory } else { FileType::RegularFile };
                    if reply.add(2 + i as u64, (i + 2) as i64, kind, entry.name) { break; }
                }
            }
        }
        reply.ok();
    }

    fn read(&mut self, _req: &Request, _ino: u64, _fh: u64, offset: i64, size: u32, _flags: i32, _lock: Option<u64>, reply: ReplyData) {
        if let Some(FsResponse::Data(data)) = self.remote_call(FsRequest::Read { 
            path: "/hello.txt".to_string(), 
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

// --- 4. Main ---

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // AUTO-CLEANUP: Try to unmount old session if it exists
    let _ = std::process::Command::new("fusermount3").args(["-u", "./p2p_test"]).output();

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
        }).expect("Address must contain PeerID (/p2p/...)");

        swarm.dial(remote)?;
        let fs = P2pFs { cmd_tx: cmd_tx.clone(), target_peer: peer_id };
        let mountpoint = "./p2p_test";
        let _ = std::fs::create_dir_all(mountpoint);
        
        println!(">>> Mounting FUSE at {}", mountpoint);
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
                SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                    println!(">>> NETWORK: Connection established with {:?}", peer_id);
                }
                SwarmEvent::Behaviour(P2pCoreBehaviourEvent::FsRpc(request_response::Event::Message { message, .. })) => {
                    match message {
                        request_response::Message::Request { request, channel, .. } => {
                            let resp = match request {
                                FsRequest::Stat { path } => {
                                    if path == "/" {
                                        FsResponse::FileAttr(FsFileAttr {
                                            ino: 1, size: 0, nlink: 2, perm: 0o755, 
                                            kind: FsFileType::Directory, atime: 0, mtime: 0, ctime: 0, uid: 1000, gid: 1000
                                        })
                                    } else if path == "/hello.txt" {
                                        FsResponse::FileAttr(FsFileAttr {
                                            ino: 2, size: 14, nlink: 1, perm: 0o644, 
                                            kind: FsFileType::File, atime: 0, mtime: 0, ctime: 0, uid: 1000, gid: 1000
                                        })
                                    } else {
                                        FsResponse::Error("Not found".to_string())
                                    }
                                },
                                FsRequest::ListDir { .. } => FsResponse::DirEntries(vec![
                                    FsDirEntry { name: "hello.txt".to_string(), is_dir: false, size: 14 }
                                ]),
                                FsRequest::Read { .. } => FsResponse::Data(b"Hello P2P FS!\n".to_vec()),
                            };
                            let _ = swarm.behaviour_mut().fs_rpc.send_response(channel, resp);
                        }
                        request_response::Message::Response { request_id, response, .. } => {
                            if let Some(tx) = pending_requests.remove(&request_id) {
                                let _ = tx.send(response);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
}
