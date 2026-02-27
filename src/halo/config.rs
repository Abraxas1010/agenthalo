use std::path::PathBuf;

pub fn halo_dir() -> PathBuf {
    if let Ok(p) = std::env::var("AGENTHALO_HOME") {
        return PathBuf::from(p);
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".agenthalo")
}

pub fn db_path() -> PathBuf {
    if let Ok(p) = std::env::var("AGENTHALO_DB_PATH") {
        return PathBuf::from(p);
    }
    halo_dir().join("traces.ndb")
}

pub fn credentials_path() -> PathBuf {
    halo_dir().join("credentials.json")
}

pub fn pricing_path() -> PathBuf {
    halo_dir().join("pricing.json")
}

pub fn addons_path() -> PathBuf {
    halo_dir().join("addons.json")
}

pub fn onchain_config_path() -> PathBuf {
    halo_dir().join("onchain.json")
}

pub fn circuit_dir() -> PathBuf {
    halo_dir().join("circuit")
}

pub fn circuit_pk_path() -> PathBuf {
    circuit_dir().join("pk.bin")
}

pub fn circuit_vk_path() -> PathBuf {
    circuit_dir().join("vk.bin")
}

pub fn circuit_metadata_path() -> PathBuf {
    circuit_dir().join("metadata.json")
}

pub fn attestations_dir() -> PathBuf {
    halo_dir().join("attestations")
}

pub fn audits_dir() -> PathBuf {
    halo_dir().join("audits")
}

pub fn signatures_dir() -> PathBuf {
    halo_dir().join("signatures")
}

pub fn pq_wallet_path() -> PathBuf {
    halo_dir().join("pq_wallet.json")
}

pub fn vault_path() -> PathBuf {
    halo_dir().join("vault.enc")
}

pub fn profile_path() -> PathBuf {
    halo_dir().join("profile.json")
}

pub fn identity_config_path() -> PathBuf {
    halo_dir().join("identity.json")
}

pub fn ensure_halo_dir() -> Result<(), String> {
    std::fs::create_dir_all(halo_dir()).map_err(|e| format!("create halo dir: {e}"))
}

pub fn ensure_attestations_dir() -> Result<(), String> {
    std::fs::create_dir_all(attestations_dir()).map_err(|e| format!("create attestations dir: {e}"))
}

pub fn ensure_audits_dir() -> Result<(), String> {
    std::fs::create_dir_all(audits_dir()).map_err(|e| format!("create audits dir: {e}"))
}

pub fn ensure_signatures_dir() -> Result<(), String> {
    std::fs::create_dir_all(signatures_dir()).map_err(|e| format!("create signatures dir: {e}"))
}

pub fn ensure_circuit_dir() -> Result<(), String> {
    std::fs::create_dir_all(circuit_dir()).map_err(|e| format!("create circuit dir: {e}"))
}
