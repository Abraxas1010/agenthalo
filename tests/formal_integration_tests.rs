use nucleusdb::protocol;
use nucleusdb::security;
use nucleusdb::sheaf::coherence;
use nucleusdb::transparency::ct6962;
use nucleusdb::vc::ipa;
use nucleusdb::verifier::gate::load_gate_config;

fn total_provenance_entries() -> usize {
    security::formal_provenance().len()
        + ct6962::formal_provenance().len()
        + ipa::formal_provenance().len()
        + coherence::formal_provenance().len()
        + protocol::formal_provenance().len()
}

fn assert_provenance_shape(entries: &[security::FormalProvenance]) {
    assert!(!entries.is_empty());
    for (name, heyting_path, local_path) in entries {
        assert!(!name.is_empty());
        assert!(heyting_path.starts_with("HeytingLean."));
        if let Some(local) = local_path {
            assert!(local.starts_with("NucleusDB.") || local.starts_with("HeytingLean.NucleusDB."));
        }
    }
}

#[test]
fn security_formal_provenance_is_complete() {
    let provenance = security::formal_provenance();
    assert_provenance_shape(&provenance);
    let theorem_names: Vec<_> = provenance.iter().map(|(n, _, _)| *n).collect();
    assert!(theorem_names.contains(&"nucleus_combine_floor_bound"));
    assert!(theorem_names.contains(&"vUpdate_chain_comm"));
    assert!(theorem_names.contains(&"validFor_of_bounds"));
    assert!(theorem_names.contains(&"singleton_bundle_valid"));
    assert!(theorem_names.contains(&"certificate_to_refinement"));
}

#[test]
fn ct6962_formal_provenance_is_complete() {
    let provenance = ct6962::formal_provenance();
    assert_provenance_shape(&provenance);
    let theorem_names: Vec<_> = provenance.iter().map(|(n, _, _)| *n).collect();
    assert!(theorem_names.contains(&"consistency_implies_prefix"));
    assert!(theorem_names.contains(&"leafChainRoot_injective"));
}

#[test]
fn ipa_commitment_has_correctness_soundness_and_hiding_basis() {
    let provenance = ipa::formal_provenance();
    assert_provenance_shape(&provenance);
    let names: Vec<_> = provenance.iter().map(|(n, _, _)| *n).collect();
    assert!(names.contains(&"openCorrect"));
    assert!(names.contains(&"openSound_of_binding"));
    assert!(names.contains(&"computationalHiding_of_dlogReduction"));
}

#[test]
fn sheaf_coherence_has_trace_topology_basis() {
    let provenance = coherence::formal_provenance();
    assert_provenance_shape(&provenance);
    let names: Vec<_> = provenance.iter().map(|(n, _, _)| *n).collect();
    assert!(names.contains(&"refines_preserves_connected"));
    assert!(names.contains(&"componentConstant_iff_exists_lift"));
}

#[test]
fn protocol_layer_has_certificate_and_refinement_basis() {
    let provenance = protocol::formal_provenance();
    assert_provenance_shape(&provenance);
    let names: Vec<_> = provenance.iter().map(|(n, _, _)| *n).collect();
    assert!(names.contains(&"nucleus_combine_floor_bound"));
    assert!(names.contains(&"vUpdate_chain_comm"));
    assert!(names.contains(&"step_eq_apply"));
    assert!(names.contains(&"verifyCommitCertificate_sound"));
}

#[test]
fn provenance_surface_exposes_fifteen_plus_theorems() {
    assert!(total_provenance_entries() >= 15);
}

#[test]
fn proof_gate_config_loads_from_repo() {
    let config = load_gate_config().expect("gate config should load");
    assert!(!config.enabled);
    assert!(config.requirements.len() >= 5);
}

#[test]
fn proof_gate_requirements_reference_valid_theorem_shapes() {
    let config = load_gate_config().expect("gate config should load");
    let mut requirement_count = 0usize;
    for (tool, reqs) in &config.requirements {
        assert!(!tool.is_empty());
        for req in reqs {
            requirement_count += 1;
            assert_eq!(req.tool_name, *tool);
            assert!(req.required_theorem.starts_with("HeytingLean."));
            assert!(req.required_theorem.contains('.'));
            assert!(!req.description.is_empty());
        }
    }
    assert!(requirement_count >= 10);
}

#[test]
fn proof_gate_tools_cover_commit_kem_trace_and_dashboard_surfaces() {
    let config = load_gate_config().expect("gate config should load");
    assert!(config.requirements.contains_key("nucleusdb_commit"));
    assert!(config
        .requirements
        .contains_key("nucleusdb_kem_encapsulate"));
    assert!(config.requirements.contains_key("nucleusdb_trace_analysis"));
    assert!(config.requirements.contains_key("nucleusdb_execute_sql"));
}

#[test]
fn advisory_gate_config_has_no_enforced_requirements_yet() {
    let config = load_gate_config().expect("gate config should load");
    assert!(config
        .requirements
        .values()
        .flat_map(|reqs| reqs.iter())
        .all(|req| !req.enforced));
}
