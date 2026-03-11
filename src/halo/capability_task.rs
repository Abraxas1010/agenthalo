use crate::halo::capability_spec::{CapabilityQuery, CapabilitySpec};
use crate::halo::p2p_discovery::AgentDiscovery;
use crate::halo::util::{digest_bytes, hex_encode};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

const TASK_GROUP_DOMAIN: &str = "agenthalo.capability.task_group.v1";

#[derive(Serialize)]
struct FormationCanonical<'a> {
    task_id: &'a str,
    assignments: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CapabilitySlot {
    pub slot_id: String,
    pub query: CapabilityQuery,
    pub redundancy: u32,
    #[serde(default)]
    pub optional: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskEdge {
    pub from_slot: String,
    pub to_slot: String,
    pub output_index: u32,
    pub input_index: u32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ManifoldConstraints {
    pub max_total_latency_ms: Option<u64>,
    pub max_total_cost_microdollars: Option<u64>,
    pub min_distinct_agents: Option<u32>,
    pub require_mutual_attestation: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TaskManifold {
    pub task_id: String,
    pub description: String,
    pub slots: Vec<CapabilitySlot>,
    pub edges: Vec<TaskEdge>,
    pub constraints: ManifoldConstraints,
    pub originator_did: String,
    pub created_at: u64,
    pub formation_timeout_ms: u64,
    pub expires_at: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TaskBid {
    pub task_id: String,
    pub slot_id: String,
    pub bidder_did: String,
    pub capability_spec: CapabilitySpec,
    pub estimated_latency_ms: u64,
    pub cost_microdollars: u64,
    pub bid_expiry: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TaskAssignment {
    pub task_id: String,
    pub slot_id: String,
    pub assigned_did: String,
    pub capability_id: String,
    pub group_topic: String,
    pub estimated_latency_ms: u64,
    pub cost_microdollars: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TaskFormation {
    pub group_id: String,
    pub task_id: String,
    pub assignments: Vec<TaskAssignment>,
    pub formed_at: u64,
    pub expires_at: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EphemeralTaskGroup {
    pub formation: TaskFormation,
    pub dissolved_at: Option<u64>,
}

fn group_topic(task_id: &str) -> String {
    format!("/agenthalo/tasks/{task_id}/group")
}

fn mutual_attestation_satisfied(
    assignments: &[TaskAssignment],
    specs: &HashMap<String, CapabilitySpec>,
    now: u64,
    attestation_max_age_secs: u64,
) -> bool {
    for left in assignments {
        for right in assignments {
            if left.assigned_did == right.assigned_did {
                continue;
            }
            let Some(left_spec) = specs.get(&left.capability_id) else {
                return false;
            };
            let Some(right_spec) = specs.get(&right.capability_id) else {
                return false;
            };
            let left_trusts_right = left_spec.attestations.iter().any(|attestation| {
                attestation.passed
                    && attestation.attester_did != attestation.subject_did
                    && attestation.subject_did == right.assigned_did
                    && attestation.attester_did == left.assigned_did
                    && now.saturating_sub(attestation.verified_at) <= attestation_max_age_secs
            });
            let right_trusts_left = right_spec.attestations.iter().any(|attestation| {
                attestation.passed
                    && attestation.attester_did != attestation.subject_did
                    && attestation.subject_did == left.assigned_did
                    && attestation.attester_did == right.assigned_did
                    && now.saturating_sub(attestation.verified_at) <= attestation_max_age_secs
            });
            if !(left_trusts_right && right_trusts_left) {
                return false;
            }
        }
    }
    true
}

impl TaskManifold {
    pub fn assemble_atomic(
        &self,
        discovery: &AgentDiscovery,
        now: u64,
        attestation_max_age_secs: u64,
    ) -> Result<TaskFormation, String> {
        if now > self.expires_at {
            return Err(format!("task manifold `{}` expired", self.task_id));
        }

        let group_topic = group_topic(&self.task_id);
        let mut assignments = Vec::new();
        let mut selected_specs = HashMap::new();

        for slot in &self.slots {
            let mut matches = discovery.find_by_query(&slot.query, now, attestation_max_age_secs);
            matches.sort_by(|left, right| {
                let left_spec = discovery.best_capability_match(
                    left,
                    &slot.query,
                    now,
                    attestation_max_age_secs,
                );
                let right_spec = discovery.best_capability_match(
                    right,
                    &slot.query,
                    now,
                    attestation_max_age_secs,
                );
                let left_attestations = left_spec
                    .map(|spec| spec.verified_attestation_count(now, attestation_max_age_secs))
                    .unwrap_or(0);
                let right_attestations = right_spec
                    .map(|spec| spec.verified_attestation_count(now, attestation_max_age_secs))
                    .unwrap_or(0);
                let left_success = left_spec
                    .map(|spec| {
                        crate::halo::capability_spec::normalized_success_rate(
                            spec.metrics.success_rate,
                        )
                    })
                    .unwrap_or(0.0);
                let right_success = right_spec
                    .map(|spec| {
                        crate::halo::capability_spec::normalized_success_rate(
                            spec.metrics.success_rate,
                        )
                    })
                    .unwrap_or(0.0);
                let left_latency = left_spec
                    .map(|spec| spec.metrics.latency_p99_ms)
                    .unwrap_or(u64::MAX);
                let right_latency = right_spec
                    .map(|spec| spec.metrics.latency_p99_ms)
                    .unwrap_or(u64::MAX);
                let left_cost = left_spec
                    .map(|spec| spec.metrics.cost_microdollars)
                    .unwrap_or(u64::MAX);
                let right_cost = right_spec
                    .map(|spec| spec.metrics.cost_microdollars)
                    .unwrap_or(u64::MAX);
                right_attestations
                    .cmp(&left_attestations)
                    .then_with(|| right_success.total_cmp(&left_success))
                    .then_with(|| left_latency.cmp(&right_latency))
                    .then_with(|| left_cost.cmp(&right_cost))
                    .then_with(|| left.did.cmp(&right.did))
            });

            let needed = slot.redundancy.max(1) as usize;
            if matches.len() < needed && !slot.optional {
                return Err(format!(
                    "task manifold `{}` missing providers for slot `{}`",
                    self.task_id, slot.slot_id
                ));
            }

            for announcement in matches.into_iter().take(needed) {
                let Some(spec) = discovery.best_capability_match(
                    &announcement,
                    &slot.query,
                    now,
                    attestation_max_age_secs,
                ) else {
                    continue;
                };
                let spec = spec.clone();
                selected_specs.insert(spec.capability_id.clone(), spec.clone());
                assignments.push(TaskAssignment {
                    task_id: self.task_id.clone(),
                    slot_id: slot.slot_id.clone(),
                    assigned_did: announcement.did.clone(),
                    capability_id: spec.capability_id.clone(),
                    group_topic: group_topic.clone(),
                    estimated_latency_ms: spec.metrics.latency_p99_ms,
                    cost_microdollars: spec.metrics.cost_microdollars,
                });
            }
        }

        if assignments.is_empty() {
            return Err(format!(
                "task manifold `{}` did not assemble any assignments",
                self.task_id
            ));
        }

        let distinct_agents = assignments
            .iter()
            .map(|assignment| assignment.assigned_did.as_str())
            .collect::<HashSet<_>>()
            .len() as u32;
        let total_latency_ms = assignments
            .iter()
            .map(|assignment| assignment.estimated_latency_ms)
            .sum::<u64>();
        let total_cost_microdollars = assignments
            .iter()
            .map(|assignment| assignment.cost_microdollars)
            .sum::<u64>();

        if let Some(max_total_latency_ms) = self.constraints.max_total_latency_ms {
            if total_latency_ms > max_total_latency_ms {
                return Err(format!(
                    "task manifold `{}` exceeded max_total_latency_ms",
                    self.task_id
                ));
            }
        }
        if let Some(max_total_cost_microdollars) = self.constraints.max_total_cost_microdollars {
            if total_cost_microdollars > max_total_cost_microdollars {
                return Err(format!(
                    "task manifold `{}` exceeded max_total_cost_microdollars",
                    self.task_id
                ));
            }
        }
        if let Some(min_distinct_agents) = self.constraints.min_distinct_agents {
            if distinct_agents < min_distinct_agents {
                return Err(format!(
                    "task manifold `{}` did not satisfy min_distinct_agents",
                    self.task_id
                ));
            }
        }
        if self.constraints.require_mutual_attestation
            && !mutual_attestation_satisfied(
                &assignments,
                &selected_specs,
                now,
                attestation_max_age_secs,
            )
        {
            return Err(format!(
                "task manifold `{}` requires mutual attestation between assigned agents",
                self.task_id
            ));
        }

        let mut canonical_assignments = assignments
            .iter()
            .map(|assignment| {
                format!(
                    "{}:{}:{}",
                    assignment.slot_id, assignment.assigned_did, assignment.capability_id
                )
            })
            .collect::<Vec<_>>();
        canonical_assignments.sort();
        let raw = serde_json::to_vec(&FormationCanonical {
            task_id: &self.task_id,
            assignments: canonical_assignments,
        })
        .unwrap_or_default();
        let group_id = hex_encode(&digest_bytes(TASK_GROUP_DOMAIN, &raw));

        Ok(TaskFormation {
            group_id,
            task_id: self.task_id.clone(),
            assignments,
            formed_at: now,
            expires_at: self.expires_at,
        })
    }
}

impl EphemeralTaskGroup {
    pub fn from_formation(formation: TaskFormation) -> Self {
        Self {
            formation,
            dissolved_at: None,
        }
    }

    pub fn dissolve(&mut self, now: u64) {
        self.dissolved_at = Some(now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::halo::capability_spec::{
        CapabilityAttestation, CapabilityDomain, CapabilityQuery, LiveMetrics, TypeSpec,
    };
    use crate::halo::p2p_discovery::AgentAnnouncement;

    fn spec(
        domain: &str,
        did: &str,
        success_rate: f64,
        latency_ms: u64,
        cost_microdollars: u64,
    ) -> CapabilitySpec {
        let mut spec = CapabilitySpec::new(
            CapabilityDomain::new(domain, 1),
            vec![TypeSpec::LeanTerm],
            vec![TypeSpec::LeanTerm],
            vec![],
        );
        spec.metrics = LiveMetrics {
            tasks_completed: 10,
            tasks_failed: 1,
            success_rate,
            latency_p50_ms: latency_ms,
            latency_p99_ms: latency_ms * 2,
            cost_microdollars,
            last_active: 100,
            onchain_reputation: Some(success_rate * 100.0),
        };
        spec.attestations.push(CapabilityAttestation {
            attester_did: format!("{did}:attester"),
            subject_did: did.to_string(),
            capability_id: spec.capability_id.clone(),
            challenge_hash: "h".to_string(),
            passed: true,
            verified_at: 100,
            ed25519_signature: vec![1],
            mldsa65_signature: vec![2],
        });
        spec
    }

    fn announcement(did: &str, spec: CapabilitySpec) -> AgentAnnouncement {
        let now = crate::halo::util::now_unix_secs();
        AgentAnnouncement {
            peer_id: did.to_string(),
            did: did.to_string(),
            name: did.to_string(),
            description: did.to_string(),
            capabilities: vec![],
            capability_specs: vec![spec],
            multiaddrs: vec![],
            protocols: vec![],
            version: "1".to_string(),
            timestamp: now,
            ttl: 300,
            did_document: None,
            evm_address: None,
            binding_proof_hash: None,
            anonymous_membership_proof: None,
            ed25519_signature: None,
            mldsa65_signature: None,
        }
    }

    fn base_query(domain_prefix: &str) -> CapabilityQuery {
        CapabilityQuery {
            domain_prefix: domain_prefix.to_string(),
            required_inputs: vec![TypeSpec::LeanTerm],
            required_outputs: vec![TypeSpec::LeanTerm],
            required_constraints: vec![],
            min_success_rate: None,
            max_latency_p99_ms: None,
            max_cost_microdollars: None,
            min_attestations: None,
            min_onchain_reputation: None,
            count: 1,
            query_timeout_ms: 250,
        }
    }

    #[test]
    fn manifold_forms_only_when_all_required_slots_are_satisfied() {
        let mut discovery = AgentDiscovery::new();
        discovery.upsert_trusted_announcement(announcement(
            "did:key:a",
            spec("prove/lean/algebra", "did:key:a", 0.9, 40, 5),
        ));
        let manifold = TaskManifold {
            task_id: "task-1".to_string(),
            description: "prove then translate".to_string(),
            slots: vec![
                CapabilitySlot {
                    slot_id: "prove".to_string(),
                    query: base_query("prove/lean"),
                    redundancy: 1,
                    optional: false,
                },
                CapabilitySlot {
                    slot_id: "translate".to_string(),
                    query: base_query("translate/coq"),
                    redundancy: 1,
                    optional: false,
                },
            ],
            edges: vec![TaskEdge {
                from_slot: "prove".to_string(),
                to_slot: "translate".to_string(),
                output_index: 0,
                input_index: 0,
            }],
            constraints: ManifoldConstraints::default(),
            originator_did: "did:key:origin".to_string(),
            created_at: 100,
            formation_timeout_ms: 250,
            expires_at: 200,
        };
        let err = manifold
            .assemble_atomic(&discovery, 150, 3600)
            .expect_err("missing required slot must fail atomically");
        assert!(err.contains("translate"));
    }

    #[test]
    fn manifold_enforces_latency_and_distinct_agent_constraints() {
        let mut discovery = AgentDiscovery::new();
        discovery.upsert_trusted_announcement(announcement(
            "did:key:a",
            spec("prove/lean/algebra", "did:key:a", 0.95, 70, 5),
        ));
        discovery.upsert_trusted_announcement(announcement(
            "did:key:b",
            spec("translate/coq/to-lean", "did:key:b", 0.92, 60, 10),
        ));
        let manifold = TaskManifold {
            task_id: "task-2".to_string(),
            description: "prove then translate".to_string(),
            slots: vec![
                CapabilitySlot {
                    slot_id: "prove".to_string(),
                    query: base_query("prove/lean"),
                    redundancy: 1,
                    optional: false,
                },
                CapabilitySlot {
                    slot_id: "translate".to_string(),
                    query: base_query("translate/coq"),
                    redundancy: 1,
                    optional: false,
                },
            ],
            edges: vec![],
            constraints: ManifoldConstraints {
                max_total_latency_ms: Some(100),
                max_total_cost_microdollars: Some(20),
                min_distinct_agents: Some(2),
                require_mutual_attestation: false,
            },
            originator_did: "did:key:origin".to_string(),
            created_at: 100,
            formation_timeout_ms: 250,
            expires_at: 200,
        };
        let err = manifold
            .assemble_atomic(&discovery, 150, 3600)
            .expect_err("latency budget should fail");
        assert!(err.contains("max_total_latency_ms"));
    }

    #[test]
    fn ephemeral_group_dissolves_after_completion() {
        let mut discovery = AgentDiscovery::new();
        discovery.upsert_trusted_announcement(announcement(
            "did:key:a",
            spec("prove/lean/algebra", "did:key:a", 0.9, 40, 5),
        ));
        let manifold = TaskManifold {
            task_id: "task-3".to_string(),
            description: "prove".to_string(),
            slots: vec![CapabilitySlot {
                slot_id: "prove".to_string(),
                query: base_query("prove/lean"),
                redundancy: 1,
                optional: false,
            }],
            edges: vec![],
            constraints: ManifoldConstraints::default(),
            originator_did: "did:key:origin".to_string(),
            created_at: 100,
            formation_timeout_ms: 250,
            expires_at: 200,
        };
        let formation = manifold
            .assemble_atomic(&discovery, 150, 3600)
            .expect("all slots satisfied");
        assert_eq!(
            formation.assignments[0].group_topic,
            "/agenthalo/tasks/task-3/group"
        );
        let mut group = EphemeralTaskGroup::from_formation(formation);
        group.dissolve(199);
        assert_eq!(group.dissolved_at, Some(199));
    }

    #[test]
    fn self_attestations_do_not_satisfy_mutual_attestation_constraint() {
        let left = spec("prove/lean/algebra", "did:key:a", 0.95, 20, 5);
        let right = spec("translate/coq/to-lean", "did:key:b", 0.94, 25, 6);
        let assignments = vec![
            TaskAssignment {
                task_id: "task-4".to_string(),
                slot_id: "prove".to_string(),
                assigned_did: "did:key:a".to_string(),
                capability_id: left.capability_id.clone(),
                group_topic: "/agenthalo/tasks/task-4/group".to_string(),
                estimated_latency_ms: 40,
                cost_microdollars: 5,
            },
            TaskAssignment {
                task_id: "task-4".to_string(),
                slot_id: "translate".to_string(),
                assigned_did: "did:key:b".to_string(),
                capability_id: right.capability_id.clone(),
                group_topic: "/agenthalo/tasks/task-4/group".to_string(),
                estimated_latency_ms: 50,
                cost_microdollars: 6,
            },
        ];
        let specs = HashMap::from([
            (left.capability_id.clone(), left),
            (right.capability_id.clone(), right),
        ]);
        assert!(!mutual_attestation_satisfied(
            &assignments,
            &specs,
            150,
            3600
        ));
    }

    #[test]
    fn manifold_prefers_lower_latency_and_higher_attestation_tiebreakers() {
        let mut discovery = AgentDiscovery::new();
        let attester_one =
            crate::halo::did::did_from_genesis_seed(&[0x91; 64]).expect("attester one");
        let attester_two =
            crate::halo::did::did_from_genesis_seed(&[0x92; 64]).expect("attester two");
        discovery.upsert_trusted_announcement(
            crate::halo::p2p_discovery::announcement_for_identity(
                &attester_one,
                libp2p::PeerId::random(),
                vec![],
                vec![],
            ),
        );
        discovery.upsert_trusted_announcement(
            crate::halo::p2p_discovery::announcement_for_identity(
                &attester_two,
                libp2p::PeerId::random(),
                vec![],
                vec![],
            ),
        );
        let mut better = spec("prove/lean/algebra", "did:key:a", 0.95, 10, 5);
        better.attestations = vec![
            crate::halo::capability_verification::attest_capability(
                &attester_one,
                "did:key:a",
                &better.capability_id,
                "h1",
                true,
                100,
            )
            .expect("attestation 1"),
            crate::halo::capability_verification::attest_capability(
                &attester_two,
                "did:key:a",
                &better.capability_id,
                "h2",
                true,
                100,
            )
            .expect("attestation 2"),
        ];
        let worse = spec("prove/lean/algebra", "did:key:b", 0.95, 200, 50);
        discovery.upsert_trusted_announcement(announcement("did:key:a", better));
        discovery.upsert_trusted_announcement(announcement("did:key:b", worse));

        let manifold = TaskManifold {
            task_id: "task-rank".to_string(),
            description: "rank best provider".to_string(),
            slots: vec![CapabilitySlot {
                slot_id: "prove".to_string(),
                query: base_query("prove/lean"),
                redundancy: 1,
                optional: false,
            }],
            edges: vec![],
            constraints: ManifoldConstraints::default(),
            originator_did: "did:key:origin".to_string(),
            created_at: 100,
            formation_timeout_ms: 250,
            expires_at: 200,
        };
        let formation = manifold
            .assemble_atomic(&discovery, 150, 3600)
            .expect("best provider selected");
        assert_eq!(formation.assignments.len(), 1);
        assert_eq!(formation.assignments[0].assigned_did, "did:key:a");
    }

    #[test]
    fn task_formation_group_id_is_deterministic() {
        let mut discovery = AgentDiscovery::new();
        discovery.upsert_trusted_announcement(announcement(
            "did:key:a",
            spec("prove/lean/algebra", "did:key:a", 0.95, 10, 5),
        ));
        let manifold = TaskManifold {
            task_id: "task-deterministic".to_string(),
            description: "deterministic group id".to_string(),
            slots: vec![CapabilitySlot {
                slot_id: "prove".to_string(),
                query: base_query("prove/lean"),
                redundancy: 1,
                optional: false,
            }],
            edges: vec![],
            constraints: ManifoldConstraints::default(),
            originator_did: "did:key:origin".to_string(),
            created_at: 100,
            formation_timeout_ms: 250,
            expires_at: 200,
        };
        let first = manifold
            .assemble_atomic(&discovery, 150, 3600)
            .expect("first formation");
        let second = manifold
            .assemble_atomic(&discovery, 150, 3600)
            .expect("second formation");
        assert_eq!(first.group_id, second.group_id);
    }
}
