use crate::builtins;
use crate::domain::{ActionConfig, ConstraintConfig, TriggerConfig};
use crate::error::RegistryError;
use crate::traits::{Action, Constraint, TriggerSpec};

type TriggerFn   = Box<dyn Fn(&TriggerConfig)    -> Option<Box<dyn TriggerSpec>> + Send + Sync>;
type ConstraintFn = Box<dyn Fn(&ConstraintConfig)  -> Option<Box<dyn Constraint>>  + Send + Sync>;
type ActionFn    = Box<dyn Fn(&ActionConfig)      -> Option<Box<dyn Action>>      + Send + Sync>;

/// Factory that converts serializable *Config values into runtime trait objects.
///
/// Built-in types are pre-registered in `with_builtins()`. Platform-specific or
/// user-defined providers are added via `register_*` — typically called from `app`
/// at startup.
pub struct Registry {
    trigger_fns:    Vec<TriggerFn>,
    constraint_fns: Vec<ConstraintFn>,
    action_fns:     Vec<ActionFn>,
}

impl Registry {
    /// Creates an empty registry with no factories registered.
    ///
    /// Use [`with_builtins`](Self::with_builtins) to include all platform-independent
    /// built-in implementations.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use koakuma_core::registry::Registry;
    ///
    /// let reg = Registry::new();
    /// // No factories yet; build_* calls will return RegistryError::UnknownProvider.
    /// ```
    pub fn new() -> Self {
        Self {
            trigger_fns:    Vec::new(),
            constraint_fns: Vec::new(),
            action_fns:     Vec::new(),
        }
    }

    /// Pre-loads all built-in, platform-independent implementations.
    pub fn with_builtins() -> Self {
        let mut reg = Self::new();

        reg.register_trigger(builtins::build_trigger);
        reg.register_constraint(builtins::build_time_range);
        reg.register_constraint(builtins::build_var_compare);
        reg.register_action(builtins::build_set_variable);
        reg.register_action(builtins::build_delay);

        reg
    }

    /// Appends a trigger factory function to the registry.
    ///
    /// The factory receives a `&TriggerConfig` and returns `Some(spec)` if it handles
    /// that variant, or `None` to pass to the next factory.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use koakuma_core::registry::Registry;
    ///
    /// let mut reg = Registry::new();
    /// reg.register_trigger(|_c| None); // a no-op factory
    /// ```
    pub fn register_trigger<F>(&mut self, f: F)
    where
        F: Fn(&TriggerConfig) -> Option<Box<dyn TriggerSpec>> + Send + Sync + 'static,
    {
        self.trigger_fns.push(Box::new(f));
    }

    /// Appends a constraint factory function to the registry.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use koakuma_core::registry::Registry;
    ///
    /// let mut reg = Registry::new();
    /// reg.register_constraint(|_c| None);
    /// ```
    pub fn register_constraint<F>(&mut self, f: F)
    where
        F: Fn(&ConstraintConfig) -> Option<Box<dyn Constraint>> + Send + Sync + 'static,
    {
        self.constraint_fns.push(Box::new(f));
    }

    /// Appends an action factory function to the registry.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use koakuma_core::registry::Registry;
    ///
    /// let mut reg = Registry::new();
    /// reg.register_action(|_c| None);
    /// ```
    pub fn register_action<F>(&mut self, f: F)
    where
        F: Fn(&ActionConfig) -> Option<Box<dyn Action>> + Send + Sync + 'static,
    {
        self.action_fns.push(Box::new(f));
    }

    /// Instantiates a [`TriggerSpec`](crate::traits::TriggerSpec) from a config value.
    ///
    /// Iterates registered factories in insertion order; returns the first `Some` result.
    ///
    /// # Errors
    ///
    /// Returns [`RegistryError::UnknownProvider`] if no factory handles `c`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use koakuma_core::registry::Registry;
    /// use koakuma_core::domain::TriggerConfig;
    ///
    /// let reg = Registry::with_builtins();
    /// let spec = reg.build_trigger(&TriggerConfig::Manual);
    /// assert!(spec.is_ok());
    /// ```
    pub fn build_trigger(&self, c: &TriggerConfig) -> Result<Box<dyn TriggerSpec>, RegistryError> {
        self.trigger_fns
            .iter()
            .find_map(|f| f(c))
            .ok_or_else(|| RegistryError::UnknownProvider(format!("{c:?}")))
    }

    /// Instantiates a [`Constraint`](crate::traits::Constraint) from a config value.
    ///
    /// # Errors
    ///
    /// Returns [`RegistryError::UnknownProvider`] if no factory handles `c`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use koakuma_core::registry::Registry;
    /// use koakuma_core::domain::ConstraintConfig;
    ///
    /// let reg = Registry::with_builtins();
    /// let c = ConstraintConfig::TimeRange { from: "09:00".to_string(), to: "17:00".to_string() };
    /// assert!(reg.build_constraint(&c).is_ok());
    /// ```
    pub fn build_constraint(&self, c: &ConstraintConfig) -> Result<Box<dyn Constraint>, RegistryError> {
        self.constraint_fns
            .iter()
            .find_map(|f| f(c))
            .ok_or_else(|| RegistryError::UnknownProvider(format!("{c:?}")))
    }

    /// Instantiates an [`Action`](crate::traits::Action) from a config value.
    ///
    /// # Errors
    ///
    /// Returns [`RegistryError::UnknownProvider`] if no factory handles `c`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use koakuma_core::registry::Registry;
    /// use koakuma_core::domain::ActionConfig;
    ///
    /// let reg = Registry::with_builtins();
    /// let action = reg.build_action(&ActionConfig::Delay { millis: 0 });
    /// assert!(action.is_ok());
    /// ```
    pub fn build_action(&self, c: &ActionConfig) -> Result<Box<dyn Action>, RegistryError> {
        self.action_fns
            .iter()
            .find_map(|f| f(c))
            .ok_or_else(|| RegistryError::UnknownProvider(format!("{c:?}")))
    }
}

impl Default for Registry {
    fn default() -> Self { Self::new() }
}
