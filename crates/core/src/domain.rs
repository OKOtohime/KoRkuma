use crate::permission::PermissionSet;
use crate::value::Value;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
    KeyPress { key: String },
    KeyRelease { key: String },
    Text { text: String },
    MouseMove { x: f64, y: f64 },
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
    Hotkey {
        keys: Vec<KeyCombo>,
    },
    WindowFocus {
        title_pattern: String,
        regex: bool,
    },
    Process {
        name: String,
        event: ProcessEvent,
    },
    Schedule {
        cron: String,
    },
    FileChange {
        path: PathBuf,
        kind: FsEventKind,
    },
    Manual,
    Custom {
        provider: String,
        params: serde_json::Value,
    },
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
    RunCommand {
        program: String,
        args: Vec<String>,
        capture: bool,
    },
    Notify {
        title: String,
        body: String,
    },
    SimulateInput {
        sequence: Vec<InputEvent>,
    },
    HttpRequest {
        method: String,
        url: String,
        body: Option<String>,
    },
    SetVariable {
        scope: VarScope,
        key: String,
        value: ValueTemplate,
    },
    Delay {
        millis: u64,
    },
    RunScript {
        lang: ScriptLang,
        source: String,
    },
    /// V2: Background UI automation interaction (see DESIGN.md §13).
    ///
    /// Executes `op` against `target`, falling back according to `on_no_background`
    /// when no background-capable backend is available.
    Interact {
        /// Which window / tab to target.
        #[serde(default)]
        target: TargetSelector,
        /// The operation to perform.
        op: UiOp,
        /// Fallback policy when background is unavailable.
        #[serde(default)]
        on_no_background: OnNoBackground,
    },
    Custom {
        provider: String,
        params: serde_json::Value,
    },
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
    ActiveWindow {
        title_pattern: String,
        regex: bool,
    },
    TimeRange {
        from: String,
        to: String,
    },
    VarCompare {
        key: String,
        op: CompareOp,
        value: Value,
    },
    Expression {
        dsl: String,
    },
    Custom {
        provider: String,
        params: serde_json::Value,
    },
}

/// Boolean expression tree for the Constraint leg of a Macro.
/// Empty / `Always` → unconditionally true.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op")]
pub enum ConstraintExpr {
    Always,
    Leaf { constraint: ConstraintConfig },
    Not { expr: Box<ConstraintExpr> },
    All { exprs: Vec<ConstraintExpr> },
    Any { exprs: Vec<ConstraintExpr> },
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

/// Condition a [`WorkflowNode::Wait`] node blocks on before resuming.
///
/// V2 (M2.1) implements [`WaitCondition::Duration`]. Event- and variable-predicate
/// waits are planned for the M2.2 scheduler, which owns event subscription.
///
/// # Examples
///
/// ```rust
/// use koakuma_core::domain::WaitCondition;
///
/// let w = WaitCondition::Duration { millis: 250 };
/// assert!(matches!(w, WaitCondition::Duration { millis: 250 }));
/// ```
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WaitCondition {
    /// Sleep for a fixed number of milliseconds.
    Duration { millis: u64 },
}

/// A node in the executable workflow tree — the Action leg of the macro, with control flow.
///
/// V2 replaces the flat `Vec<ActionConfig>` of V1 with this recursive tree, driven
/// asynchronously by [`workflow::run_workflow`](crate::workflow::run_workflow). A macro
/// without an explicit `workflow` is treated as a [`Seq`](WorkflowNode::Seq) of its
/// `actions` (see [`Macro::root_workflow`]), so old configurations keep working unchanged.
///
/// # Control-flow semantics
///
/// - [`Seq`](WorkflowNode::Seq) — run children in order; stop on the first that halts or fails.
/// - [`Parallel`](WorkflowNode::Parallel) — run children concurrently, each with a forked
///   context (local variables are isolated; the global store is shared).
/// - [`If`](WorkflowNode::If) / [`While`](WorkflowNode::While) — gated by a [`ConstraintExpr`].
/// - [`ForEach`](WorkflowNode::ForEach) — iterate a literal [`Value::List`],
///   binding each element to a local variable.
/// - [`Retry`](WorkflowNode::Retry) — re-run a child up to `times` attempts on failure.
/// - [`Timeout`](WorkflowNode::Timeout) — fail a child if it does not finish in `millis`.
/// - [`Wait`](WorkflowNode::Wait) — block on a [`WaitCondition`].
///
/// # Examples
///
/// ```rust
/// use koakuma_core::domain::{WorkflowNode, ActionConfig};
///
/// let wf = WorkflowNode::Seq(vec![
///     WorkflowNode::Action(ActionConfig::Delay { millis: 100 }),
///     WorkflowNode::Action(ActionConfig::Notify {
///         title: "Done".into(),
///         body: "Finished".into(),
///     }),
/// ]);
/// assert!(matches!(wf, WorkflowNode::Seq(_)));
/// ```
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "node")]
pub enum WorkflowNode {
    /// A single leaf action.
    Action(ActionConfig),
    /// Run children sequentially; halt on the first Stop or failure.
    Seq(Vec<WorkflowNode>),
    /// Run children concurrently (each gets a forked execution context).
    Parallel(Vec<WorkflowNode>),
    /// Run `then` when `cond` is true, otherwise `otherwise` (if present).
    If {
        cond: ConstraintExpr,
        then: Box<WorkflowNode>,
        #[serde(default)]
        otherwise: Option<Box<WorkflowNode>>,
    },
    /// Repeat `body` while `cond` holds, bounded by `max_iter` iterations.
    While {
        cond: ConstraintExpr,
        body: Box<WorkflowNode>,
        max_iter: u32,
    },
    /// Bind each element of the literal list `items` to local `var` and run `body`.
    ForEach {
        items: ValueTemplate,
        var: String,
        body: Box<WorkflowNode>,
    },
    /// Re-run `body` up to `times` attempts on failure, sleeping `backoff_ms` between tries.
    Retry {
        body: Box<WorkflowNode>,
        times: u32,
        #[serde(default)]
        backoff_ms: u64,
    },
    /// Fail `body` if it does not complete within `millis` milliseconds.
    Timeout {
        body: Box<WorkflowNode>,
        millis: u64,
    },
    /// Block until `until` is satisfied.
    Wait { until: WaitCondition },
}

