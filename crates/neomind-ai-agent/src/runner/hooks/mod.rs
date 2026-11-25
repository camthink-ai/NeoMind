//! Pre-dispatch tool hooks.

use crate::model::ToolPolicy;
use crate::ports::{CapabilityClass, OperationCapability, ToolInvocation};

pub trait ToolCallHook: Send + Sync {
    fn before_dispatch(
        &self,
        call: &ToolInvocation,
        capability: &OperationCapability,
        dry_run: bool,
        tool_policy: &ToolPolicy,
    ) -> Result<(), String>;
}

pub struct AllowlistHook;

impl ToolCallHook for AllowlistHook {
    fn before_dispatch(
        &self,
        call: &ToolInvocation,
        _capability: &OperationCapability,
        _dry_run: bool,
        tool_policy: &ToolPolicy,
    ) -> Result<(), String> {
        match tool_policy {
            ToolPolicy::Disabled => Err("tools disabled".into()),
            ToolPolicy::Unrestricted => Ok(()),
            ToolPolicy::Scoped { allowed_tools, .. } => {
                if allowed_tools.is_empty() || allowed_tools.iter().any(|t| t == &call.name) {
                    Ok(())
                } else {
                    Err(format!("{} not in allowlist", call.name))
                }
            }
        }
    }
}

pub struct SafetyPolicyHook;

impl ToolCallHook for SafetyPolicyHook {
    fn before_dispatch(
        &self,
        call: &ToolInvocation,
        capability: &OperationCapability,
        dry_run: bool,
        _tool_policy: &ToolPolicy,
    ) -> Result<(), String> {
        match capability.class {
            CapabilityClass::Unknown => Err(format!("unknown capability: {}", call.name)),
            CapabilityClass::SideEffect if dry_run => {
                Err(format!("dry-run rejects side effect: {}", call.name))
            }
            CapabilityClass::SideEffect | CapabilityClass::ReadOnly => Ok(()),
        }
    }
}

pub fn default_hooks() -> Vec<Box<dyn ToolCallHook>> {
    vec![Box::new(AllowlistHook), Box::new(SafetyPolicyHook)]
}
