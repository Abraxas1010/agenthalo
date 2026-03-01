use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop};

#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CryptoScope {
    Sign,
    Vault,
    Wallet,
    Identity,
    Genesis,
    Admin,
}

impl CryptoScope {
    pub fn hkdf_info(&self) -> &'static [u8] {
        match self {
            Self::Sign => b"agenthalo.scope.sign.v2",
            Self::Vault => b"agenthalo.scope.vault.v2",
            Self::Wallet => b"agenthalo.scope.wallet.v2",
            Self::Identity => b"agenthalo.scope.identity.v2",
            Self::Genesis => b"agenthalo.scope.genesis.v2",
            Self::Admin => b"agenthalo.scope.admin.v2",
        }
    }

    pub fn default_ttl_secs(&self) -> u64 {
        match self {
            Self::Sign => 300,
            Self::Vault => 300,
            Self::Wallet => 120,
            Self::Identity => 1800,
            Self::Genesis => 30,
            Self::Admin => 60,
        }
    }

    pub fn admin_scopes() -> &'static [CryptoScope] {
        &[
            Self::Sign,
            Self::Vault,
            Self::Wallet,
            Self::Identity,
            Self::Genesis,
        ]
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Sign => "sign",
            Self::Vault => "vault",
            Self::Wallet => "wallet",
            Self::Identity => "identity",
            Self::Genesis => "genesis",
            Self::Admin => "admin",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "sign" => Some(Self::Sign),
            "vault" => Some(Self::Vault),
            "wallet" => Some(Self::Wallet),
            "identity" => Some(Self::Identity),
            "genesis" => Some(Self::Genesis),
            "admin" => Some(Self::Admin),
            _ => None,
        }
    }
}

#[derive(Zeroize, ZeroizeOnDrop)]
pub struct ScopeKey {
    key: [u8; 32],
    #[zeroize(skip)]
    pub scope: CryptoScope,
    #[zeroize(skip)]
    pub created_at: u64,
    #[zeroize(skip)]
    pub expires_at: u64,
}

impl std::fmt::Debug for ScopeKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScopeKey")
            .field("key", &"<redacted>")
            .field("scope", &self.scope)
            .field("created_at", &self.created_at)
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

impl ScopeKey {
    pub fn new(key: [u8; 32], scope: CryptoScope, now: u64) -> Self {
        Self {
            key,
            scope,
            created_at: now,
            expires_at: now + scope.default_ttl_secs(),
        }
    }

    pub fn is_expired(&self, now: u64) -> bool {
        now >= self.expires_at
    }

    pub fn key_bytes(&self) -> &[u8; 32] {
        &self.key
    }

    pub fn touch(&mut self, now: u64) {
        self.expires_at = now + self.scope.default_ttl_secs();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_ttl_expiry_works() {
        let k = ScopeKey::new([0xAA; 32], CryptoScope::Wallet, 10);
        assert!(!k.is_expired(11));
        assert!(k.is_expired(130));
    }

    #[test]
    fn hkdf_infos_are_unique() {
        let infos: std::collections::HashSet<&'static [u8]> = [
            CryptoScope::Sign,
            CryptoScope::Vault,
            CryptoScope::Wallet,
            CryptoScope::Identity,
            CryptoScope::Genesis,
            CryptoScope::Admin,
        ]
        .iter()
        .map(|s| s.hkdf_info())
        .collect();
        assert_eq!(infos.len(), 6);
    }

    #[test]
    fn admin_scope_expansion() {
        let scopes = CryptoScope::admin_scopes();
        assert!(scopes.contains(&CryptoScope::Sign));
        assert!(scopes.contains(&CryptoScope::Vault));
        assert!(scopes.contains(&CryptoScope::Wallet));
        assert!(scopes.contains(&CryptoScope::Identity));
        assert!(scopes.contains(&CryptoScope::Genesis));
    }
}
