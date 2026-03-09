use crate::audit::CommitEvidence;
use crate::blob_store::BlobStore;
use crate::commitment::default_commitment_policy;
use crate::immutable::{self, WriteMode};
use crate::keymap::KeyMap;
use crate::materialize::materialize;
use crate::persistence::{load_snapshot, save_snapshot, PersistenceError};
use crate::security::{
    default_reduction_contracts, validate_commit_shape, validate_parameters,
    validate_reduction_contracts, verify_post_commit_refinement, ParameterError, ParameterSet,
    ReductionContract, RefinementError, SecurityPolicyError, VcProfile,
};
use crate::sheaf::coherence::{build_sheaf_coherence, verify_sheaf_coherence, LocalSection};
use crate::state::{apply, Delta, State};
use crate::transparency::ct6962::{
    hex_encode, leaf_hash, make_consistency_proof, make_sth, verify_consistency_proof,
    ConsistencyProof, NodeHash, SignedTreeHead,
};
use crate::type_map::TypeMap;
use crate::typed_value::{TypeTag, TypedValue};
use crate::vc::binary_merkle::{DemoBinaryMerkle, Proof as BinaryMerkleProof};
use crate::vc::ipa::{DemoIpa, Proof as IpaProof};
use crate::vc::kzg::{
    load_and_validate_trusted_setup, DemoKzg, Proof as KzgProof, TrustedSetupArtifact,
};
use crate::vc::VC;
use crate::vector_index::VectorIndex;
use crate::witness::{
    default_algorithm_tag, sign_message, verify_quorum_for_algorithm, WitnessConfig, WitnessError,
};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum VcBackend {
    Ipa,
    Kzg,
    BinaryMerkle,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "backend", rename_all = "snake_case")]
