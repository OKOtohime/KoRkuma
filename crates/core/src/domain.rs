use std::path::PathBuf;
use serde::{Deserialize, Serialize};
use crate::permission::PermissionSet;
use crate::value::Value;

/// Unique identifier for a [`Macro`], backed by a UUID v4.
///
/// # Examples
///
/// ```rust
/// use koakuma_core::domain::MacroId;
///
/// let id = MacroId::new_v4();
/// assert!(!id.is_nil());
/// ```
pub type MacroId = uuid::Uuid;

/// A keyboard shortcut consisting of zero or more modifier keys and one primary key.
///
/// # Examples
///
/// ```rust
/// use koakuma_core::domain::KeyCombo;
///
/// let combo = KeyCombo {
///     modifiers: vec!["Ctrl".to_string(), "Shift".to_string()],
///     key: "S".to_string(),
/// };
/// assert_eq!(combo.key, "S");
/// assert_eq!(combo.modifiers.len(), 2);
/// ```
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KeyCombo {
    pub modifiers: Vec<String>,
    pub key: String,
}

/// Whether a process was started or stopped.
///
/// # Examples
///
/// ```rust
/// use koakuma_core::domain::ProcessEvent;
///
/// let ev = ProcessEvent::Started;
/// assert!(matches!(ev, ProcessEvent::Started));
/// ```
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum ProcessEvent {
    Started,
    Stopped,
}

/// The kind of filesystem change that triggers a [`TriggerConfig::FileChange`] rule.
///
/// # Examples
///
/// ```rust
/// use koakuma_core::domain::FsEventKind;
///
/// let kind = FsEventKind::Modified;
/// assert!(matches!(kind, FsEventKind::Modified));
/// ```
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum FsEventKind {
    Created,
    Modified,
    Deleted,
}

/// Which variable namespace a SetVariable action targets.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum VarScope {
    Global,
    Local,
}

/// Binary comparison operator used in [`ConstraintConfig::VarCompare`].
///
/// # Examples
///
/// ```rust
/// use koakuma_core::domain::CompareOp;
///
/// let op = CompareOp::Ge;
/// assert!(matches!(op, CompareOp::Ge));
/// ```
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum CompareOp {
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
}

/// The scripting language used by [`ActionConfig::RunScript`].
///
/// # Examples
///
/// ```rust
/// use koakuma_core::domain::ScriptLang;
///
/// let lang = ScriptLang::Rhai;
/// assert!(matches!(lang, ScriptLang::Rhai));
/// ```
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum ScriptLang {
    Rhai,
}

/// A single step in a simulated input sequence.
///
/// # Examples
///
/// ```rust
/// use koakuma_core::domain::InputEvent;
///
/// let press = InputEvent::KeyPress { key: "Enter".to_string() };
/// let click = InputEvent::MouseClick { button: "left".to_string() };
/// assert!(matches!(press, InputEvent::KeyPress { .. }));
/// assert!(matches!(click, InputEvent::MouseClick { .. }));
/// ```
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum InputEvent {
    KeyPress   { key: String },
    KeyRelease { key: String },
    Text       { text: String },
    MouseMove  { x: f64, y: f64 },
    MouseClick { button: String },
}

/// V1: literal values only; V2 will support template strings like `{{event.key}}`.
pub type ValueTemplate = Value;

/// Serializable description of a macro trigger condition.
///
/// Each variant corresponds to a platform event type. `TriggerConfig` is stored inside
/// a [`Macro`] and converted to a [`traits::TriggerSpec`] at dispatch time via
/// [`registry::Registry::build_trigger`].
///
/// # Examples
///
/// ```rust
/// use koakuma_core::domain::{TriggerConfig, KeyCombo};
///
/// let manual = TriggerConfig::Manual;
/// let hotkey = TriggerConfig::Hotkey {
///     keys: vec![KeyCombo { modifiers: vec!["Ctrl".to_string()], key: "F1".to_string() }],
/// };
/// assert!(matches!(manual, TriggerConfig::Manual));
/// assert!(matches!(hotkey, TriggerConfig::Hotkey { .. }));
/// ```
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum TriggerConfig {
    Hotkey      { keys: Vec<KeyCombo> },
    WindowFocus { title_pattern: String, regex: bool },
    Process     { name: String, event: ProcessEvent },
    Schedule    { cron: String },
    FileChange  { path: PathBuf, kind: FsEventKind },
    Manual,
    Custom      { provider: String, params: serde_json::Value },
}

