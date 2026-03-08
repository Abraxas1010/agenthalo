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

pub fn cab_nonce_store_path() -> PathBuf {
    halo_dir().join("cab_nonces.json")
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

pub fn crypto_header_path() -> PathBuf {
    halo_dir().join("crypto_header.json")
}

pub fn agent_credentials_dir() -> PathBuf {
    halo_dir().join("agent_credentials")
}

pub fn pq_wallet_v2_path() -> PathBuf {
    halo_dir().join("pq_wallet.v2.enc")
}

pub fn vault_v2_path() -> PathBuf {
    halo_dir().join("vault.v2.enc")
}

pub fn identity_v2_path() -> PathBuf {
    halo_dir().join("identity.v2.enc")
}

pub fn profile_v2_path() -> PathBuf {
    halo_dir().join("profile.v2.enc")
}

pub fn genesis_seed_v2_path() -> PathBuf {
    halo_dir().join("genesis_seed.v2.enc")
}

pub fn wdk_seed_v2_path() -> PathBuf {
    halo_dir().join("wdk_seed.v2.enc")
}

pub fn identity_social_ledger_path() -> PathBuf {
    halo_dir().join("identity_social_ledger.jsonl")
}

pub fn capability_store_path() -> PathBuf {
    halo_dir().join("capabilities.json")
}

pub fn access_policy_store_path() -> PathBuf {
    halo_dir().join("access_policies.json")
}

pub fn proof_gate_config_path() -> PathBuf {
    halo_dir().join("proof_gate.json")
}

pub fn p2pclaw_config_path() -> PathBuf {
    halo_dir().join("p2pclaw.json")
}

pub fn proof_certificates_dir() -> PathBuf {
    halo_dir().join("proof_certificates")
}

pub fn nym_config_dir() -> PathBuf {
    halo_dir().join("nym")
}

pub fn nym_state_path() -> PathBuf {
    halo_dir().join("nym_state.json")
}

pub fn genesis_seed_path() -> PathBuf {
    halo_dir().join("genesis_seed.enc")
}

pub fn ensure_halo_dir() -> Result<(), String> {
    let dir = halo_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("create halo dir: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| format!("set halo dir permissions: {e}"))?;
    }
    Ok(())
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

pub fn ensure_agent_credentials_dir() -> Result<(), String> {
    let path = agent_credentials_dir();
    std::fs::create_dir_all(&path)
        .map_err(|e| format!("create agent credentials dir {}: {e}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).map_err(|e| {
            format!(
                "chmod agent credentials dir {} to 0700: {e}",
                path.display()
            )
        })?;
    }
    Ok(())
}

pub fn ensure_proof_certificates_dir() -> Result<(), String> {
    let path = proof_certificates_dir();
    std::fs::create_dir_all(&path)
        .map_err(|e| format!("create proof certificates dir {}: {e}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).map_err(|e| {
            format!(
                "chmod proof certificates dir {} to 0700: {e}",
                path.display()
            )
        })?;
    }
    Ok(())
}

pub fn ensure_circuit_dir() -> Result<(), String> {
    std::fs::create_dir_all(circuit_dir()).map_err(|e| format!("create circuit dir: {e}"))
}