/// Serializable selector for the target of a background interaction.
///
/// Attached to [`ActionConfig::Interact`] to describe "which target" without
/// coupling the action config to a runtime backend.
///
/// # Examples
///
/// ```rust
/// use koakuma_core::domain::TargetSelector;
///
/// let fg = TargetSelector::Foreground;
/// let tab = TargetSelector::BrowserTab { url_pattern: "github.com".into() };
/// assert!(matches!(fg, TargetSelector::Foreground));
/// assert!(matches!(tab, TargetSelector::BrowserTab { .. }));
/// ```
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum TargetSelector {
    /// Send the operation to whatever window currently has focus (V1 default).
    #[default]
    Foreground,
    /// Find the first window whose title matches `title_pattern`.
    Window { title_pattern: String, regex: bool },
    /// Find the first window owned by a process named `name`.
    Process { name: String },
    /// Find the first browser tab whose URL contains `url_pattern`.
    BrowserTab { url_pattern: String },
    /// Plugin-defined selector.
    Custom { provider: String, params: serde_json::Value },
}

/// Path to a specific element within a UI tree.
///
/// Format is backend-dependent:
/// - **Windows UIA**: `"name:Submit"` (by accessible name), `"id:btn_ok"` (by AutomationId)
/// - **CDP / browser**: any CSS selector, e.g. `"#submit"` or `".btn-primary"`
/// - Empty string or omitted → targets the window/tab root.
pub type UiPath = String;

/// A backend-agnostic UI operation executed against a [`TargetSelector`].
///
/// # Examples
///
/// ```rust
/// use koakuma_core::domain::UiOp;
///
/// let click = UiOp::Click { node: "#submit".into() };
/// let set   = UiOp::SetText { node: "#search".into(), text: "hello".into() };
/// assert!(matches!(click, UiOp::Click { .. }));
/// assert!(matches!(set,   UiOp::SetText { .. }));
/// ```
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op")]
pub enum UiOp {
    /// Invoke / click the element at `node`.
    Click { node: UiPath },
    /// Set the value / text of the element at `node`.
    SetText { node: UiPath, text: String },
    /// Dispatch keyboard input.
    SendKeys { keys: Vec<KeyCombo> },
    /// Focus the element at `node`, or the root if `None`.
    Focus {
        #[serde(default)]
        node: Option<UiPath>,
    },
    /// Read and return the value of the element at `node` (logged via `ctx.log`).
    ReadValue { node: UiPath },
}