/// Serializable description of a macro action step.
///
/// Each variant is converted to a [`traits::Action`] at dispatch time via
/// [`registry::Registry::build_action`].
///
/// # Examples
///
/// ```rust
/// use koakuma_core::domain::ActionConfig;
///
/// let delay = ActionConfig::Delay { millis: 500 };
/// let notify = ActionConfig::Notify {
///     title: "Done".to_string(),
///     body: "Macro finished.".to_string(),
/// };
/// assert!(matches!(delay, ActionConfig::Delay { millis: 500 }));
/// assert!(matches!(notify, ActionConfig::Notify { .. }));
/// ```
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ActionConfig {
    RunCommand    { program: String, args: Vec<String>, capture: bool },
    Notify        { title: String, body: String },
    SimulateInput { sequence: Vec<InputEvent> },
    HttpRequest   { method: String, url: String, body: Option<String> },
    SetVariable   { scope: VarScope, key: String, value: ValueTemplate },
    Delay         { millis: u64 },
    RunScript     { lang: ScriptLang, source: String },
    Custom        { provider: String, params: serde_json::Value },
}

/// Serializable description of a single constraint leaf node.
///
/// Leaf nodes in a [`ConstraintExpr`] tree are converted to [`traits::Constraint`]
/// objects at evaluation time via [`registry::Registry::build_constraint`].
///
/// # Examples
///
/// ```rust
/// use koakuma_core::domain::ConstraintConfig;
///
/// let time = ConstraintConfig::TimeRange {
///     from: "09:00".to_string(),
///     to: "17:00".to_string(),
/// };
/// assert!(matches!(time, ConstraintConfig::TimeRange { .. }));
/// ```
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ConstraintConfig {
    ActiveWindow { title_pattern: String, regex: bool },
    TimeRange    { from: String, to: String },
    VarCompare   { key: String, op: CompareOp, value: Value },
    Expression   { dsl: String },
    Custom       { provider: String, params: serde_json::Value },
}

/// Boolean expression tree for the Constraint leg of a Macro.
/// Empty / `Always` → unconditionally true.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op")]
pub enum ConstraintExpr {
    Always,
    Leaf { constraint: ConstraintConfig },
    Not  { expr: Box<ConstraintExpr> },
    All  { exprs: Vec<ConstraintExpr> },
    Any  { exprs: Vec<ConstraintExpr> },
}

impl ConstraintExpr {
    /// Recursively evaluates the expression tree, returning `true` if the macro should fire.
    ///
    /// Leaf nodes are built via `reg` and evaluated with `ctx`. `All` and `Any` short-circuit.
    ///
    /// # Errors
    ///
    /// Returns [`error::ConstraintError`] if any leaf node fails to build (unknown provider)
    /// or returns a runtime evaluation error.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use koakuma_core::event::{Event, EventKind};
    /// # use koakuma_core::value::Value;
    /// # use koakuma_core::context::EvalContext;
    /// # use koakuma_core::registry::Registry;
    /// # use koakuma_store::InMemoryStateStore;
    /// # use std::time::SystemTime;
    /// use koakuma_core::domain::ConstraintExpr;
    ///
    /// # let event = Event { kind: EventKind::Manual, source: "t".to_string(), timestamp: SystemTime::UNIX_EPOCH, payload: Value::Null };
    /// # let store = InMemoryStateStore::new();
    /// # let ctx = EvalContext { event: &event, macro_id: uuid::Uuid::nil(), store: &store };
    /// # let reg = Registry::with_builtins();
    /// assert!(ConstraintExpr::Always.evaluate(&ctx, &reg).unwrap());
    /// ```
    pub fn evaluate(
        &self,
        ctx: &crate::context::EvalContext,
        reg: &crate::registry::Registry,
    ) -> Result<bool, crate::error::ConstraintError> {
        match self {
            ConstraintExpr::Always => Ok(true),
            ConstraintExpr::Leaf { constraint: c } => reg.build_constraint(c)?.evaluate(ctx),
            ConstraintExpr::Not { expr: e } => Ok(!e.evaluate(ctx, reg)?),
            ConstraintExpr::All { exprs: v } => {
                for e in v {
                    if !e.evaluate(ctx, reg)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            ConstraintExpr::Any { exprs: v } => {
                for e in v {
                    if e.evaluate(ctx, reg)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
        }
    }
}

/// The core three-tuple (Hook + Constraint + Action). Serializable, diffable, shareable.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Macro {
    pub id: MacroId,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub category: Option<String>,
    /// OR semantics: any trigger fires evaluation.
    pub triggers: Vec<TriggerConfig>,
    pub constraints: ConstraintExpr,
    /// V1: sequential execution.
    pub actions: Vec<ActionConfig>,
    pub granted_permissions: PermissionSet,
}
