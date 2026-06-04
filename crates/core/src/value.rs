use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The core dynamic value type — decouples domain logic from serde_json and rhai.
///
/// `Value` is the universal data carrier used for event payloads, state variables,
/// action parameters, and constraint operands throughout the pipeline.
///
/// # Examples
///
/// ```rust
/// use korkuma_core::value::Value;
/// use std::collections::BTreeMap;
///
/// let int_val = Value::Int(42);
/// let str_val = Value::Str("hello".to_string());
/// let map_val = Value::Map(BTreeMap::from([
///     ("count".to_string(), Value::Int(3)),
/// ]));
///
/// assert_eq!(int_val, Value::Int(42));
/// assert_ne!(int_val, str_val);
/// assert!(matches!(map_val, Value::Map(_)));
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    List(Vec<Value>),
    Map(BTreeMap<String, Value>),
}
