use crate::domain::{ActionConfig, OnNoBackground, TargetSelector, WorkflowNode};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Restricts a file permission to a specific path, a directory prefix, or any path.
///
/// # Examples
///
/// ```rust
/// use korkuma_core::permission::PathScope;
/// use std::path::PathBuf;
///
/// let exact  = PathScope::Exact(PathBuf::from("/home/user/notes.txt"));
/// let prefix = PathScope::Prefix(PathBuf::from("/home/user/"));
/// let any    = PathScope::Any;
/// assert!(matches!(any, PathScope::Any));
/// ```
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum PathScope {
    Exact(PathBuf),
    Prefix(PathBuf),
    Any,
}

/// A single capability that a macro must declare to perform a sensitive operation.
///
/// Macros declare required permissions at save time via
/// [`domain::Macro::granted_permissions`]. Actions verify authorization at runtime
/// via [`PermissionGrant::allows`].
///
/// # Examples
///
/// ```rust
/// use korkuma_core::permission::{Permission, PathScope};
/// use std::path::PathBuf;
///
/// let net   = Permission::Network;
/// let read  = Permission::FileRead { scope: PathScope::Any };
/// let write = Permission::FileWrite { scope: PathScope::Prefix(PathBuf::from("/tmp/")) };
///
/// assert_ne!(net, read);
/// assert!(matches!(write, Permission::FileWrite { .. }));
/// ```
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind")]
pub enum Permission {
    InputSimulation,
    FileRead { scope: PathScope },
    FileWrite { scope: PathScope },
    Network,
    RunCommand,
    ScriptExecution,
    ClipboardRead,
    ClipboardWrite,
    /// Allowed to invoke / click / type into a background window via UIA or PostMessage.
    WindowInteraction,
    /// Allowed to execute operations on a browser tab via CDP or WebExtension.
    BrowserControl,
    /// Allowed to bring a window to the foreground (required for Degrade fallback).
    ForegroundTakeover,
}

impl Permission {
    /// Returns a short human-readable label for this permission, including the
    /// refined path scope for file permissions.
    ///
    /// Used by the permission approval dialog and the per-macro permission
    /// management page so users can read exactly what a macro is authorized to do.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use korkuma_core::permission::{Permission, PathScope};
    /// use std::path::PathBuf;
    ///
    /// assert_eq!(Permission::Network.describe(), "Network access");
    /// assert_eq!(
    ///     Permission::FileWrite { scope: PathScope::Prefix(PathBuf::from("/tmp")) }.describe(),
    ///     "File write (under /tmp)",
    /// );
    /// ```
    pub fn describe(&self) -> String {
        match self {
            Permission::InputSimulation => "Input simulation".to_string(),
            Permission::FileRead { scope } => format!("File read ({})", scope_label(scope)),
            Permission::FileWrite { scope } => format!("File write ({})", scope_label(scope)),
            Permission::Network => "Network access".to_string(),
            Permission::RunCommand => "Run command".to_string(),
            Permission::ScriptExecution => "Script execution".to_string(),
            Permission::ClipboardRead => "Clipboard read".to_string(),
            Permission::ClipboardWrite => "Clipboard write".to_string(),
            Permission::WindowInteraction => "Background window interaction".to_string(),
            Permission::BrowserControl => "Browser control".to_string(),
            Permission::ForegroundTakeover => "Foreground takeover".to_string(),
        }
    }
}

/// Renders a [`PathScope`] as a short suffix for [`Permission::describe`].
fn scope_label(scope: &PathScope) -> String {
    match scope {
        PathScope::Exact(p) => format!("exactly {}", p.display()),
        PathScope::Prefix(p) => format!("under {}", p.display()),
        PathScope::Any => "any path".to_string(),
    }
}

/// The set of permissions a [`domain::Macro`] declares it needs.
///
/// Stored inside the macro definition and serialized with it. Converted to a
/// [`PermissionGrant`] at dispatch time via [`PermissionGrant::from_set`].
///
/// # Examples
///
/// ```rust
/// use korkuma_core::permission::{Permission, PermissionSet};
///
/// let set = PermissionSet(vec![Permission::Network, Permission::RunCommand]);
/// assert_eq!(set.0.len(), 2);
///
/// let empty = PermissionSet::default();
/// assert!(empty.0.is_empty());
/// ```
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PermissionSet(pub Vec<Permission>);

/// Runtime authorization structure built from a macro's `granted_permissions` at execution time.
#[derive(Clone, Debug)]
pub struct PermissionGrant {
    granted: Vec<Permission>,
}

impl PermissionGrant {
    /// Creates a grant from an explicit list of permissions.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use korkuma_core::permission::{Permission, PermissionGrant};
    ///
    /// let grant = PermissionGrant::new(vec![Permission::Network]);
    /// assert!(grant.allows(&Permission::Network));
    /// ```
    pub fn new(permissions: Vec<Permission>) -> Self {
        Self {
            granted: permissions,
        }
    }

    /// Creates a grant from a [`PermissionSet`] stored in a macro definition.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use korkuma_core::permission::{Permission, PermissionSet, PermissionGrant};
    ///
    /// let set = PermissionSet(vec![Permission::RunCommand]);
    /// let grant = PermissionGrant::from_set(&set);
    /// assert!(grant.allows(&Permission::RunCommand));
    /// ```
    pub fn from_set(set: &PermissionSet) -> Self {
        Self::new(set.0.clone())
    }

