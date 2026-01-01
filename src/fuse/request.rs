use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub enum FsRequest {
    ListDir {
        path: String,
    },
    Stat {
        path: String,
    },
    Read {
        path: String,
        offset: u64,
        length: u32,
    },
}
