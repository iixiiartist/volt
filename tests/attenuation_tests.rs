use volt::attenuation::{effective_permission, TrustLevel};
use volt::models::PermissionLevel;

#[test]
fn test_builtin_respects_allow() {
    assert_eq!(
        effective_permission(TrustLevel::Builtin, PermissionLevel::Allow, "bash"),
        PermissionLevel::Allow
    );
}

#[test]
fn test_builtin_respects_prompt() {
    assert_eq!(
        effective_permission(TrustLevel::Builtin, PermissionLevel::Prompt, "bash"),
        PermissionLevel::Prompt
    );
}

#[test]
fn test_trusted_respects_declared() {
    assert_eq!(
        effective_permission(TrustLevel::Trusted, PermissionLevel::Allow, "write"),
        PermissionLevel::Allow
    );
}

#[test]
fn test_installed_read_only_read_allowed() {
    assert_eq!(
        effective_permission(TrustLevel::Installed, PermissionLevel::Allow, "read"),
        PermissionLevel::Allow
    );
}

#[test]
fn test_installed_read_only_grep_allowed() {
    assert_eq!(
        effective_permission(TrustLevel::Installed, PermissionLevel::Allow, "grep"),
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

#[test]
fn test_installed_write_blocked() {
    assert_eq!(
        effective_permission(TrustLevel::Installed, PermissionLevel::Allow, "write"),
        PermissionLevel::Blocked
    );
}

#[test]
fn test_installed_edit_blocked() {
    assert_eq!(
        effective_permission(TrustLevel::Installed, PermissionLevel::Allow, "edit"),
        PermissionLevel::Blocked
    );
}

#[test]
fn test_installed_browser_navigate_allowed() {
    assert_eq!(
        effective_permission(
            TrustLevel::Installed,
            PermissionLevel::Allow,
            "browser_navigate"
        ),
        PermissionLevel::Allow
    );
}

#[test]
fn test_installed_with_prompt_becomes_blocked() {
    // Even if declared as Prompt, installed maxes out at ReadOnly which blocks bash
    assert_eq!(
        effective_permission(TrustLevel::Installed, PermissionLevel::Prompt, "bash"),
        PermissionLevel::Blocked
    );
}

#[test]
fn test_installed_readonly_declared() {
    assert_eq!(
        effective_permission(TrustLevel::Installed, PermissionLevel::ReadOnly, "read"),
        PermissionLevel::Allow // ReadOnly tools still pass
    );
    assert_eq!(
        effective_permission(TrustLevel::Installed, PermissionLevel::ReadOnly, "bash"),
        PermissionLevel::Blocked
    );
}
