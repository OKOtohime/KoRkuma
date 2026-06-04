//! [`RunCommandAction`] — executes an external process.

use async_trait::async_trait;
use korkuma_core::context::ExecContext;
use korkuma_core::domain::ActionConfig;
use korkuma_core::error::ActionError;
use korkuma_core::permission::{Permission, PermissionSet};
use korkuma_core::traits::{Action, Outcome};

/// Executes an external command, optionally capturing its output.
///
/// Requires [`Permission::RunCommand`] to be granted on the macro.
///
/// **Config**: [`ActionConfig::RunCommand`]
///
/// When `capture` is `false` the child process is spawned and the action
/// returns immediately without waiting. When `capture` is `true` the action
/// waits for the process to finish, forwards stdout/stderr through
/// [`LogHandle`](korkuma_core::context::LogHandle), and returns
/// [`ActionError::Failed`] if the exit code is non-zero.
pub struct RunCommandAction {
    program: String,
    args: Vec<String>,
    capture: bool,
}

#[async_trait]
impl Action for RunCommandAction {
    fn id(&self) -> &'static str {
        "run_command"
    }

    fn required_permissions(&self) -> PermissionSet {
        PermissionSet(vec![Permission::RunCommand])
    }

    async fn execute(&self, ctx: &mut ExecContext) -> Result<Outcome, ActionError> {
        if !ctx.permissions.allows(&Permission::RunCommand) {
            return Err(ActionError::PermissionDenied("RunCommand".to_string()));
        }

        let mut cmd = std::process::Command::new(&self.program);
        cmd.args(&self.args);

        if self.capture {
            let output = cmd
                .output()
                .map_err(|e| ActionError::Failed(format!("spawn failed: {e}")))?;

            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);

            if !stdout.is_empty() {
                ctx.log.log("info", stdout.trim());
            }
            if !stderr.is_empty() {
                ctx.log.log("warn", stderr.trim());
            }

            if !output.status.success() {
                return Err(ActionError::Failed(format!(
                    "`{}` exited with code {:?}",
                    self.program,
                    output.status.code()
                )));
            }
        } else {
            cmd.spawn()
                .map_err(|e| ActionError::Failed(format!("spawn failed: {e}")))?;
        }

        Ok(Outcome::Continue)
    }
}

/// Factory: builds [`RunCommandAction`] from [`ActionConfig::RunCommand`].
pub fn build(c: &ActionConfig) -> Option<Box<dyn Action>> {
    if let ActionConfig::RunCommand {
        program,
        args,
        capture,
    } = c
    {
        Some(Box::new(RunCommandAction {
            program: program.clone(),
            args: args.clone(),
            capture: *capture,
        }))
    } else {
        None
    }
}
