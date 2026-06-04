//! ParamSchema — schema descriptor for macro config form fields.
//!
//! Used by the visual editor (§16.1) to render per-variant forms without
//! hard-coding Slint components for each config type. Plugin providers
//! (§15.2) will supply their own schema at registration time.

/// The data type of a single form field in a [`ParamSchema`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParamType {
    /// Single-line string input.
    Str,
    /// Integer number input.
    Int,
    /// Boolean toggle (checkbox).
    Bool,
    /// Fixed-option dropdown.
    Enum(Vec<String>),
    /// File-system path (browse button optional).
    Path,
    /// Sensitive field — display is masked.
    Secret,
    /// Multi-line text area.
    Multiline,
    /// Duration in milliseconds (numeric input with unit hint).
    Duration,
    /// Raw JSON (editor fallback for complex / nested values).
    Json,
}

/// Descriptor for one field in a schema-driven form.
///
/// # Examples
///
/// ```rust
/// use koakuma_core::schema::{ParamField, ParamType};
///
/// let f = ParamField::required("title", "Title", ParamType::Str);
/// assert_eq!(f.name, "title");
/// assert!(f.required);
/// ```
#[derive(Debug, Clone)]
pub struct ParamField {
    /// Serialized field name (matches the JSON / serde key).
    pub name: String,
    /// Human-readable label shown next to the input.
    pub label: String,
    /// The data type / widget type for this field.
    pub ty: ParamType,
    /// Whether the field must be filled before saving.
    pub required: bool,
}

impl ParamField {
    /// Constructs a required field.
    pub fn required(name: &str, label: &str, ty: ParamType) -> Self {
        Self { name: name.into(), label: label.into(), ty, required: true }
    }

    /// Constructs an optional field.
    pub fn optional(name: &str, label: &str, ty: ParamType) -> Self {
        Self { name: name.into(), label: label.into(), ty, required: false }
    }
}

/// Full schema for one config variant — describes which form fields to render.
///
/// Produced by the `param_schema()` methods on domain config types, or supplied
/// by a plugin's `plugin.toml` manifest (V3 §15.2). The visual editor uses this
/// to render a typed form for the selected node without per-variant Slint code.
///
/// # Examples
///
/// ```rust
/// use koakuma_core::schema::{ParamSchema, ParamField, ParamType};
///
/// let schema = ParamSchema::new(
///     "Notify",
///     "Send Notification",
///     vec![
///         ParamField::required("title", "Title", ParamType::Str),
///         ParamField::required("body", "Body", ParamType::Multiline),
///     ],
/// );
/// assert_eq!(schema.type_name, "Notify");
/// assert_eq!(schema.fields.len(), 2);
/// ```
#[derive(Debug, Clone)]
pub struct ParamSchema {
    /// The serde `type` tag value (e.g. `"Notify"`, `"ActiveWindow"`).
    pub type_name: String,
    /// Human-readable variant name shown in type drop-downs.
    pub display_name: String,
    /// Ordered list of fields to render.
    pub fields: Vec<ParamField>,
}

impl ParamSchema {
    /// Constructs a new schema.
    pub fn new(type_name: &str, display_name: &str, fields: Vec<ParamField>) -> Self {
        Self {
            type_name: type_name.into(),
            display_name: display_name.into(),
            fields,
        }
    }
}
