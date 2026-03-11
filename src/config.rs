use std::path::PathBuf;

pub fn nucleusdb_dir() -> PathBuf {
    if let Ok(p) = std::env::var("NUCLEUSDB_HOME") {
        return PathBuf::from(p);
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".nucleusdb")
}

pub fn db_path() -> PathBuf {
    std::env::var("NUCLEUSDB_DB_PATH")
        .ok()
        .map(PathBuf::from)
        .unwrap_or_else(|| nucleusdb_dir().join("nucleusdb.ndb"))
}

pub fn credentials_path() -> PathBuf {
    nucleusdb_dir().join("credentials.json")
}
pub fn crypto_header_path() -> PathBuf {
    nucleusdb_dir().join("crypto_header.json")
}
pub fn genesis_seed_v2_path() -> PathBuf {
    nucleusdb_dir().join("genesis_seed.v2.enc")
}
pub fn identity_config_path() -> PathBuf {
    nucleusdb_dir().join("identity.json")
}
pub fn identity_v2_path() -> PathBuf {
    nucleusdb_dir().join("identity.v2.enc")
}
pub fn vault_v2_path() -> PathBuf {
    nucleusdb_dir().join("vault.v2.enc")
}
pub fn proof_gate_config_path() -> PathBuf {
    nucleusdb_dir().join("proof_gate.json")
}
pub fn proof_certificates_dir() -> PathBuf {
    nucleusdb_dir().join("proof_certificates")
}
pub fn discord_status_path() -> PathBuf {
    nucleusdb_dir().join("discord_status.json")
}
pub fn discord_export_dir() -> PathBuf {
    nucleusdb_dir().join("exports")
}
pub fn cab_nonce_store_path() -> PathBuf {
    nucleusdb_dir().join("cab_nonces.json")
}
pub fn discord_db_path() -> PathBuf {
    std::env::var("NUCLEUSDB_DISCORD_DB_PATH")
        .ok()
        .map(PathBuf::from)
        .or_else(|| std::env::var("NUCLEUSDB_DB_PATH").ok().map(PathBuf::from))
        .unwrap_or_else(|| db_path())
}

pub fn ensure_nucleusdb_dir() -> Result<(), String> {
    let dir = nucleusdb_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("create nucleusdb dir: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| format!("set nucleusdb dir permissions: {e}"))?;
    }
    Ok(())
}

pub fn ensure_proof_certificates_dir() -> Result<(), String> {
    let path = proof_certificates_dir();
    std::fs::create_dir_all(&path)
        .map_err(|e| format!("create proof certificates dir {}: {e}", path.display()))?;
    Ok(())
}

pub fn wallet_seed_path() -> PathBuf {
    nucleusdb_dir().join("pq_wallet.json")
}
pub fn legacy_genesis_seed_path() -> PathBuf {
    nucleusdb_dir().join("genesis_seed.enc")
}
