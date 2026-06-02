use std::collections::BTreeMap;
use std::sync::Mutex;
use koakuma_core::state::StateStore;
use koakuma_core::value::Value;

/// An in-memory implementation of [`StateStore`] backed by a `Mutex<BTreeMap>`.
///
/// All operations are thread-safe. Intended for development, testing, and the
/// M1 runtime. Replace with a persistent implementation for production use.
///
/// # Examples
///
/// ```rust
/// use koakuma_store::InMemoryStateStore;
/// use koakuma_core::state::StateStore;
/// use koakuma_core::value::Value;
///
/// let store = InMemoryStateStore::new();
/// store.set("hits", Value::Int(0));
/// let new_val = store.increment("hits", 1);
/// assert_eq!(new_val, 1);
/// assert_eq!(store.get("hits"), Some(Value::Int(1)));
/// ```
pub struct InMemoryStateStore {
    data: Mutex<BTreeMap<String, Value>>,
}

impl InMemoryStateStore {
    /// Creates an empty store.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use koakuma_store::InMemoryStateStore;
    /// use koakuma_core::state::StateStore;
    ///
    /// let store = InMemoryStateStore::new();
    /// assert!(store.snapshot().is_empty());
    /// ```
    pub fn new() -> Self {
        Self { data: Mutex::new(BTreeMap::new()) }
    }
}

impl Default for InMemoryStateStore {
    fn default() -> Self { Self::new() }
}

impl StateStore for InMemoryStateStore {
    fn get(&self, key: &str) -> Option<Value> {
        self.data.lock().unwrap().get(key).cloned()
    }

    fn set(&self, key: &str, value: Value) {
        self.data.lock().unwrap().insert(key.to_string(), value);
    }

    fn increment(&self, key: &str, by: i64) -> i64 {
        let mut data = self.data.lock().unwrap();
        let current = match data.get(key) {
            Some(Value::Int(n)) => *n,
            _ => 0,
        };
        let next = current + by;
        data.insert(key.to_string(), Value::Int(next));
        next
    }

    fn remove(&self, key: &str) {
        self.data.lock().unwrap().remove(key);
    }

    fn snapshot(&self) -> BTreeMap<String, Value> {
        self.data.lock().unwrap().clone()
    }
}
