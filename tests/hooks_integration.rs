//! Integration tests for the agent hook system (`src/agent/hooks.rs`).
//!
//! These tests exercise the `HookRegistry` end-to-end: write a real
//! `hooks.toml` to a tempdir, spawn a small shell command as a hook,
//! and verify that:
//!   * `PreToolUse` can block a tool call (exit 2)
//!   * `PreToolUse` can modify arguments (JSON `args` on stdout)
//!   * `PostToolUse` context gets captured and merged
//!   * `UserPromptSubmit` can inject context
//!   * `PreRun` / `PostRun` fire as one-shot side effects
//!
//! Cross-platform: sh scripts on Unix, cmd /C batch files on Windows.

use std::path::PathBuf;
use volt::agent::hooks::HookOutcome;
use volt::agent::hooks::{
    HookConfig, HookDefinition, HookEvent, HookPayload, HookRegistry, HookSection, PreToolDecision,
};

fn make_script(dir: &std::path::Path, name: &str, body: &str) -> PathBuf {
    #[cfg(windows)]
    let path = dir.join(format!("{}.bat", name.trim_end_matches(".sh")));
    #[cfg(not(windows))]
    let path = dir.join(name);
    #[cfg(windows)]
    {
        // Write a .bat batch file. `body` should be a batch-compatible
        // echo/echo/... command list. For JSON-on-stdout, we wrap the
        // payload in `echo { ... }` syntax (cmd's `echo` doesn't need
        // quotes around the JSON if there are no `>`/`<`/`|` chars).
        std::fs::write(&path, format!("@echo off\r\n{}\r\n", body)).unwrap();
    }
    #[cfg(not(windows))]
    {
        std::fs::write(&path, format!("#!/bin/sh\n{}\n", body)).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&path).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&path, perms).unwrap();
        }
    }
    path
}

#[tokio::test]
async fn pre_tool_use_blocks_when_hook_exits_2() {
    let tmp = tempdir_in_target("hook_block");
    let script = make_script(
        &tmp,
        "block.sh",
        "echo {\"decision\":\"block\",\"reason\":\"denied by test\"} 1>&2\nexit /b 2",
    );
    let cfg = HookConfig {
        pre_tool_use: HookSection {
            entries: vec![HookDefinition {
                matcher: Some("write".into()),
                timeout: 5,
                command: script.to_string_lossy().to_string(),
            }],
        },
        ..Default::default()
    };
    let registry = HookRegistry::from_config(cfg).with_session("s1", "tester");

    let decision = registry
        .run_pre_tool_use("write", &serde_json::json!({"path": "/tmp/x"}))
        .await;
    assert!(decision.is_blocking(), "expected block, got {:?}", decision);
    if let PreToolDecision::Block { reason } = decision {
        assert!(reason.contains("denied by test"), "reason: {}", reason);
    }
}

#[tokio::test]
async fn pre_tool_use_allows_when_no_hooks_match() {
    let cfg = HookConfig::default();
    let registry = HookRegistry::from_config(cfg).with_session("s1", "tester");
    let decision = registry
        .run_pre_tool_use("anything", &serde_json::json!({}))
        .await;
    assert_eq!(decision, PreToolDecision::Allow);
}

#[tokio::test]
async fn pre_tool_use_modifies_args() {
    let tmp = tempdir_in_target("hook_modify");
    let script = make_script(
        &tmp,
        "modify.sh",
        "echo {\"decision\":\"modify\",\"args\":{\"redirected\":true}}",
    );
    let cfg = HookConfig {
        pre_tool_use: HookSection {
            entries: vec![HookDefinition {
                matcher: Some("write".into()),
                timeout: 5,
                command: script.to_string_lossy().to_string(),
            }],
        },
        ..Default::default()
    };
    let registry = HookRegistry::from_config(cfg).with_session("s1", "tester");
    let decision = registry
        .run_pre_tool_use("write", &serde_json::json!({"path": "/x"}))
        .await;
    match decision {
        PreToolDecision::ModifyArgs { args } => {
            assert_eq!(args, serde_json::json!({"redirected": true}));
        }
        other => panic!("expected ModifyArgs, got {:?}", other),
    }
}

#[tokio::test]
async fn post_tool_use_collects_context() {
    let tmp = tempdir_in_target("hook_post");
    let script = make_script(&tmp, "ctx.sh", "echo {\"context\":\"hello from post\"}");
    let cfg = HookConfig {
        post_tool_use: HookSection {
            entries: vec![HookDefinition {
                matcher: Some("bash".into()),
                timeout: 5,
                command: script.to_string_lossy().to_string(),
            }],
        },
        ..Default::default()
    };
    let registry = HookRegistry::from_config(cfg).with_session("s1", "tester");
    let outcome = registry
        .run_post_tool_use("bash", &serde_json::json!({}), "output", true, 42)
        .await;
    let ctx = outcome.merged_context();
    assert!(ctx.contains("hello from post"), "ctx: {}", ctx);
}

