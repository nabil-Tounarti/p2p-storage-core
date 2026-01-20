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

use futures::StreamExt;
use libp2p::{
    Multiaddr, PeerId, StreamProtocol, identify, identity, request_response,
    swarm::NetworkBehaviour, swarm::SwarmEvent,
};
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FsRequest(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FsResponse(pub String);

#[derive(NetworkBehaviour)]
pub struct P2pCoreBehaviour {
    identify: identify::Behaviour,
    fs_rpc: request_response::cbor::Behaviour<FsRequest, FsResponse>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());
    println!("Local peer id: {:?}", local_peer_id);

    let mut swarm = libp2p::SwarmBuilder::with_existing_identity(local_key)
        .with_tokio()
        .with_tcp(
            libp2p::tcp::Config::default(),
            libp2p::noise::Config::new,
            libp2p::yamux::Config::default,
        )?
        .with_behaviour(|key| {
            Ok(P2pCoreBehaviour {
                identify: identify::Behaviour::new(identify::Config::new(
                    "/proto/1.0.0".into(),
                    key.public(),
                )),
                fs_rpc: request_response::cbor::Behaviour::new(
                    [(
                        // FIX: The order must be (Protocol, Support), not (Support, Protocol)
                        StreamProtocol::new("/fs-rpc/1.0.0"),
                        request_response::ProtocolSupport::Full,
                    )],
                    request_response::Config::default(),
                ),
            })
        })?
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
        .build();

    swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;

    if let Some(addr_str) = std::env::args().nth(1) {
        let remote: Multiaddr = addr_str.parse()?;
        swarm.dial(remote)?;
        println!("Dialed remote address: {:?}", addr_str);
    }

    loop {
        tokio::select! {
            event = swarm.select_next_some() => match event {
                SwarmEvent::NewListenAddr { address, .. } => {
                    println!("Listening on: {:?}", address);
                }

                SwarmEvent::Behaviour(P2pCoreBehaviourEvent::Identify(identify::Event::Received { peer_id, .. })) => {
                    println!("Identified peer: {:?}. Sending request...", peer_id);
                    swarm.behaviour_mut().fs_rpc.send_request(&peer_id, FsRequest("Hello!".to_string()));
                }

                // FIXED LINE: Added .. to ignore connection_id and other metadata
                SwarmEvent::Behaviour(P2pCoreBehaviourEvent::FsRpc(request_response::Event::Message { peer, message, .. })) => {
                    match message {
                        request_response::Message::Request { request, channel, .. } => {
                            println!("Received Request: '{}' from {:?}", request.0, peer);
                            let _ = swarm.behaviour_mut().fs_rpc.send_response(channel, FsResponse("Acknowledged".to_string()));
                        }
                        request_response::Message::Response { response, .. } => {
                            println!("Received Response: '{}' from {:?}", response.0, peer);
                        }
                    }
                }
                _ => {}
            }
        }
    }
}
