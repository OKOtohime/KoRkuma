use std::path::PathBuf;

use koakuma_core::domain::{ActionConfig, ConstraintExpr, Macro, MacroId, TriggerConfig};
use koakuma_core::permission::PermissionSet;
use koakuma_core::state::StateStore;
use koakuma_core::value::Value;
use koakuma_store::{load_macros, save_macros, InMemoryStateStore, StoreError};

// ── Test helpers ──────────────────────────────────────────────────────────────

fn tmp_path(tag: &str) -> PathBuf {
    std::env::temp_dir()
        .join(format!("koakuma_store_test_{}_{}.json", std::process::id(), tag))
}

fn make_macro(name: &str, enabled: bool) -> Macro {
    Macro {
        id: MacroId::nil(),
        name: name.to_string(),
        description: String::new(),
        enabled,
        category: None,
        triggers: vec![TriggerConfig::Manual],
        constraints: ConstraintExpr::Always,
        actions: vec![ActionConfig::Notify {
            title: "t".to_string(),
            body: "b".to_string(),
        }],
        granted_permissions: PermissionSet::default(),
    }
}

// ── load_macros ───────────────────────────────────────────────────────────────

#[test]
fn load_returns_empty_vec_for_missing_file() {
    let path = tmp_path("load_missing");
    let _ = std::fs::remove_file(&path);
    assert!(load_macros(&path).unwrap().is_empty());
}

#[test]
fn load_returns_empty_vec_for_empty_json_array() {
    let path = tmp_path("load_empty_array");
    std::fs::write(&path, b"[]").unwrap();
    assert!(load_macros(&path).unwrap().is_empty());
    let _ = std::fs::remove_file(&path);
}

