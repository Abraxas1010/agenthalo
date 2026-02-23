use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum ChannelStatus {
    Open,
    Closing,
    Closed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ChannelRecord {
    pub participant1: String,
    pub participant2: String,
    pub capacity: u64,
    pub status: ChannelStatus,
    pub created_at: u64,
    pub last_seq: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum SettlementOp {
    Open {
        p1: String,
        p2: String,
        capacity: u64,
    },
    Close {
        p1: String,
        p2: String,
    },
    Splice {
        p1: String,
        p2: String,
        new_capacity: u64,
    },
    Update {
        p1: String,
        p2: String,
        balance1: u64,
        balance2: u64,
    },
}
