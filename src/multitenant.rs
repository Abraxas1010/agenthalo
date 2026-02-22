use crate::persistence::{append_wal_event, init_wal, load_wal, truncate_wal, PersistenceError};
use crate::protocol::{CommitEntry, CommitError, NucleusDb, QueryProof, VcBackend};
use crate::security_utils::{ct_eq_32, domain_hash_32};
use crate::sheaf::coherence::LocalSection;
use crate::state::Delta;
use crate::transparency::ct6962::NodeHash;
use crate::witness::{WitnessConfig, WitnessKeyMaterialSource, WitnessSignatureAlgorithm};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

#[derive(Clone, Debug)]
pub struct MultiTenantPolicy {
    pub require_binary_merkle_backend: bool,
    pub require_mldsa_signatures: bool,
    pub deny_insecure_default_witness_seed: bool,
}

impl MultiTenantPolicy {
    pub fn permissive() -> Self {
        Self {
            require_binary_merkle_backend: false,
            require_mldsa_signatures: false,
            deny_insecure_default_witness_seed: false,
        }
    }

    pub fn production() -> Self {
        Self {
            require_binary_merkle_backend: true,
            require_mldsa_signatures: true,
            deny_insecure_default_witness_seed: true,
        }
    }
}

impl Default for MultiTenantPolicy {
    fn default() -> Self {
        Self::permissive()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum TenantRole {
    Reader,
    Writer,
    Admin,
}

impl TenantRole {
    pub fn from_tag(tag: &str) -> Option<Self> {
        match tag.trim().to_ascii_lowercase().as_str() {
            "reader" => Some(Self::Reader),
            "writer" => Some(Self::Writer),
            "admin" => Some(Self::Admin),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub enum MultiTenantError {
    TenantAlreadyExists {
        tenant_id: String,
    },
    TenantNotFound {
        tenant_id: String,
    },
    TenantAuthFailed {
        tenant_id: String,
    },
    TenantPrincipalNotFound {
        tenant_id: String,
        principal_id: String,
    },
    TenantPrincipalAlreadyExists {
        tenant_id: String,
        principal_id: String,
    },
    TenantPermissionDenied {
        tenant_id: String,
        principal_id: String,
        required: TenantRole,
        got: TenantRole,
    },
    TenantPolicyViolation {
        tenant_id: String,
        reason: String,
    },
    TenantDbPoisoned {
        tenant_id: String,
    },
    TenantMapPoisoned,
    QueryIndexMissing {
        tenant_id: String,
        idx: usize,
    },
    Commit(CommitError),
    Persistence(PersistenceError),
}

#[derive(Clone, Debug)]
struct TenantCredential {
    token_hash: [u8; 32],
    role: TenantRole,
}

#[derive(Debug)]
struct TenantSlot {
    principals: RwLock<BTreeMap<String, TenantCredential>>,
    db: RwLock<NucleusDb>,
    wal_path: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TenantSnapshot {
    pub tenant_id: String,
    pub backend: VcBackend,
    pub entries: usize,
    pub state_values: Vec<u64>,
}

#[derive(Clone, Debug)]
pub struct MultiTenantNucleusDb {
    policy: MultiTenantPolicy,
    tenants: Arc<RwLock<BTreeMap<String, Arc<TenantSlot>>>>,
}

impl MultiTenantNucleusDb {
    pub fn new(policy: MultiTenantPolicy) -> Self {
        Self {
            policy,
            tenants: Arc::new(RwLock::new(BTreeMap::new())),
        }
    }

    fn token_hash(token: &str) -> [u8; 32] {
        domain_hash_32(b"nucleusdb.multitenant.auth.v1", token.as_bytes())
    }

    fn validate_tenant_policy(
        policy: &MultiTenantPolicy,
        tenant_id: &str,
        db: &NucleusDb,
    ) -> Result<(), MultiTenantError> {
        if policy.require_binary_merkle_backend && db.backend != VcBackend::BinaryMerkle {
            return Err(MultiTenantError::TenantPolicyViolation {
                tenant_id: tenant_id.to_string(),
                reason: "backend must be binary_merkle in production policy".to_string(),
            });
        }
        if policy.require_mldsa_signatures
            && db.witness_cfg.signing_algorithm != WitnessSignatureAlgorithm::MlDsa65
        {
            return Err(MultiTenantError::TenantPolicyViolation {
                tenant_id: tenant_id.to_string(),
                reason: "witness signing algorithm must be ml_dsa65 in production policy"
                    .to_string(),
            });
        }
        if policy.deny_insecure_default_witness_seed
            && db.witness_cfg.key_material_source == WitnessKeyMaterialSource::InsecureDefaultSeed
        {
            return Err(MultiTenantError::TenantPolicyViolation {
                tenant_id: tenant_id.to_string(),
                reason: "witness config used insecure default development seed".to_string(),
            });
        }
        Ok(())
    }

    pub fn register_tenant(
        &self,
        tenant_id: impl Into<String>,
        auth_token: &str,
        db: NucleusDb,
    ) -> Result<(), MultiTenantError> {
        self.register_tenant_internal(tenant_id, auth_token, db, None, false)
    }

    pub fn register_tenant_with_wal_path(
        &self,
        tenant_id: impl Into<String>,
        auth_token: &str,
        db: NucleusDb,
        wal_path: Option<PathBuf>,
    ) -> Result<(), MultiTenantError> {
        self.register_tenant_internal(tenant_id, auth_token, db, wal_path, true)
    }

    fn register_tenant_internal(
        &self,
        tenant_id: impl Into<String>,
        auth_token: &str,
        db: NucleusDb,
        wal_path: Option<PathBuf>,
        initialize_wal: bool,
    ) -> Result<(), MultiTenantError> {
        let tenant_id = tenant_id.into();
        Self::validate_tenant_policy(&self.policy, &tenant_id, &db)?;
        if initialize_wal {
            if let Some(path) = wal_path.as_ref() {
                init_wal(path, &db).map_err(MultiTenantError::Persistence)?;
            }
        }

        let mut map = self
            .tenants
            .write()
            .map_err(|_| MultiTenantError::TenantMapPoisoned)?;
        if map.contains_key(&tenant_id) {
            return Err(MultiTenantError::TenantAlreadyExists { tenant_id });
        }

        let mut principals = BTreeMap::new();
        principals.insert(
            "admin".to_string(),
            TenantCredential {
                token_hash: Self::token_hash(auth_token),
                role: TenantRole::Admin,
            },
        );

        let slot = TenantSlot {
            principals: RwLock::new(principals),
            db: RwLock::new(db),
            wal_path,
        };
        map.insert(tenant_id, Arc::new(slot));
        Ok(())
    }

    pub fn register_tenant_from_wal(
        &self,
        tenant_id: impl Into<String>,
        auth_token: &str,
        witness_cfg: WitnessConfig,
        wal_path: PathBuf,
    ) -> Result<(), MultiTenantError> {
        let db = load_wal(&wal_path, witness_cfg).map_err(MultiTenantError::Persistence)?;
        self.register_tenant_internal(tenant_id, auth_token, db, Some(wal_path), false)
    }

    pub fn ensure_wal_initialized(
        &self,
        tenant_id: &str,
        auth_token: &str,
    ) -> Result<(), MultiTenantError> {
        let slot =
            self.authorized_tenant_slot_as(tenant_id, "admin", auth_token, TenantRole::Admin)?;
        if let Some(path) = slot.wal_path.as_ref() {
            let db = slot
                .db
                .read()
                .map_err(|_| MultiTenantError::TenantDbPoisoned {
                    tenant_id: tenant_id.to_string(),
                })?;
            init_wal(path, &db).map_err(MultiTenantError::Persistence)?;
        }
        Ok(())
    }

    fn authorized_tenant_slot_as(
        &self,
        tenant_id: &str,
        principal_id: &str,
        auth_token: &str,
        required: TenantRole,
    ) -> Result<Arc<TenantSlot>, MultiTenantError> {
        let map = self
            .tenants
            .read()
            .map_err(|_| MultiTenantError::TenantMapPoisoned)?;
        let slot = map
            .get(tenant_id)
            .cloned()
            .ok_or_else(|| MultiTenantError::TenantNotFound {
                tenant_id: tenant_id.to_string(),
            })?;

        {
            let principals =
                slot.principals
                    .read()
                    .map_err(|_| MultiTenantError::TenantDbPoisoned {
                        tenant_id: tenant_id.to_string(),
                    })?;
            let cred = principals.get(principal_id).ok_or_else(|| {
                MultiTenantError::TenantPrincipalNotFound {
                    tenant_id: tenant_id.to_string(),
                    principal_id: principal_id.to_string(),
                }
            })?;

            let got = Self::token_hash(auth_token);
            if !ct_eq_32(&cred.token_hash, &got) {
                return Err(MultiTenantError::TenantAuthFailed {
                    tenant_id: tenant_id.to_string(),
                });
            }
            if cred.role < required {
                return Err(MultiTenantError::TenantPermissionDenied {
                    tenant_id: tenant_id.to_string(),
                    principal_id: principal_id.to_string(),
                    required,
                    got: cred.role,
                });
            }
        }
        Ok(slot)
    }

    pub fn tenant_ids(&self) -> Result<Vec<String>, MultiTenantError> {
        let map = self
            .tenants
            .read()
            .map_err(|_| MultiTenantError::TenantMapPoisoned)?;
        Ok(map.keys().cloned().collect())
    }

    pub fn commit(
        &self,
        tenant_id: &str,
        auth_token: &str,
        delta: Delta,
        local_views: &[LocalSection],
    ) -> Result<CommitEntry, MultiTenantError> {
        self.commit_as(tenant_id, "admin", auth_token, delta, local_views)
    }

    pub fn commit_as(
        &self,
        tenant_id: &str,
        principal_id: &str,
        auth_token: &str,
        delta: Delta,
        local_views: &[LocalSection],
    ) -> Result<CommitEntry, MultiTenantError> {
        let slot = self.authorized_tenant_slot_as(
            tenant_id,
            principal_id,
            auth_token,
            TenantRole::Writer,
        )?;
        let delta_for_wal = delta.clone();
        let mut db = slot
            .db
            .write()
            .map_err(|_| MultiTenantError::TenantDbPoisoned {
                tenant_id: tenant_id.to_string(),
            })?;
        let entry = db
            .commit(delta, local_views)
            .map_err(MultiTenantError::Commit)?;
        if let Some(path) = slot.wal_path.as_ref() {
            append_wal_event(path, &delta_for_wal, &db, &entry)
                .map_err(MultiTenantError::Persistence)?;
        }
        Ok(entry)
    }

    pub fn query(
        &self,
        tenant_id: &str,
        auth_token: &str,
        idx: usize,
    ) -> Result<(u64, QueryProof, NodeHash), MultiTenantError> {
        self.query_as(tenant_id, "admin", auth_token, idx)
    }

    pub fn query_as(
        &self,
        tenant_id: &str,
        principal_id: &str,
        auth_token: &str,
        idx: usize,
    ) -> Result<(u64, QueryProof, NodeHash), MultiTenantError> {
        let slot = self.authorized_tenant_slot_as(
            tenant_id,
            principal_id,
            auth_token,
            TenantRole::Reader,
        )?;
        let db = slot
            .db
            .read()
            .map_err(|_| MultiTenantError::TenantDbPoisoned {
                tenant_id: tenant_id.to_string(),
            })?;
        db.query(idx)
            .ok_or_else(|| MultiTenantError::QueryIndexMissing {
                tenant_id: tenant_id.to_string(),
                idx,
            })
    }

    pub fn verify_query(
        &self,
        tenant_id: &str,
        auth_token: &str,
        idx: usize,
        value: u64,
        proof: &QueryProof,
        state_root: NodeHash,
    ) -> Result<bool, MultiTenantError> {
        self.verify_query_as(
            tenant_id, "admin", auth_token, idx, value, proof, state_root,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn verify_query_as(
        &self,
        tenant_id: &str,
        principal_id: &str,
        auth_token: &str,
        idx: usize,
        value: u64,
        proof: &QueryProof,
        state_root: NodeHash,
    ) -> Result<bool, MultiTenantError> {
        let slot = self.authorized_tenant_slot_as(
            tenant_id,
            principal_id,
            auth_token,
            TenantRole::Reader,
        )?;
        let db = slot
            .db
            .read()
            .map_err(|_| MultiTenantError::TenantDbPoisoned {
                tenant_id: tenant_id.to_string(),
            })?;
        Ok(db.verify_query(idx, value, proof, state_root))
    }

    pub fn save_tenant_snapshot(
        &self,
        tenant_id: &str,
        auth_token: &str,
        path: impl AsRef<Path>,
    ) -> Result<(), MultiTenantError> {
        self.save_tenant_snapshot_as(tenant_id, "admin", auth_token, path)
    }

    pub fn save_tenant_snapshot_as(
        &self,
        tenant_id: &str,
        principal_id: &str,
        auth_token: &str,
        path: impl AsRef<Path>,
    ) -> Result<(), MultiTenantError> {
        let slot = self.authorized_tenant_slot_as(
            tenant_id,
            principal_id,
            auth_token,
            TenantRole::Reader,
        )?;
        let db = slot
            .db
            .read()
            .map_err(|_| MultiTenantError::TenantDbPoisoned {
                tenant_id: tenant_id.to_string(),
            })?;
        db.save_persistent(path)
            .map_err(MultiTenantError::Persistence)
    }

    pub fn snapshot_tenant(
        &self,
        tenant_id: &str,
        auth_token: &str,
    ) -> Result<TenantSnapshot, MultiTenantError> {
        self.snapshot_tenant_as(tenant_id, "admin", auth_token)
    }

    pub fn snapshot_tenant_as(
        &self,
        tenant_id: &str,
        principal_id: &str,
        auth_token: &str,
    ) -> Result<TenantSnapshot, MultiTenantError> {
        let slot = self.authorized_tenant_slot_as(
            tenant_id,
            principal_id,
            auth_token,
            TenantRole::Reader,
        )?;
        let db = slot
            .db
            .read()
            .map_err(|_| MultiTenantError::TenantDbPoisoned {
                tenant_id: tenant_id.to_string(),
            })?;
        Ok(TenantSnapshot {
            tenant_id: tenant_id.to_string(),
            backend: db.backend.clone(),
            entries: db.entries.len(),
            state_values: db.state.values.clone(),
        })
    }

    pub fn register_principal(
        &self,
        tenant_id: &str,
        admin_principal_id: &str,
        admin_auth_token: &str,
        principal_id: &str,
        principal_auth_token: &str,
        role: TenantRole,
    ) -> Result<(), MultiTenantError> {
        let slot = self.authorized_tenant_slot_as(
            tenant_id,
            admin_principal_id,
            admin_auth_token,
            TenantRole::Admin,
        )?;
        let mut principals =
            slot.principals
                .write()
                .map_err(|_| MultiTenantError::TenantDbPoisoned {
                    tenant_id: tenant_id.to_string(),
                })?;
        if principals.contains_key(principal_id) {
            return Err(MultiTenantError::TenantPrincipalAlreadyExists {
                tenant_id: tenant_id.to_string(),
                principal_id: principal_id.to_string(),
            });
        }
        principals.insert(
            principal_id.to_string(),
            TenantCredential {
                token_hash: Self::token_hash(principal_auth_token),
                role,
            },
        );
        Ok(())
    }

    pub fn checkpoint_tenant(
        &self,
        tenant_id: &str,
        principal_id: &str,
        auth_token: &str,
        snapshot_path: impl AsRef<Path>,
    ) -> Result<(), MultiTenantError> {
        let slot =
            self.authorized_tenant_slot_as(tenant_id, principal_id, auth_token, TenantRole::Admin)?;
        let db = slot
            .db
            .read()
            .map_err(|_| MultiTenantError::TenantDbPoisoned {
                tenant_id: tenant_id.to_string(),
            })?;
        db.save_persistent(snapshot_path)
            .map_err(MultiTenantError::Persistence)?;
        if let Some(path) = slot.wal_path.as_ref() {
            truncate_wal(path, &db).map_err(MultiTenantError::Persistence)?;
        }
        Ok(())
    }
}
