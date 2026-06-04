use crate::value::Value;
use std::collections::BTreeMap;

/// Thread-safe key-value store for variables shared across macro executions.
///
/// The engine and action implementations access global state through this trait.
/// Use `korkuma_store::InMemoryStateStore` for development and testing; replace
/// with a persistent implementation for production deployments.
///
/// # Examples
///
/// ```rust
/// # use korkuma_store::InMemoryStateStore;
/// use korkuma_core::state::StateStore;
/// use korkuma_core::value::Value;
///
/// let store = InMemoryStateStore::new();
/// store.set("x", Value::Int(10));
/// assert_eq!(store.get("x"), Some(Value::Int(10)));
/// store.remove("x");
/// assert_eq!(store.get("x"), None);
/// ```
pub trait StateStore: Send + Sync {
    /// Returns the value stored under `key`, or `None` if the key does not exist.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use korkuma_store::InMemoryStateStore;
    /// use korkuma_core::state::StateStore;
    /// use korkuma_core::value::Value;
    ///
    /// let store = InMemoryStateStore::new();
    /// assert_eq!(store.get("missing"), None);
    /// store.set("k", Value::Bool(true));
    /// assert_eq!(store.get("k"), Some(Value::Bool(true)));
    /// ```
    fn get(&self, key: &str) -> Option<Value>;

    /// Inserts or replaces the value stored under `key`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use korkuma_store::InMemoryStateStore;
    /// use korkuma_core::state::StateStore;
    /// use korkuma_core::value::Value;
    ///
    /// let store = InMemoryStateStore::new();
    /// store.set("n", Value::Int(1));
    /// store.set("n", Value::Int(2));
    /// assert_eq!(store.get("n"), Some(Value::Int(2)));
    /// ```
    fn set(&self, key: &str, value: Value);

    /// Atomic increment — enables "trigger N times in M minutes" patterns.
    fn increment(&self, key: &str, by: i64) -> i64;

    /// Removes the value stored under `key`. No-op if the key does not exist.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use korkuma_store::InMemoryStateStore;
    /// use korkuma_core::state::StateStore;
    /// use korkuma_core::value::Value;
    ///
    /// let store = InMemoryStateStore::new();
    /// store.set("tmp", Value::Str("hello".to_string()));
    /// store.remove("tmp");
    /// assert_eq!(store.get("tmp"), None);
    /// store.remove("tmp"); // no-op: key already absent
    /// ```
    fn remove(&self, key: &str);

    /// Full snapshot for the UI variable monitor.
    fn snapshot(&self) -> BTreeMap<String, Value>;
}
