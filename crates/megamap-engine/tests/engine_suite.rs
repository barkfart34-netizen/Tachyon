//! Unit tests from upstream `Teiwazik/teiwaziks-all-in-one-map` plus Tachyon i18n verification.

pub use megamap_engine as engine;

#[path = "suite/fake.rs"]
pub mod fake;
#[path = "suite/golden.rs"]
pub mod golden;
#[path = "suite/tests.rs"]
mod tests;

#[test]
fn id_id_json_translates_every_key_via_game_lookup() {
    use std::collections::BTreeMap;
    use std::path::Path;
    use std::sync::OnceLock;

    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tachyon/content/megamap/assets/megamap/lang/id_id.json");
    let raw = std::fs::read_to_string(&path).expect("id_id.json exists");
    let map: BTreeMap<String, String> = serde_json::from_str(&raw).expect("valid id_id.json");
    assert!(map.len() >= 450, "expected >=450 keys, got {}", map.len());

    static TABLE: OnceLock<BTreeMap<String, String>> = OnceLock::new();
    let _ = TABLE.set(map.clone());
    fn lookup(key: &str) -> Option<String> {
        TABLE.get().and_then(|t| t.get(key).cloned())
    }

    let tr = engine::lang::Tr {
        lang: engine::lang::Lang::En,
        game: Some(lookup),
    };
    for (k, expected) in &map {
        assert_eq!(tr.t(k), *expected, "mismatch for key {k}");
    }
}
