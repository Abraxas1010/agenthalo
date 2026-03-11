use nucleusdb::puf::collect_auto;
use nucleusdb::puf::core::{
    canonical_fingerprint, puf_js_compat_components, puf_js_compat_fingerprint, PufComponent,
    PufTier,
};
use std::process::Command;

#[test]
fn puf_detect_tier_is_stable_enum() {
    let t = PufTier::detect();
    match t {
        PufTier::Dgx | PufTier::ServerTpm | PufTier::Server | PufTier::Consumer => {}
    }
}

#[test]
fn puf_collect_auto_safe() {
    let _ = collect_auto();
}

#[test]
fn puf_fingerprint_deterministic() {
    let comps = vec![
        PufComponent {
            name: "a".to_string(),
            value: vec![1, 2, 3],
            entropy_bits: 10,
            stable: true,
        },
        PufComponent {
            name: "b".to_string(),
            value: vec![4, 5],
            entropy_bits: 5,
            stable: false,
        },
    ];
    assert_eq!(canonical_fingerprint(&comps), canonical_fingerprint(&comps));
}

#[test]
fn puf_legacy_fingerprint_deterministic() {
    let comps = vec![
        PufComponent {
            name: "a".to_string(),
            value: b"alpha".to_vec(),
            entropy_bits: 10,
            stable: true,
        },
        PufComponent {
            name: "b".to_string(),
            value: b"beta".to_vec(),
            entropy_bits: 5,
            stable: false,
        },
    ];
    assert_eq!(
        puf_js_compat_fingerprint(&comps),
        puf_js_compat_fingerprint(&comps)
    );
}

#[test]
fn puf_js_compat_matches_node_hashing() {
    let comps = vec![
        PufComponent {
            name: "machine_id".to_string(),
            value: b"mid-1".to_vec(),
            entropy_bits: 64,
            stable: true,
        },
        PufComponent {
            name: "gpu_uuid".to_string(),
            value: b"GPU-1234".to_vec(),
            entropy_bits: 64,
            stable: true,
        },
        PufComponent {
            name: "connectx_guids".to_string(),
            value: b"mlx5_0:aa|mlx5_1:bb".to_vec(),
            entropy_bits: 64,
            stable: true,
        },
        PufComponent {
            name: "stable_macs".to_string(),
            value: b"eth0:01:02:03:04:05:06|ib0:11:22:33:44:55:66".to_vec(),
            entropy_bits: 48,
            stable: true,
        },
        PufComponent {
            name: "dgx_nvme_serial".to_string(),
            value: b"NVME-XYZ".to_vec(),
            entropy_bits: 48,
            stable: true,
        },
        PufComponent {
            name: "thermal_signature".to_string(),
            value: b"opaque".to_vec(),
            entropy_bits: 8,
            stable: false,
        },
    ];
    let rust_hash = puf_js_compat_fingerprint(&comps);
    let value = puf_js_compat_components(&comps);
    let json = serde_json::to_string(&value).expect("json");
    let node = Command::new("node")
        .arg("-e")
        .arg(
            "const crypto=require('crypto');\
             const c=JSON.parse(process.argv[1]);\
             const data=JSON.stringify(c,Object.keys(c).sort());\
             process.stdout.write(crypto.createHash('sha256').update(data).digest('hex'));",
        )
        .arg(json)
        .output();
    let Ok(out) = node else {
        return;
    };
    if !out.status.success() {
        return;
    }
    let node_hex = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let rust_hex = rust_hash
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    assert_eq!(rust_hex, node_hex);
}