pub enum QueryProof {
    Ipa(IpaProof),
    Kzg(KzgProof),
    BinaryMerkle(BinaryMerkleProof),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CommitEntry {
    pub height: u64,
    pub prev_state_root: NodeHash,
    pub state_root: NodeHash,
    pub delta_digest: u64,
    pub cert_digest: u64,
    pub sheaf_coherence_digest: u64,
    #[serde(default)]
    pub vc_backend_id: String,
    pub vc_scheme_id: String,
    pub vc_domain_separator: String,
    pub vc_max_degree: usize,
    pub sth: SignedTreeHead,
    #[serde(default = "default_algorithm_tag")]
    pub witness_signature_algorithm: String,
    pub witness_sigs: Vec<(String, String)>,
    #[serde(default)]
    pub identity_ledger_head_hash: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum CommitError {
    SheafIncoherent,
    WitnessQuorumFailed,
    EmptyWitnessSet,
    WitnessSigningFailed(WitnessError),

    SecurityPolicyInvalid(SecurityPolicyError),
    SecurityRefinementFailed(RefinementError),
    MonotoneViolation,
}

#[derive(Clone, Debug)]
pub struct NucleusDb {
    pub backend: VcBackend,
    pub state: State,
    pub keymap: KeyMap,
    pub entries: Vec<CommitEntry>,
    pub witness_cfg: WitnessConfig,
    pub security_params: ParameterSet,
    pub reduction_contracts: Vec<ReductionContract>,
    pub kzg_trusted_setup: Option<TrustedSetupArtifact>,
    pub(crate) ct_leaves: Vec<NodeHash>,
    pub(crate) current_sth: Option<SignedTreeHead>,
    pub write_mode: WriteMode,
    pub(crate) monotone_seals: Vec<NodeHash>,
    /// Type tags for each key (blob vs direct encoding).
    pub type_map: TypeMap,
    /// Content-addressable store for blob-typed values (Text, Json, Bytes, Vector).
    pub blob_store: BlobStore,
    /// Approximate nearest-neighbor index for Vector-typed keys.
    pub vector_index: VectorIndex,
}

impl NucleusDb {
    fn backend_id(&self) -> &'static str {
        match self.backend {
            VcBackend::Ipa => "ipa",
            VcBackend::Kzg => "kzg",
            VcBackend::BinaryMerkle => "binary_merkle",
        }
    }

    pub fn new(initial: State, backend: VcBackend, witness_cfg: WitnessConfig) -> Self {
        let profile = match &backend {
            VcBackend::Ipa => VcProfile::Ipa,
            VcBackend::Kzg => VcProfile::Kzg,
            VcBackend::BinaryMerkle => VcProfile::BinaryMerkle,
        };
        let mut params = ParameterSet::default();
        params.commitment_policy =
            default_commitment_policy(profile.clone(), params.max_vector_len);
        let reductions = default_reduction_contracts(profile.clone());
        Self::with_security(initial, backend, witness_cfg, params, reductions)
            .expect("default NucleusDB security parameters/setup must be valid")
    }

    pub fn with_security(
        initial: State,
        backend: VcBackend,
        witness_cfg: WitnessConfig,
        security_params: ParameterSet,
        reduction_contracts: Vec<ReductionContract>,
    ) -> Result<Self, SecurityPolicyError> {
        let profile = match &backend {
            VcBackend::Ipa => VcProfile::Ipa,
            VcBackend::Kzg => VcProfile::Kzg,
            VcBackend::BinaryMerkle => VcProfile::BinaryMerkle,
        };
        validate_parameters(&security_params, profile, &witness_cfg)
            .map_err(SecurityPolicyError::Parameter)?;
        validate_reduction_contracts(&reduction_contracts)?;
        let kzg_trusted_setup = Self::load_kzg_setup_if_required(&backend, &security_params)?;
        Ok(Self {
            backend,
            state: initial,
            keymap: KeyMap::new(),
            entries: vec![],
            witness_cfg,
            security_params,
            reduction_contracts,
            kzg_trusted_setup,
            ct_leaves: vec![],
            current_sth: None,
            write_mode: WriteMode::Normal,
            monotone_seals: vec![],
            type_map: TypeMap::new(),
            blob_store: BlobStore::new(),
            vector_index: VectorIndex::new(),
        })
    }

    fn load_kzg_setup_if_required(
        backend: &VcBackend,
        security_params: &ParameterSet,
    ) -> Result<Option<TrustedSetupArtifact>, SecurityPolicyError> {
        if *backend != VcBackend::Kzg || !security_params.require_kzg_trusted_setup {
            return Ok(None);
        }

        let setup_id = security_params.kzg_trusted_setup_id.as_deref().ok_or(
            SecurityPolicyError::Parameter(ParameterError::MissingKzgTrustedSetup),
        )?;
        let setup_path = security_params.kzg_trusted_setup_path.as_deref().ok_or(
            SecurityPolicyError::Parameter(ParameterError::MissingKzgTrustedSetupPath),
        )?;
        let setup_attestation = security_params
            .kzg_trusted_setup_attestation_sha512
            .as_deref()
            .ok_or(SecurityPolicyError::Parameter(
                ParameterError::MissingKzgTrustedSetupAttestation,
            ))?;

        let setup = load_and_validate_trusted_setup(setup_path, setup_id, setup_attestation)
            .map_err(SecurityPolicyError::KzgTrustedSetup)?;
        if security_params.max_vector_len > setup.max_degree {
            return Err(SecurityPolicyError::KzgSetupDegreeInsufficient {
                setup_id: setup.setup_id.clone(),
                setup_max_degree: setup.max_degree,
                required_max_vector_len: security_params.max_vector_len,
            });
        }
        Ok(Some(setup))
    }

    fn vc_profile(&self) -> VcProfile {
        match self.backend {
            VcBackend::Ipa => VcProfile::Ipa,
            VcBackend::Kzg => VcProfile::Kzg,
            VcBackend::BinaryMerkle => VcProfile::BinaryMerkle,
        }
    }

    pub fn current_sth(&self) -> Option<SignedTreeHead> {
        self.current_sth.clone()
    }

    pub fn save_persistent(&self, path: impl AsRef<Path>) -> Result<(), PersistenceError> {
        save_snapshot(path.as_ref(), self)
    }

    pub fn load_persistent(
        path: impl AsRef<Path>,
        witness_cfg: WitnessConfig,
    ) -> Result<Self, PersistenceError> {
        load_snapshot(path.as_ref(), witness_cfg)
    }

    pub fn aether_maintenance_tick(&mut self, now_unix: u64) -> bool {
        let vector_changed = self.vector_index.maintenance_tick(now_unix);
        let blob_changed = self.blob_store.maintenance_tick(now_unix);
        vector_changed || blob_changed
    }

    pub fn soft_reset_aether_memory(&mut self) {
        self.vector_index.soft_reset_governor();
        self.blob_store.soft_reset_governor();
    }

    /// Get the current write mode.
    pub fn write_mode(&self) -> &WriteMode {
        &self.write_mode
    }

    /// Lock the database into append-only mode.  This is a one-way operation:
    /// once enabled, the lock cannot be reverted.  Every subsequent commit
    /// will verify monotone extension and produce a cryptographic seal.
    pub fn set_append_only(&mut self) {
        self.write_mode = WriteMode::AppendOnly;
    }

    /// Get the current monotone seal chain.
    pub fn monotone_seals(&self) -> &[NodeHash] {
        &self.monotone_seals
    }

    fn state_root_for(&self, vec: &[u64]) -> NodeHash {
        match self.backend {
            VcBackend::Ipa => {
                let c = DemoIpa::commit(vec);
                DemoIpa::digest(&c)
            }
            VcBackend::Kzg => {
                let c = match self.kzg_trusted_setup.as_ref() {
                    Some(setup) => DemoKzg::commit_trusted(vec, setup)
                        .expect("trusted setup validated at DB initialization"),
                    None => DemoKzg::commit(vec),
                };
                DemoKzg::digest(&c)
            }
            VcBackend::BinaryMerkle => {
                let c = DemoBinaryMerkle::commit(vec);
                DemoBinaryMerkle::digest(&c)
            }
        }
    }

    fn now_unix_secs() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    fn ct_leaf_payload(
        height: u64,
        prev_state_root: &NodeHash,
        state_root: &NodeHash,
        delta_digest: u64,
        cert_digest: u64,
        sheaf_coherence_digest: u64,
    ) -> String {
        format!(
            "nucleusdb.ct.leaf.v2|height={height}|prev_state_root={}|state_root={}|delta_digest={delta_digest}|cert_digest={cert_digest}|sheaf_coherence_digest={sheaf_coherence_digest}",
            hex_encode(prev_state_root),
            hex_encode(state_root),
        )
    }

    pub fn commit(
        &mut self,
        delta: Delta,
        local_views: &[LocalSection],
    ) -> Result<CommitEntry, CommitError> {
        validate_parameters(&self.security_params, self.vc_profile(), &self.witness_cfg)
            .map_err(SecurityPolicyError::Parameter)
            .map_err(CommitError::SecurityPolicyInvalid)?;
        validate_reduction_contracts(&self.reduction_contracts)
            .map_err(CommitError::SecurityPolicyInvalid)?;

        if self.witness_cfg.witnesses.is_empty() {
            return Err(CommitError::EmptyWitnessSet);
        }

        let next = apply(&self.state, &delta);
        validate_commit_shape(&self.security_params, delta.writes.len(), next.values.len())
            .map_err(CommitError::SecurityRefinementFailed)?;

        // Monotone extension check (AppendOnly mode).
        if self.write_mode == WriteMode::AppendOnly
            && !immutable::verify_monotone_extension(&self.state, &self.keymap, &next, &self.keymap)
        {
            return Err(CommitError::MonotoneViolation);
        }

        let sheaf_pf = build_sheaf_coherence(local_views);
        if !verify_sheaf_coherence(&next, &sheaf_pf) {
            return Err(CommitError::SheafIncoherent);
        }

        let vec = materialize(&next);
        let state_root = self.state_root_for(&vec);
        let prev_root = self
            .entries
            .last()
            .map(|e| e.state_root)
            .unwrap_or_else(|| self.state_root_for(&materialize(&self.state)));

        let height = (self.entries.len() as u64) + 1;
        let delta_digest = (delta.writes.len() as u64).wrapping_mul(13);
        let cert_digest = height.wrapping_mul(31);

        let payload = Self::ct_leaf_payload(
            height,
            &prev_root,
            &state_root,
            delta_digest,
            cert_digest,
            sheaf_pf.digest,
        );
        self.ct_leaves.push(leaf_hash(payload.as_bytes()));
        let sth = make_sth(&self.ct_leaves, Self::now_unix_secs());
        self.current_sth = Some(sth.clone());

        let msg = format!(
            "{}:{}:{}:{}:{}",
            height,
            hex_encode(&prev_root),
            hex_encode(&state_root),
            hex_encode(&sth.root_hash),
            sth.timestamp_unix_secs
        );
        let mut sigs = Vec::new();
        let sig_alg = self.witness_cfg.signing_algorithm;
        for w in self
            .witness_cfg
            .witnesses
            .iter()
            .take(self.witness_cfg.threshold)
        {
            let sig = sign_message(&self.witness_cfg, w, &msg)
                .map_err(CommitError::WitnessSigningFailed)?;
            sigs.push((w.clone(), sig));
        }

        if !verify_quorum_for_algorithm(&self.witness_cfg, sig_alg, &msg, &sigs) {
            return Err(CommitError::WitnessQuorumFailed);
        }

        self.state = next;
        let entry = CommitEntry {
            height,
            prev_state_root: prev_root,
            state_root,
            delta_digest,
            cert_digest,
            sheaf_coherence_digest: sheaf_pf.digest,
            vc_backend_id: self.backend_id().to_string(),
            vc_scheme_id: self.security_params.commitment_policy.scheme_id.clone(),
            vc_domain_separator: self
                .security_params
                .commitment_policy
                .domain_separator
                .clone(),
            vc_max_degree: self.security_params.commitment_policy.max_degree,
            sth,
            witness_signature_algorithm: sig_alg.as_tag().to_string(),
            witness_sigs: sigs,
            identity_ledger_head_hash: crate::halo::identity_ledger::latest_head_hash()
                .ok()
                .flatten(),
        };
        verify_post_commit_refinement(
            height,
            prev_root,
            entry.height,
            entry.prev_state_root,
            entry.sth.tree_size,
            entry.sth.root_hash,
            entry.sth.timestamp_unix_secs,
            &entry.sth.sig,
        )
        .map_err(CommitError::SecurityRefinementFailed)?;
        self.entries.push(entry.clone());

        // Compute and chain monotone seal (AppendOnly mode).
        if self.write_mode == WriteMode::AppendOnly {
            let kv_digest = immutable::key_value_digest(&self.state, &self.keymap);
            let prev_seal = self.monotone_seals.last().copied().unwrap_or_else(|| {
                match crate::halo::identity_ledger::latest_completed_genesis_hash() {
                    Ok(Some(anchor)) => immutable::genesis_seal_with_anchor(&anchor),
                    _ => immutable::genesis_seal(),
                }
            });
            let seal = immutable::next_seal(&prev_seal, &kv_digest);
            self.monotone_seals.push(seal);
        }

        Ok(entry)
    }

    pub fn commit_with_evidence(
        &mut self,
        delta: Delta,
        local_views: &[LocalSection],
    ) -> Result<(CommitEntry, CommitEvidence), CommitError> {
        let entry = self.commit(delta, local_views)?;
        let prev = if self.entries.len() >= 2 {
            Some(&self.entries[self.entries.len() - 2])
        } else {
            None
        };
        let consistency_with_prev = prev.and_then(|p| {
            self.consistency_from(p.sth.tree_size)
                .map(|(_new_sth, proof)| proof)
        });
        let evidence = CommitEvidence::from_entry(prev, &entry, consistency_with_prev.as_ref());
        Ok((entry, evidence))
    }

    pub fn query(&self, idx: usize) -> Option<(u64, QueryProof, NodeHash)> {
        let vec = materialize(&self.state);
        let value = vec.get(idx).copied()?;
        let state_root = self.state_root_for(&vec);
        let proof = match self.backend {
            VcBackend::Ipa => QueryProof::Ipa(DemoIpa::open(&vec, idx)),
            VcBackend::Kzg => {
                let p = match self.kzg_trusted_setup.as_ref() {
                    Some(setup) => DemoKzg::open_trusted(&vec, idx, setup)
                        .expect("trusted setup validated at DB initialization"),
                    None => DemoKzg::open(&vec, idx),
                };
                QueryProof::Kzg(p)
            }
            VcBackend::BinaryMerkle => QueryProof::BinaryMerkle(DemoBinaryMerkle::open(&vec, idx)),
        };
        Some((value, proof, state_root))
    }

    pub fn verify_query(
        &self,
        idx: usize,
        value: u64,
        proof: &QueryProof,
        state_root: NodeHash,
    ) -> bool {
        let vec = materialize(&self.state);
        let expected_root = self.state_root_for(&vec);
        if expected_root != state_root {
            return false;
        }

        match (self.backend.clone(), proof) {
            (VcBackend::Ipa, QueryProof::Ipa(p)) => {
                let c = DemoIpa::commit(&vec);
                DemoIpa::digest(&c) == state_root && DemoIpa::verify(&c, idx, &value, p)
            }
            (VcBackend::Kzg, QueryProof::Kzg(p)) => {
                let c = match self.kzg_trusted_setup.as_ref() {
                    Some(setup) => match DemoKzg::commit_trusted(&vec, setup) {
                        Ok(c) => c,
                        Err(_) => return false,
                    },
                    None => DemoKzg::commit(&vec),
                };
                if DemoKzg::digest(&c) != state_root {
                    return false;
                }
                match self.kzg_trusted_setup.as_ref() {
                    Some(setup) => {
                        DemoKzg::verify_trusted(&c, idx, &value, p, setup).unwrap_or(false)
                    }
                    None => DemoKzg::verify(&c, idx, &value, p),
                }
            }
            (VcBackend::BinaryMerkle, QueryProof::BinaryMerkle(p)) => {
                let c = DemoBinaryMerkle::commit(&vec);
                DemoBinaryMerkle::digest(&c) == state_root
                    && DemoBinaryMerkle::verify(&c, idx, &value, p)
            }
            _ => false,
        }
    }

    pub fn consistency_from(&self, old_size: u64) -> Option<(SignedTreeHead, ConsistencyProof)> {
        let new_sth = self.current_sth()?;
        let proof = make_consistency_proof(&self.ct_leaves, old_size as usize)?;
        Some((new_sth, proof))
    }

    pub fn verify_head_extension(
        &self,
        old: &SignedTreeHead,
        new: &SignedTreeHead,
        proof: &ConsistencyProof,
    ) -> bool {
        verify_consistency_proof(proof, &old.root_hash, &new.root_hash)
    }

    // -------------------------------------------------------------------
    // Typed value helpers
    // -------------------------------------------------------------------

    /// Insert or update a typed value for a key.
    ///
    /// This writes the encoded u64 cell into pending_writes (returned as a
    /// Delta write), stores any blob data, updates the type map and vector
    /// index.  The caller is responsible for committing the delta.
    pub fn put_typed(&mut self, key: &str, value: TypedValue) -> Result<(usize, u64), String> {
        let tag = value.tag();
        let (cell, blob) = value.encode(key);

        // Validate/update vector index first so vector writes fail closed.
        if tag == TypeTag::Vector {
            let dims = match &value {
                TypedValue::Vector(dims) => dims.clone(),
                _ => {
                    return Err(
                        "internal typed-value mismatch: expected vector payload".to_string()
                    );
                }
            };
            self.vector_index.upsert(key, dims)?;
        } else {
            // Non-vector values must not leave stale vector entries behind.
            self.vector_index.remove(key);
        }

        // Store blob if present.
        if let Some(blob_data) = blob {
            self.blob_store.put(key, blob_data);
        } else {
            // Remove any stale blob entry.
            self.blob_store.remove(key);
        }

        self.type_map.set(key, tag);
        let idx = self.keymap.get_or_create(key);
        Ok((idx, cell))
    }

    /// Read a typed value for a key.
    pub fn get_typed(&self, key: &str) -> Option<TypedValue> {
        let idx = self.keymap.get(key)?;
        let cell = self.state.values.get(idx).copied().unwrap_or(0);
        let tag = self.type_map.get(key);
        let blob = self.blob_store.get(key);
        TypedValue::decode(tag, cell, blob).ok()
    }

    /// Engineering-layer variant that records blob liveness before decoding.
    pub fn get_typed_touching(&mut self, key: &str) -> Option<TypedValue> {
        let idx = self.keymap.get(key)?;
        let cell = self.state.values.get(idx).copied().unwrap_or(0);
        let tag = self.type_map.get(key);
        let blob = self.blob_store.get_with_access(key);
        TypedValue::decode(tag, cell, blob).ok()
    }
}
