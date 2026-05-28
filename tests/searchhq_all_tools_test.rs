use std::sync::Arc;
use volt::capability::{CapabilityManager, CapabilityScope};
use volt::tools::ToolRegistry;

fn get_token() -> String {
    if let Ok(t) = std::env::var("SEARCHHQ_API_TOKEN") {
        return t;
    }
    let home = std::env::var("USERPROFILE").unwrap_or_else(|_| ".".into());
    let path = format!("{}/.searchhq/session.json", home);
    if let Ok(content) = std::fs::read_to_string(&path) {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(t) = val.get("access_token").and_then(|v| v.as_str()) {
                if !t.is_empty() {
                    return t.to_string();
                }
            }
        }
    }
    String::new()
}

#[tokio::test]
async fn test_all_searchhq_tools() {
    let token = get_token();
    assert!(!token.is_empty(), "No SearchHQ token found");

    let registry = ToolRegistry::new();
    let count = volt::tools::searchhq::register_searchhq_tools(&registry, &token)
        .await
        .expect("Failed to register SearchHQ tools");

    println!("Registered {} tools\n", count);

    let cap_mgr = {
        let mgr = Arc::new(CapabilityManager::new());
        for scope in &[
            CapabilityScope::FsRead,
            CapabilityScope::FsWrite,
            CapabilityScope::System,
            CapabilityScope::Network,
            CapabilityScope::Database,
            CapabilityScope::Memory,
        ] {
            mgr.issue(scope.clone(), 500, chrono::Duration::hours(1)).await;
        }
        mgr
    };

    let defs = registry.get_definitions().await;
    let mut passes = 0;
    let mut fails = 0;

    for def in &defs {
        if def.category != "searchhq-mcp" {
            continue;
        }

        // Build minimal valid args for each tool
        let args = minimal_args(&def.name);
        println!(
            "  [{:30}] calling with args: {}",
            def.name,
            serde_json::to_string(&args).unwrap_or_default()
        );

        let result = registry.execute_gated(&def.name, &args, &cap_mgr).await;
        match result {
            Ok(r) => {
                if r.success {
                    passes += 1;
                    println!("  ✓ PASS ({:}ms)", r.duration_ms);
                } else {
                    fails += 1;
                    println!(
                        "  ✗ FAIL (tool returned error): {}",
                        r.error.unwrap_or_default()
                    );
                }
            }
            Err(e) => {
                fails += 1;
                println!("  ✗ ERROR: {}", e);
            }
        }
    }

    println!(
        "\nResults: {}/{} passed, {}/{} failed",
        passes,
        passes + fails,
        fails,
        passes + fails
    );
}

fn minimal_args(tool: &str) -> serde_json::Value {
    match tool {
        "searchhq_list_threads" => serde_json::json!({"limit": 5}),
        "searchhq_get_thread" => {
            serde_json::json!({"thread_id": "00000000-0000-0000-0000-000000000000"})
        }
        "searchhq_search_threads" => serde_json::json!({"query": "test"}),
        "searchhq_create_thread" => serde_json::json!({"name": "Volt MCP Test Thread"}),
        "searchhq_run_search" => {
            serde_json::json!({"thread_id": "00000000-0000-0000-0000-000000000000", "query": "test query"})
        }
        "searchhq_chat" => {
            serde_json::json!({"thread_id": "00000000-0000-0000-0000-000000000000", "message": "hello"})
        }
        "searchhq_ask_library" => serde_json::json!({"query": "test query"}),
        "searchhq_save_clip" => {
            serde_json::json!({"thread_id": "00000000-0000-0000-0000-000000000000", "type": "text", "title": "test clip", "content": "test content"})
        }
        "searchhq_add_comment" => {
            serde_json::json!({"thread_id": "00000000-0000-0000-0000-000000000000", "body": "test comment"})
        }
        "searchhq_list_feeds" => {
            serde_json::json!({"thread_id": "00000000-0000-0000-0000-000000000000"})
        }
        "searchhq_add_feed" => {
            serde_json::json!({"thread_id": "00000000-0000-0000-0000-000000000000", "feed_url": "https://example.com/rss"})
        }
        "searchhq_list_chats" => {
            serde_json::json!({"thread_id": "00000000-0000-0000-0000-000000000000"})
        }
        "searchhq_get_chat" => {
            serde_json::json!({"chat_id": "00000000-0000-0000-0000-000000000000"})
        }
        "searchhq_rate_response" => serde_json::json!({"interaction_type": "search", "score": 1}),
        "searchhq_get_feed_articles" => {
            serde_json::json!({"feed_id": "00000000-0000-0000-0000-000000000000"})
        }
        "searchhq_refresh_feed" => {
            serde_json::json!({"thread_id": "00000000-0000-0000-0000-000000000000"})
        }
        "searchhq_clip_article" => {
            serde_json::json!({"thread_id": "00000000-0000-0000-0000-000000000000", "url": "https://example.com"})
        }
        "searchhq_run_agent" => {
            serde_json::json!({"thread_id": "00000000-0000-0000-0000-000000000000", "topic": "test topic"})
        }
        "searchhq_generate_sandbox" => {
            serde_json::json!({"thread_id": "00000000-0000-0000-0000-000000000000", "prompt": "simple hello world page"})
        }
        _ => serde_json::json!({}),
    }
}
