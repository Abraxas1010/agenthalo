use nucleusdb::keymap::KeyMap;

#[test]
fn keymap_get_or_create_is_stable() {
    let mut km = KeyMap::new();
    let a = km.get_or_create("alpha");
    let b = km.get_or_create("beta");
    let a2 = km.get_or_create("alpha");

    assert_eq!(a, 0);
    assert_eq!(b, 1);
    assert_eq!(a2, a);
    assert_eq!(km.len(), 2);
}

#[test]
fn keymap_reverse_lookup_and_iteration_work() {
    let mut km = KeyMap::new();
    let i0 = km.get_or_create("k0");
    let i1 = km.get_or_create("k1");

    assert_eq!(km.get("k0"), Some(i0));
    assert_eq!(km.get("k1"), Some(i1));
    assert_eq!(km.key_at(i0), Some("k0"));
    assert_eq!(km.key_at(i1), Some("k1"));

    let pairs: Vec<_> = km.all_keys().collect();
    assert_eq!(pairs, vec![("k0", 0), ("k1", 1)]);
}

#[test]
fn keymap_like_prefix_and_exact_matching() {
    let mut km = KeyMap::new();
    km.get_or_create("temp_a");
    km.get_or_create("temp_b");
    km.get_or_create("other");

    let pref: Vec<_> = km
        .keys_matching("temp%")
        .into_iter()
        .map(|(k, _)| k)
        .collect();
    assert_eq!(pref, vec!["temp_a".to_string(), "temp_b".to_string()]);

    let exact = km.keys_matching("other");
    assert_eq!(exact.len(), 1);
    assert_eq!(exact[0].0, "other".to_string());
}