/// Fallback policy when no background-capable backend is available.
///
/// Attached to [`ActionConfig::Interact`] via `on_no_background`.
///
/// # Examples
///
/// ```rust
/// use koakuma_core::domain::OnNoBackground;
///
/// assert!(matches!(OnNoBackground::default(), OnNoBackground::Degrade));
/// ```
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum OnNoBackground {
    /// Degrade to foreground-synthetic (requires `ForegroundTakeover` permission).
    #[default]
    Degrade,
    /// Hard-fail; emit an error and abort the action.
    Fail,
    /// Queue the operation until the target becomes reachable.
    Queue,
}

/// Per-macro policy governing concurrent workflow executions when the same macro
/// is triggered repeatedly (e.g., a hotkey held down or fired in rapid succession).
///
/// See DESIGN.md §14.2 for the full semantics of each variant.
///
/// # Examples
///
/// ```rust
/// use koakuma_core::domain::ConcurrencyPolicy;
///
/// let p = ConcurrencyPolicy::Queue { max: 4 };
/// assert!(matches!(p, ConcurrencyPolicy::Queue { max: 4 }));
/// assert!(matches!(ConcurrencyPolicy::default(), ConcurrencyPolicy::Parallel));
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum ConcurrencyPolicy {
    /// Default: every trigger spawns an independent workflow (V1 behaviour).
    Parallel,
    /// Serialise executions; up to `max` pending triggers are queued, excess dropped.
    Queue { max: usize },
    /// Ignore new triggers while one instance is already running.
    DropIfRunning,
    /// Cancel the running instance and start a fresh one on each trigger.
    RestartIfRunning,
    /// Wait `ms` after the last trigger before executing; resets on each new trigger.
    Debounce { ms: u64 },
    /// Execute at most once per `ms` window; the first trigger in the window wins.
    Throttle { ms: u64 },
}

impl Default for ConcurrencyPolicy {
    fn default() -> Self {
        Self::Parallel
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
    /// Flat action list (V1). Used as the fallback workflow when `workflow` is `None`.
    pub actions: Vec<ActionConfig>,
    /// Optional V2 control-flow workflow tree. When present it takes precedence over
    /// `actions`. Absent in V1 configs; `#[serde(default)]` keeps them loadable.
    #[serde(default)]
    pub workflow: Option<WorkflowNode>,
    pub granted_permissions: PermissionSet,
    /// M2.2: dispatch priority — higher value fires first when multiple macros match
    /// the same event. Also determines queue ordering inside the scheduler.
    #[serde(default)]
    pub priority: i32,
    /// M2.2: concurrency policy applied when this macro is triggered repeatedly.
    #[serde(default)]
    pub concurrency: ConcurrencyPolicy,
}

impl Macro {
    /// Returns the root [`WorkflowNode`] to execute for this macro.
    ///
    /// If an explicit [`workflow`](Macro::workflow) is set it is returned as-is;
    /// otherwise the flat [`actions`](Macro::actions) list is wrapped in a
    /// [`WorkflowNode::Seq`], giving V1 configurations identical sequential semantics.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use koakuma_core::domain::{Macro, ConstraintExpr, TriggerConfig, ActionConfig, WorkflowNode};
    /// use koakuma_core::permission::PermissionSet;
    ///
    /// let m = Macro {
    ///     id: uuid::Uuid::nil(),
    ///     name: "m".into(),
    ///     description: String::new(),
    ///     enabled: true,
    ///     category: None,
    ///     triggers: vec![TriggerConfig::Manual],
    ///     constraints: ConstraintExpr::Always,
    ///     actions: vec![ActionConfig::Delay { millis: 0 }],
    ///     workflow: None,
    ///     granted_permissions: PermissionSet::default(),
    ///     priority: 0,
    ///     concurrency: Default::default(),
    /// };
    /// assert!(matches!(m.root_workflow(), WorkflowNode::Seq(v) if v.len() == 1));
    /// ```
    pub fn root_workflow(&self) -> WorkflowNode {
        match &self.workflow {
            Some(w) => w.clone(),
            None => WorkflowNode::Seq(
                self.actions
                    .iter()
                    .cloned()
                    .map(WorkflowNode::Action)
                    .collect(),
            ),
        }
    }
}
