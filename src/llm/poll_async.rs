use crate::models::LLMResponse;
use serde_json::Value;
use std::time::Duration;
use tracing;

/// Poll an async inference endpoint until it returns a completed, failed, or error status.
///
/// # Parameters
/// * `http` — The reqwest client to use for polling.
/// * `poll_url` — The full URL to poll (including any path segments).
/// * `max_polls` — Maximum number of polling attempts.
/// * `poll_interval` — Delay between each poll attempt.
/// * `timeout` — Per-request timeout for each poll HTTP call.
/// * `apply_auth` — Closure that applies auth headers to a RequestBuilder.
/// * `parse_response` — Closure that converts the final JSON response into an LLMResponse.
///
/// # Returns
/// The parsed `LLMResponse` on success, or an error if the job fails or times out.
///
/// # Example
/// ```ignore
/// let response = poll_async_inference(
///     &client,
///     "https://api.example.com/v1/jobs/12345",
///     120,
///     Duration::from_secs(2),
///     Duration::from_secs(30),
///     |req| req.header("Authorization", "Bearer token"),
///     parse_openai_response,
/// ).await?;
/// ```
pub async fn poll_async_inference<F, G>(
    http: &reqwest::Client,
    poll_url: &str,
    max_polls: u32,
    poll_interval: Duration,
    timeout: Duration,
    apply_auth: F,
    parse_response: G,
) -> anyhow::Result<LLMResponse>
where
    F: Fn(reqwest::RequestBuilder) -> reqwest::RequestBuilder,
    G: FnOnce(Value) -> anyhow::Result<LLMResponse>,
{
    for poll_num in 0..max_polls {
        tokio::time::sleep(poll_interval).await;

        let req = apply_auth(http.get(poll_url));
        let resp_val = match req.timeout(timeout).send().await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("Poll {} failed to send: {}", poll_num, e);
                continue;
            }
        };

        let status = resp_val.status();
        if !status.is_success() {
            let err_body = resp_val.text().await.unwrap_or_default();
            let trunc = &err_body[..500.min(err_body.len())];
            anyhow::bail!("async poll HTTP {}: {}", status.as_u16(), trunc);
        }

        let resp: Value = resp_val.json().await?;

        let state = resp["status"].as_str().unwrap_or("unknown");
        match state {
            "completed" | "succeeded" => {
                return parse_response(resp);
            }
            "failed" | "error" => {
                let err = resp["error"].as_str().unwrap_or("unknown error");
                anyhow::bail!("async inference failed: {}", err);
            }
            _ => {
                // Still processing, continue polling
                continue;
            }
        }
    }

    anyhow::bail!("async inference timed out after {} polls", max_polls);
}
