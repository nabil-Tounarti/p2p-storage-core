use libp2p::swarm::NetworkBehaviour;
use libp2p::{identify, request_response};

use crate::fuse::{request::FsRequest, response::FsResponse};

#[derive(NetworkBehaviour)]
pub struct P2pCoreBehaviour {
    identify: identify::Behaviour,
    fs_rpc: request_response::cbor::Behaviour<FsRequest, FsResponse>,
}