#[test]
fn load_returns_json_error_for_garbage_content() {
    let path = tmp_path("load_garbage");
    std::fs::write(&path, b"not json at all !@#$").unwrap();
    assert!(matches!(load_macros(&path), Err(StoreError::Json(_))));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn load_returns_json_error_for_wrong_shape() {
    let path = tmp_path("load_wrong_shape");
    // Valid JSON but an object, not an array.
    std::fs::write(&path, b"{\"type\": \"object\"}").unwrap();
    assert!(matches!(load_macros(&path), Err(StoreError::Json(_))));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn load_returns_json_error_for_array_of_strings() {
    let path = tmp_path("load_str_array");
    std::fs::write(&path, br#"["not","a","macro"]"#).unwrap();
    assert!(matches!(load_macros(&path), Err(StoreError::Json(_))));
    let _ = std::fs::remove_file(&path);
}

// ── save_macros ───────────────────────────────────────────────────────────────

#[test]
fn save_empty_slice_creates_valid_file() {
    let path = tmp_path("save_empty");
    save_macros(&path, &[]).unwrap();
    assert!(path.exists());
    let content = std::fs::read_to_string(&path).unwrap();
    assert_eq!(content.trim(), "[]");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn save_does_not_leave_tmp_file() {
    let path = tmp_path("save_no_tmp");
    save_macros(&path, &[make_macro("X", true)]).unwrap();
    let tmp = path.with_extension("json.tmp");
    assert!(!tmp.exists(), ".tmp file must be removed after atomic rename");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn save_writes_macro_name_to_file() {
    let path = tmp_path("save_name");
    save_macros(&path, &[make_macro("SpecialName", true)]).unwrap();
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.contains("SpecialName"));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn save_overwrites_existing_content() {
    let path = tmp_path("save_overwrite");
    save_macros(&path, &[make_macro("First", true)]).unwrap();
    save_macros(&path, &[make_macro("Second", false)]).unwrap();
    let loaded = load_macros(&path).unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].name, "Second");
    let _ = std::fs::remove_file(&path);
}

// ── round-trip ────────────────────────────────────────────────────────────────

#[test]
fn round_trip_single_macro_preserves_fields() {
    let path = tmp_path("rt_single");
    let original = make_macro("Persist", false);
    save_macros(&path, &[original]).unwrap();
    let loaded = load_macros(&path).unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].name, "Persist");
    assert!(!loaded[0].enabled);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn round_trip_preserves_order_for_multiple_macros() {
    let path = tmp_path("rt_multi");
    let names = ["Ant", "Bear", "Cat"];
    let macros: Vec<Macro> = names.iter().map(|n| make_macro(n, true)).collect();
    save_macros(&path, &macros).unwrap();
    let loaded = load_macros(&path).unwrap();
    assert_eq!(loaded.len(), 3);
    for (i, name) in names.iter().enumerate() {
        assert_eq!(&loaded[i].name, name);
    }
    let _ = std::fs::remove_file(&path);
}

#[test]
fn round_trip_disabled_macro_stays_disabled() {
    let path = tmp_path("rt_disabled");
    save_macros(&path, &[make_macro("Off", false)]).unwrap();
    let loaded = load_macros(&path).unwrap();
    assert!(!loaded[0].enabled);
    let _ = std::fs::remove_file(&path);
}

// ── InMemoryStateStore ────────────────────────────────────────────────────────

#[test]
fn state_get_missing_key_returns_none() {
    let store = InMemoryStateStore::new();
    assert_eq!(store.get("no_such_key"), None);
}

#[test]
fn state_set_then_get_returns_value() {
    let store = InMemoryStateStore::new();
    store.set("k", Value::Int(42));
    assert_eq!(store.get("k"), Some(Value::Int(42)));
}

#[test]
fn state_set_overwrites_previous_value() {
    let store = InMemoryStateStore::new();
    store.set("k", Value::Int(1));
    store.set("k", Value::Int(99));
    assert_eq!(store.get("k"), Some(Value::Int(99)));
}

#[test]
fn state_remove_deletes_key() {
    let store = InMemoryStateStore::new();
    store.set("k", Value::Bool(true));
    store.remove("k");
    assert_eq!(store.get("k"), None);
}

#[test]
fn state_remove_missing_key_is_noop() {
    let store = InMemoryStateStore::new();
    store.remove("ghost"); // must not panic
}

#[test]
fn state_increment_on_missing_key_starts_from_zero() {
    let store = InMemoryStateStore::new();
    let result = store.increment("counter", 1);
    assert_eq!(result, 1);
    assert_eq!(store.get("counter"), Some(Value::Int(1)));
}

#[test]
fn state_increment_accumulates_across_calls() {
    let store = InMemoryStateStore::new();
    assert_eq!(store.increment("n", 3), 3);
    assert_eq!(store.increment("n", 7), 10);
    assert_eq!(store.increment("n", -2), 8);
}

#[test]
fn state_increment_on_non_int_treats_as_zero() {
    let store = InMemoryStateStore::new();
    store.set("k", Value::Bool(true));
    assert_eq!(store.increment("k", 5), 5);
}

#[test]
fn state_snapshot_contains_all_current_entries() {
    let store = InMemoryStateStore::new();
    store.set("a", Value::Int(1));
    store.set("b", Value::Str("hello".to_string()));
    let snap = store.snapshot();
    assert_eq!(snap.len(), 2);
    assert_eq!(snap.get("a").cloned(), Some(Value::Int(1)));
    assert_eq!(snap.get("b").cloned(), Some(Value::Str("hello".to_string())));
}

#[test]
fn state_snapshot_is_independent_of_later_mutations() {
    let store = InMemoryStateStore::new();
    store.set("x", Value::Int(10));
    let snap = store.snapshot();
    store.set("x", Value::Int(999));
    assert_eq!(snap.get("x").cloned(), Some(Value::Int(10)));
    assert_eq!(store.get("x"), Some(Value::Int(999)));
}

#[test]
fn state_snapshot_empty_on_new_store() {
    assert!(InMemoryStateStore::new().snapshot().is_empty());
}

#[test]
fn state_default_is_equivalent_to_new() {
    let a = InMemoryStateStore::default();
    assert!(a.snapshot().is_empty());
}