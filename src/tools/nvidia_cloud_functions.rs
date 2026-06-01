use crate::models::ToolResult;
use serde_json::json;
use std::sync::Arc;

const NVCF_BASE: &str = "https://api.nvcf.nvidia.com/v2/nvcf";

fn get_api_key() -> Option<String> {
    std::env::var("NVIDIA_API_KEY")
        .or_else(|_| std::env::var("NVCF_API_KEY"))
        .ok()
        .filter(|k| !k.is_empty())
}

fn authed_client() -> Option<reqwest::Client> {
    Some(crate::http_client().clone())
}

fn make_auth_req(client: &reqwest::Client, method: reqwest::Method, url: String) -> Option<reqwest::RequestBuilder> {
    let key = get_api_key()?;
    Some(client.request(method, &url).header("Authorization", format!("Bearer {}", key)))
}

pub fn register_nvidia_cloud_functions(registry: &Arc<crate::tools::ToolRegistry>) {
    let reg = registry.clone();

    // nvidia_list_functions
    let list_fn: crate::tools::ToolFn = Arc::new(move |_args: serde_json::Value| {
        let reg = reg.clone();
        Box::pin(async move {
            let client = match authed_client() {
                Some(c) => c,
                None => return ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some("NVIDIA_API_KEY or NVCF_API_KEY not set".into()),
                    duration_ms: 0,
                },
            };
            let req = match make_auth_req(&client, reqwest::Method::GET, format!("{}/functions", NVCF_BASE)) {
                Some(r) => r,
                None => return ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some("failed to create request".into()),
                    duration_ms: 0,
                },
            };
            match req.timeout(std::time::Duration::from_secs(30)).send().await {
                Ok(resp) => {
                    let status = resp.status();
                    match resp.json::<serde_json::Value>().await {
                        Ok(val) => ToolResult {
                            success: status.is_success(),
                            output: serde_json::to_string_pretty(&val).unwrap_or_default(),
                            error: if status.is_success() { None } else {
                                Some(val["error"].as_str().unwrap_or("unknown error").to_string())
                            },
                            duration_ms: 0,
                        },
                        Err(e) => ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some(format!("failed to parse response: {}", e)),
                            duration_ms: 0,
                        },
                    }
                }
                Err(e) => ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("request failed: {}", e)),
                    duration_ms: 0,
                },
            }
        }) as std::pin::Pin<Box<dyn std::future::Future<Output = ToolResult> + Send>>
    });
    registry.register_with_permission(
        "nvidia_list_functions",
        "List available NVIDIA Cloud Functions and their versions",
        json!({
            "type": "object",
            "properties": {}
        }),
        "builtin",
        list_fn,
        crate::models::PermissionLevel::Allow,
        crate::attenuation::TrustLevel::Builtin,
    );

    // nvidia_call_function
    let call_reg = registry.clone();
    let call_fn: crate::tools::ToolFn = Arc::new(move |args: serde_json::Value| {
        let _ = call_reg.clone();
        let function_id = args["function_id"].as_str().unwrap_or("").to_string();
        let version_id = args.get("version_id").and_then(|v| v.as_str()).unwrap_or("1").to_string();
        let payload = args.get("payload").cloned().unwrap_or(json!({}));
        Box::pin(async move {
            if function_id.is_empty() {
                return ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some("function_id is required".into()),
                    duration_ms: 0,
                };
            }
            let client = match authed_client() {
                Some(c) => c,
                None => return ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some("NVIDIA_API_KEY or NVCF_API_KEY not set".into()),
                    duration_ms: 0,
                },
            };
            let url = format!("{}/functions/{}/versions/{}", NVCF_BASE, function_id, version_id);
            let req = match make_auth_req(&client, reqwest::Method::POST, url) {
                Some(r) => r.json(&payload),
                None => return ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some("failed to create request".into()),
                    duration_ms: 0,
                },
            };
            match req.timeout(std::time::Duration::from_secs(120)).send().await {
                Ok(resp) => {
                    let status = resp.status();
                    // Handle async invocation (202)
                    if status == reqwest::StatusCode::ACCEPTED {
                        let async_resp: serde_json::Value = match resp.json().await {
                            Ok(v) => v,
                            Err(e) => return ToolResult {
                                success: false,
                                output: String::new(),
                                error: Some(format!("failed to parse async response: {}", e)),
                                duration_ms: 0,
                            },
                        };
                        let req_id = async_resp["request_id"].as_str().unwrap_or("").to_string();
                        if req_id.is_empty() {
                            return ToolResult {
                                success: true,
                                output: serde_json::to_string_pretty(&async_resp).unwrap_or_default(),
                                error: None,
                                duration_ms: 0,
                            };
                        }
                        // Poll for completion
                        let poll_url = format!("{}/functions/{}/versions/{}/status", NVCF_BASE, function_id, version_id);
                        for _ in 0..60 {
                            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                            let poll_req = match make_auth_req(&client, reqwest::Method::GET, poll_url.clone()) {
                                Some(r) => r,
                                None => break,
                            };
                            if let Ok(poll_resp) = poll_req.timeout(std::time::Duration::from_secs(30)).send().await {
                                if let Ok(poll_val) = poll_resp.json::<serde_json::Value>().await {
                                    let state = poll_val["status"].as_str().unwrap_or("unknown");
                                    match state {
                                        "completed" | "succeeded" => {
                                            return ToolResult {
                                                success: true,
                                                output: serde_json::to_string_pretty(&poll_val).unwrap_or_default(),
                                                error: None,
                                                duration_ms: 0,
                                            };
                                        }
                                        "failed" | "error" => {
                                            return ToolResult {
                                                success: false,
                                                output: String::new(),
                                                error: Some(poll_val["error"].as_str().unwrap_or("function invocation failed").to_string()),
                                                duration_ms: 0,
                                            };
                                        }
                                        _ => continue,
                                    }
                                }
                            }
                        }
                        return ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some("function invocation timed out".into()),
                            duration_ms: 0,
                        };
                    }
                    match resp.json::<serde_json::Value>().await {
                        Ok(val) => ToolResult {
                            success: status.is_success(),
                            output: serde_json::to_string_pretty(&val).unwrap_or_default(),
                            error: if status.is_success() { None } else {
                                Some(val["error"].as_str().unwrap_or("unknown error").to_string())
                            },
                            duration_ms: 0,
                        },
                        Err(e) => ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some(format!("failed to parse response: {}", e)),
                            duration_ms: 0,
                        },
                    }
                }
                Err(e) => ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("request failed: {}", e)),
                    duration_ms: 0,
                },
            }
        }) as std::pin::Pin<Box<dyn std::future::Future<Output = ToolResult> + Send>>
    });
    registry.register_with_permission(
        "nvidia_call_function",
        "Invoke an NVIDIA Cloud Function by ID with optional payload. Handles async polling automatically.",
        json!({
            "type": "object",
            "properties": {
                "function_id": { "type": "string", "description": "The NVIDIA Cloud Function ID to invoke" },
                "version_id": { "type": "string", "description": "Function version (default: 1)" },
                "payload": { "type": "object", "description": "Input payload for the function" }
            },
            "required": ["function_id"]
        }),
        "builtin",
        call_fn,
        crate::models::PermissionLevel::Allow,
        crate::attenuation::TrustLevel::Builtin,
    );

    // nvidia_deploy_function
    let deploy_reg = registry.clone();
    let deploy_fn: crate::tools::ToolFn = Arc::new(move |args: serde_json::Value| {
        let _ = deploy_reg.clone();
        let name = args["name"].as_str().unwrap_or("").to_string();
        let desc = args.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let tags = args.get("tags").cloned().unwrap_or(json!({}));
        Box::pin(async move {
            if name.is_empty() {
                return ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some("name is required".into()),
                    duration_ms: 0,
                };
            }
            let client = match authed_client() {
                Some(c) => c,
                None => return ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some("NVIDIA_API_KEY or NVCF_API_KEY not set".into()),
                    duration_ms: 0,
                },
            };
            let body = json!({
                "name": name,
                "description": desc,
                "tags": tags,
                "inference_uri": args.get("inference_uri").and_then(|v| v.as_str()).unwrap_or(""),
                "health_uri": args.get("health_uri").and_then(|v| v.as_str()).unwrap_or(""),
                "function_type": args.get("function_type").and_then(|v| v.as_str()).unwrap_or("custom"),
            });
            let req = match make_auth_req(&client, reqwest::Method::POST, format!("{}/functions", NVCF_BASE)) {
                Some(r) => r.json(&body),
                None => return ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some("failed to create request".into()),
                    duration_ms: 0,
                },
            };
            match req.timeout(std::time::Duration::from_secs(60)).send().await {
                Ok(resp) => {
                    let status = resp.status();
                    match resp.json::<serde_json::Value>().await {
                        Ok(val) => ToolResult {
                            success: status.is_success(),
                            output: serde_json::to_string_pretty(&val).unwrap_or_default(),
                            error: if status.is_success() { None } else {
                                Some(val["error"].as_str().unwrap_or("deployment failed").to_string())
                            },
                            duration_ms: 0,
                        },
                        Err(e) => ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some(format!("failed to parse response: {}", e)),
                            duration_ms: 0,
                        },
                    }
                }
                Err(e) => ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("request failed: {}", e)),
                    duration_ms: 0,
                },
            }
        }) as std::pin::Pin<Box<dyn std::future::Future<Output = ToolResult> + Send>>
    });
    registry.register_with_permission(
        "nvidia_deploy_function",
        "Deploy a new NVIDIA Cloud Function with a name, description, inference URI, and tags",
        json!({
            "type": "object",
            "properties": {
                "name": { "type": "string", "description": "Name of the function to deploy" },
                "description": { "type": "string", "description": "Description of the function" },
                "inference_uri": { "type": "string", "description": "Inference endpoint URI for the function" },
                "health_uri": { "type": "string", "description": "Health check URI" },
                "tags": { "type": "object", "description": "Key-value tags for the function" },
                "function_type": { "type": "string", "description": "Type of function (default: custom)" }
            },
            "required": ["name"]
        }),
        "builtin",
        deploy_fn,
        crate::models::PermissionLevel::Allow,
        crate::attenuation::TrustLevel::Builtin,
    );
}
