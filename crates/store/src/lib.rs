//! State persistence: in-memory runtime store and JSON macro file I/O.
//!
//! # Runtime store
//!
//! [`InMemoryStateStore`] implements [`StateStore`] for development and M1 runtime use.
//!
//! # JSON persistence
//!
//! [`load_macros`] and [`save_macros`] provide atomic read/write for `macros.json`.
//! Saves write to a `.tmp` side-file, then rename to guarantee the on-disk file is
//! never left in a partially-written state.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Mutex;

use korkuma_core::domain::Macro;
use korkuma_core::state::StateStore;
use korkuma_core::value::Value;
use thiserror::Error;

// ── Error ─────────────────────────────────────────────────────────────────────

/// Errors that can occur when loading or saving the macro configuration file.
///
/// # Examples
///
/// ```rust
/// use korkuma_store::StoreError;
///
/// let io_err: StoreError = std::io::Error::from(std::io::ErrorKind::PermissionDenied).into();
/// assert!(matches!(io_err, StoreError::Io(_)));
/// ```
#[derive(Debug, Error)]
pub enum StoreError {
    /// A filesystem operation failed.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    /// JSON serialisation or deserialisation failed.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

// ── JSON persistence ──────────────────────────────────────────────────────────

/// Deserialises a [`Macro`] list from a UTF-8 JSON file.
///
/// Returns an empty `Vec` if the file does not exist, so callers can treat a
/// missing file the same as an empty configuration.
///
/// # Errors
///
/// Returns [`StoreError::Io`] for filesystem errors other than *not-found*, and
/// [`StoreError::Json`] if the file content is not a valid macro array.
///
/// # Examples
///
/// ```rust,no_run
/// use korkuma_store::load_macros;
///
/// let macros = load_macros(std::path::Path::new("macros.json")).unwrap();
/// println!("loaded {} macro(s)", macros.len());
/// ```
pub fn load_macros(path: &Path) -> Result<Vec<Macro>, StoreError> {
    match std::fs::read_to_string(path) {
        Ok(data) => Ok(serde_json::from_str(&data)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(StoreError::Io(e)),
    }
}

/// Atomically serialises `macros` to a JSON file at `path`.
///
/// The function writes to `<path>.tmp`, then renames to `path`. A crash during
/// writing leaves the original file intact — only a successful `rename` replaces
/// it.
///
/// # Errors
///
/// Returns [`StoreError::Json`] if serialisation fails, or [`StoreError::Io`]
/// for any filesystem error.
///
/// # Examples
///
/// ```rust,no_run
/// use korkuma_store::save_macros;
///
/// save_macros(std::path::Path::new("macros.json"), &[]).unwrap();
/// ```
pub fn save_macros(path: &Path, macros: &[Macro]) -> Result<(), StoreError> {
    let json = serde_json::to_string_pretty(macros)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json.as_bytes())?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

// ── In-memory runtime store ───────────────────────────────────────────────────

/// An in-memory implementation of [`StateStore`] backed by a `Mutex<BTreeMap>`.
///
/// All operations are thread-safe. Intended for development, testing, and the
/// M1 runtime. Replace with a persistent implementation for production use.
///
/// # Examples
///
/// ```rust
/// use korkuma_store::InMemoryStateStore;
/// use korkuma_core::state::StateStore;
/// use korkuma_core::value::Value;
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
    /// use korkuma_store::InMemoryStateStore;
    /// use korkuma_core::state::StateStore;
    ///
    /// let store = InMemoryStateStore::new();
    /// assert!(store.snapshot().is_empty());
    /// ```
    pub fn new() -> Self {
        Self {
            data: Mutex::new(BTreeMap::new()),
        }
    }
}

impl Default for InMemoryStateStore {
    fn default() -> Self {
        Self::new()
    }
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