    /// Returns `true` if `permission` is present in this grant.
    ///
    /// Uses `PartialEq` on [`Permission`], so the scope fields of `FileRead`/`FileWrite`
    /// must match exactly — there is no partial scope matching in V1.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use korkuma_core::permission::{Permission, PermissionGrant};
    ///
    /// let grant = PermissionGrant::new(vec![Permission::Network]);
    /// assert!( grant.allows(&Permission::Network));
    /// assert!(!grant.allows(&Permission::RunCommand));
    /// ```
    pub fn allows(&self, permission: &Permission) -> bool {
        self.granted.contains(permission)
    }
}

/// Statically aggregates the [`PermissionSet`] required by a list of action configs.
///
/// Inspects each [`ActionConfig`] variant and collects the union of required
/// permissions without instantiating the actions. Used at macro-save time to
/// populate [`domain::Macro::granted_permissions`].
///
/// # Examples
///
/// ```rust
/// use korkuma_core::domain::ActionConfig;
/// use korkuma_core::permission::{Permission, aggregate_from_configs};
///
/// let actions = vec![
///     ActionConfig::RunCommand { program: "echo".to_string(), args: vec![], capture: false },
///     ActionConfig::Notify { title: "T".to_string(), body: "B".to_string() },
/// ];
/// let set = aggregate_from_configs(&actions);
/// assert_eq!(set.0.len(), 1);
/// assert!(set.0.contains(&Permission::RunCommand));
/// ```
pub fn aggregate_from_configs(actions: &[ActionConfig]) -> PermissionSet {
    let mut perms: Vec<Permission> = Vec::new();
    for action in actions {
        add_action_perms(action, &mut perms);
    }
    PermissionSet(perms)
}

/// Statically aggregates the [`PermissionSet`] required by an entire workflow tree.
///
/// Walks every control-flow branch of the [`WorkflowNode`] (`Seq`, `Parallel`,
/// `If`/`then`/`otherwise`, `While`/`ForEach`/`Retry`/`Timeout` bodies) and unions
/// the permissions of every reachable [`ActionConfig`]. This is the V2-correct
/// counterpart of [`aggregate_from_configs`], which only inspects the flat
/// `Macro::actions` list and misses actions nested inside a workflow tree.
///
/// # Examples
///
/// ```rust
/// use korkuma_core::domain::{ActionConfig, ScriptLang, WorkflowNode};
/// use korkuma_core::permission::{Permission, aggregate_from_workflow};
///
/// let wf = WorkflowNode::Seq(vec![
///     WorkflowNode::Action(ActionConfig::Notify { title: "t".into(), body: "b".into() }),
///     WorkflowNode::Action(ActionConfig::RunScript {
///         lang: ScriptLang::Rhai,
///         source: "1".into(),
///     }),
/// ]);
/// let set = aggregate_from_workflow(&wf);
/// assert!(set.0.contains(&Permission::ScriptExecution));
/// assert_eq!(set.0.len(), 1);
/// ```
pub fn aggregate_from_workflow(root: &WorkflowNode) -> PermissionSet {
    let mut perms: Vec<Permission> = Vec::new();
    collect_workflow_perms(root, &mut perms);
    PermissionSet(perms)
}

/// Depth-first walk that appends every reachable action's permissions into `perms`.
fn collect_workflow_perms(node: &WorkflowNode, perms: &mut Vec<Permission>) {
    match node {
        WorkflowNode::Action(a) => add_action_perms(a, perms),
        WorkflowNode::Seq(children) | WorkflowNode::Parallel(children) => {
            children.iter().for_each(|c| collect_workflow_perms(c, perms));
        }
        WorkflowNode::If { then, otherwise, .. } => {
            collect_workflow_perms(then, perms);
            if let Some(otherwise) = otherwise {
                collect_workflow_perms(otherwise, perms);
            }
        }
        WorkflowNode::While { body, .. }
        | WorkflowNode::ForEach { body, .. }
        | WorkflowNode::Retry { body, .. }
        | WorkflowNode::Timeout { body, .. } => collect_workflow_perms(body, perms),
        WorkflowNode::Wait { .. } => {}
    }
}

/// Appends the permissions required by a single action config, de-duplicating
/// against entries already present in `perms`.
fn add_action_perms(action: &ActionConfig, perms: &mut Vec<Permission>) {
    let mut add = |p: Permission| {
        if !perms.contains(&p) {
            perms.push(p);
        }
    };
    match action {
        ActionConfig::RunCommand { .. } => add(Permission::RunCommand),
        ActionConfig::SimulateInput { .. } => add(Permission::InputSimulation),
        ActionConfig::RunScript { .. } => add(Permission::ScriptExecution),
        ActionConfig::HttpRequest { .. } => add(Permission::Network),
        ActionConfig::Interact { target, on_no_background, .. } => {
            match target {
                TargetSelector::BrowserTab { .. } => add(Permission::BrowserControl),
                _ => add(Permission::WindowInteraction),
            }
            if matches!(on_no_background, OnNoBackground::Degrade) {
                add(Permission::ForegroundTakeover);
            }
        }
        _ => {}
    }
}
