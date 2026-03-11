use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct CapabilityToken {
    #[serde(default)]
    pub token_id_hex: Option<String>,
}
