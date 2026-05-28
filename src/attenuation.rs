use crate::models::{PermissionLevel, ToolDefinition};
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustLevel {
    Builtin,
    Trusted,
    Installed,
}

/// Attenuates declared PermissionLevel based on trust tier.
pub fn effective_permission(
    trust: TrustLevel,
    declared: PermissionLevel,
    tool_name: &str,
) -> PermissionLevel {
    match trust {
        TrustLevel::Builtin => declared,
        TrustLevel::Trusted => declared,
        TrustLevel::Installed => {
            let read_only: HashSet<&str> = [
                "read",
                "glob",
                "grep",
                "web_fetch",
                "web_search",
                "browser_navigate",
                "browser_extract",
                "json_validate",
                "json_prettify",
                "json_query",
                "memory_read",
                "get_current_time",
                "convert_time",
                "sequential_thinking",
                "web_search",
                "you_research",
                "you_contents",
            ]
            .iter()
            .copied()
            .collect();
            if read_only.contains(tool_name) {
                PermissionLevel::Allow
            } else {
                PermissionLevel::Blocked
            }
        }
    }
}

pub struct AttenuatedTool {
    pub def: ToolDefinition,
    pub effective_perms: PermissionLevel,
    pub trust: TrustLevel,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_unmodified() {
        assert_eq!(
            effective_permission(TrustLevel::Builtin, PermissionLevel::Allow, "bash"),
            PermissionLevel::Allow
        );
    }

    #[test]
    fn test_installed_read_only_passes() {
        assert_eq!(
            effective_permission(TrustLevel::Installed, PermissionLevel::Allow, "read"),
            PermissionLevel::Allow
        );
    }

    #[test]
    fn test_installed_bash_blocked() {
        assert_eq!(
            effective_permission(TrustLevel::Installed, PermissionLevel::Allow, "bash"),
            PermissionLevel::Blocked
        );
    }
}
