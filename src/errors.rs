use thiserror::Error;

#[derive(Error,Debug)]
pub enum P2pStorageErrors<'a> {
    #[error("Failed to Identify: {0}")]
    FaileToIdentify(&'a str),
}
