use volt::attenuation::{effective_permission, TrustLevel};
use volt::models::PermissionLevel;

const CASES: &[(TrustLevel, PermissionLevel, &str, PermissionLevel)] = &[
    (TrustLevel::Builtin, PermissionLevel::Allow, "bash", PermissionLevel::Allow),
    (TrustLevel::Builtin, PermissionLevel::Prompt, "bash", PermissionLevel::Prompt),
    (TrustLevel::Trusted, PermissionLevel::Allow, "write", PermissionLevel::Allow),
    (TrustLevel::Installed, PermissionLevel::Allow, "read", PermissionLevel::Allow),
    (TrustLevel::Installed, PermissionLevel::Allow, "grep", PermissionLevel::Allow),
    (TrustLevel::Installed, PermissionLevel::Allow, "bash", PermissionLevel::Blocked),
    (TrustLevel::Installed, PermissionLevel::Allow, "write", PermissionLevel::Blocked),
    (TrustLevel::Installed, PermissionLevel::Allow, "edit", PermissionLevel::Blocked),
    (TrustLevel::Installed, PermissionLevel::Allow, "browser_navigate", PermissionLevel::Allow),
    (TrustLevel::Installed, PermissionLevel::Prompt, "bash", PermissionLevel::Blocked),
];

#[test]
fn test_attenuation_table() {
    for (trust, declared, tool, expected) in CASES {
        let got = effective_permission(*trust, *declared, tool);
        assert_eq!(
            got, *expected,
            "attenuation({trust:?}, {declared:?}, {tool:?}) = {got:?}, want {expected:?}"
        );
    }
}

#[test]
fn test_installed_readonly_declared() {
    // ReadOnly-declared tools are still allowed; bash is blocked at the Installed trust ceiling.
    assert_eq!(
        effective_permission(TrustLevel::Installed, PermissionLevel::ReadOnly, "read"),
        PermissionLevel::Allow
    );
    assert_eq!(
        effective_permission(TrustLevel::Installed, PermissionLevel::ReadOnly, "bash"),
        PermissionLevel::Blocked
    );
}
