use crate::errors::P2pStorageErrors;
use crate::errors::P2pStorageErrors::FailedToIdentify;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use keyring::Entry;
use libp2p::{PeerId, identity::Keypair};
use log::info;

const SERVICE_NAME: &str = "p2p-storage";
const USER_NAME: &str = "user";

pub struct Identity {
    pub peer_id: PeerId,
}

impl Identity {
    pub fn get_or_create_local_key() -> Result<Keypair, P2pStorageErrors<'static>> {
        let entry = Entry::new(SERVICE_NAME, USER_NAME)
            .map_err(|_| FailedToIdentify("Cannot acces the system keyring."))?;
        let local_key_protobuf = match entry.get_password() {
            Ok(local_key_base64) => {
                info!("Local ed25519 key found in system keyring.");
                STANDARD
                    .decode(local_key_base64)
                    .map_err(|_| FailedToIdentify("Cannot decode local ed25519 key."))?
            }
            Err(_) => {
                info!("Local ed25519 key not found in system keyring.");
                let local_key_protobuf = Keypair::generate_ed25519()
                    .to_protobuf_encoding()
                    .map_err(|_| FailedToIdentify("Cannot generate ed25519 key."))?;
                let local_key_base64 = STANDARD.encode(local_key_protobuf.clone());
                entry.set_password(&local_key_base64).map_err(|_| {
                    FailedToIdentify("Failed to save ed25519 key in system keyring")
                })?;
                local_key_protobuf
            }
        };
        Keypair::from_protobuf_encoding(&local_key_protobuf)
            .map_err(|_| FailedToIdentify("Failed to decode Ed25519 key from Protobuf."))
    }

    pub fn get_peer_id() -> Result<Self, P2pStorageErrors<'static>> {
        let local_key = Self::get_or_create_local_key()?;
        info!("ed25519 key decoded succefuly.");
        let peer_id = PeerId::from(local_key.public());
        info!("your peer Id is: {peer_id}");
        Ok(Identity { peer_id })
    }
}