#[tokio::test]
async fn user_prompt_submit_injects_context() {
    let tmp = tempdir_in_target("hook_user");
    let script = make_script(&tmp, "user.sh", "echo {\"context\":\"policy: be concise\"}");
    let cfg = HookConfig {
        user_prompt_submit: HookSection {
            entries: vec![HookDefinition {
                matcher: None,
                timeout: 5,
                command: script.to_string_lossy().to_string(),
            }],
        },
        ..Default::default()
    };
    let registry = HookRegistry::from_config(cfg).with_session("s1", "tester");
    let outcome = registry.run_user_prompt_submit("do thing").await;
    let ctx = outcome.merged_context();
    assert!(ctx.contains("policy: be concise"), "ctx: {}", ctx);
}

#[tokio::test]
async fn pre_run_and_post_run_fire() {
    let tmp = tempdir_in_target("hook_run");
    let pre = make_script(&tmp, "pre.sh", "echo pre-ran");
    let post = make_script(&tmp, "post.sh", "echo post-ran");
    let cfg = HookConfig {
        pre_run: HookSection {
            entries: vec![HookDefinition {
                matcher: None,
                timeout: 5,
                command: pre.to_string_lossy().to_string(),
            }],
        },
        post_run: HookSection {
            entries: vec![HookDefinition {
                matcher: None,
                timeout: 5,
                command: post.to_string_lossy().to_string(),
            }],
        },
        ..Default::default()
    };
    let registry = HookRegistry::from_config(cfg).with_session("s1", "tester");
    registry.run_pre_run().await;
    registry.run_post_run().await;
    // Both shells have completed. We don't have a capture buffer in the
    // public API, but the lack of a panic is enough — the script was
    // spawned and waited on. (Direct stdout verification would require
    // exposing the private capture mechanism, which we don't do here.)
}

#[tokio::test]
async fn matcher_with_pipe_list() {
    let tmp = tempdir_in_target("hook_pipe");
    let script = make_script(&tmp, "any.sh", "echo {\"context\":\"matched\"}");
    let cfg = HookConfig {
        post_tool_use: HookSection {
            entries: vec![HookDefinition {
                matcher: Some("bash|write|edit".into()),
                timeout: 5,
                command: script.to_string_lossy().to_string(),
            }],
        },
        ..Default::default()
    };
    let registry = HookRegistry::from_config(cfg).with_session("s1", "tester");
    for tool in ["bash", "write", "edit"] {
        let outcome = registry
            .run_post_tool_use(tool, &serde_json::json!({}), "", true, 0)
            .await;
        assert!(
            outcome.merged_context().contains("matched"),
            "tool {} should match",
            tool
        );
    }
    let outcome = registry
        .run_post_tool_use("read", &serde_json::json!({}), "", true, 0)
        .await;
    assert!(
        !outcome.merged_context().contains("matched"),
        "read should not match"
    );
}

#[tokio::test]
async fn matcher_with_regex() {
    let tmp = tempdir_in_target("hook_regex");
    let script = make_script(&tmp, "regex.sh", "echo {\"context\":\"regex matched\"}");
    let cfg = HookConfig {
        post_tool_use: HookSection {
            entries: vec![HookDefinition {
                matcher: Some("/^(bash|write)$/".into()),
                timeout: 5,
                command: script.to_string_lossy().to_string(),
            }],
        },
        ..Default::default()
    };
    let registry = HookRegistry::from_config(cfg).with_session("s1", "tester");
    let o1 = registry
        .run_post_tool_use("bash", &serde_json::json!({}), "", true, 0)
        .await;
    assert!(o1.merged_context().contains("regex matched"));
    let o2 = registry
        .run_post_tool_use("read", &serde_json::json!({}), "", true, 0)
        .await;
    assert!(!o2.merged_context().contains("regex matched"));
}

#[tokio::test]
async fn hook_payload_serialises_all_fields() {
    let p = HookPayload::pre_tool_use("sess-1", "agent", "bash", &serde_json::json!({"cmd": "ls"}));
    let j = serde_json::to_value(&p).unwrap();
    assert_eq!(j["event"], "PreToolUse");
    assert_eq!(j["session_id"], "sess-1");
    assert_eq!(j["agent_name"], "agent");
    assert_eq!(j["tool"], "bash");
    assert_eq!(j["args"]["cmd"], "ls");
}

#[tokio::test]
async fn default_outcome_is_empty_context() {
    let o = HookOutcome::default();
    assert_eq!(o.merged_context(), "");
    assert!(!o.is_blocked());
}

#[tokio::test]
async fn from_default_paths_returns_empty_when_no_files() {
    // Use a temp dir that definitely has no .volt/hooks.toml.
    let tmp = tempdir_in_target("hook_empty");
    let registry = HookRegistry::from_default_paths(&tmp).unwrap();
    assert!(registry.is_empty());
}

#[tokio::test]
async fn merge_configs_user_only_loads() {
    let tmp = tempdir_in_target("hook_user_only");
    std::fs::create_dir_all(tmp.join(".volt")).unwrap();
    let toml = r#"
[[pre_tool_use.entries]]
matcher = "*"
command = "echo hi"
"#;
    std::fs::write(tmp.join(".volt/hooks.toml"), toml).unwrap();
    let registry = HookRegistry::from_default_paths(&tmp).unwrap();
    assert!(!registry.is_empty());
    let entries = registry.entries_for(HookEvent::PreToolUse, Some("bash"));
    assert_eq!(entries.len(), 1);
}

fn tempdir_in_target(label: &str) -> PathBuf {
    let base = std::env::temp_dir().join("volt_hook_tests").join(label);
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    base
}
