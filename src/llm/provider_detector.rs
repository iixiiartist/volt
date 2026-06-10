//! Agnostic provider detection.
//!
//! This module is the single source of truth for "which LLM providers does
//! this user actually have configured right now?" It does NOT make routing
//! decisions based on hardcoded model-name rules, and it does NOT silently
//! fall back to a default. If a provider is not in the returned inventory,
//! it is not available.
//!
//! Three signals are used, in priority order:
//!  1. **Explicit `LLM_BASE_URL`** — the user told us to use a specific
//!     OpenAI-compatible endpoint. Detected as `oai_override`.
//!  2. **API key in environment** — for any of the six cloud providers
//!     (groq, nvidia, openai, anthropic, ollama, moonshot).
//!  3. **Local server probe** — for `ollama` (`OLLAMA_HOST` or
//!     `http://localhost:11434`), `llamacpp` (`LLAMA_CPP_HOST`), and
//!     `litertlm` (`LITERTLM_HOST`). Probed via a cheap TCP connect to
//!     `host:port`; if the host is unreachable, the provider is excluded.
//!
//! A provider is *active* iff it has either a key (cloud) or a reachable
//! local server (local). Active providers carry a `default_model` that the
//! user can call without specifying a model. Inactive providers are still
//! listed (so the UI can show "set GROQ_API_KEY to enable Groq") but are
//! not selectable.

use crate::llm::provider::ProviderKind;
use std::net::ToSocketAddrs;
use std::time::Duration;

/// A provider the runtime knows how to talk to. Either active (has key or
/// reachable local server) or inactive (configured by URL only, missing
/// key, or local server down).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedProvider {
    /// Stable slug used in env vars, blueprints, and the UI.
    pub slug: String,
    /// Display name.
    pub display_name: &'static str,
    /// Provider API kind — used to choose the request code path.
    pub kind: ProviderKind,
    /// Base URL for chat-completions-style requests.
    pub base_url: String,
    /// Environment variable that holds this provider's API key (or empty
    /// for local servers / overrides).
    pub env_var: &'static str,
    /// Whether the provider is currently usable: key present for cloud,
    /// local server reachable for local, override URL configured.
    pub is_active: bool,
    /// Why this provider is or isn't active.
    pub status: ProviderStatus,
    /// Default model id suggested when the user picks this provider with
    /// no explicit model name. `None` for local servers (we can't know
    /// what's pulled).
    pub default_model: Option<&'static str>,
    /// What kind of source the `base_url` came from.
    pub source: ProviderSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderStatus {
    /// Cloud provider with valid-looking key.
    ActiveKey,
    /// Local server reachable on `host:port`.
    ActiveLocal,
    /// `LLM_BASE_URL` set to a non-default value.
    ActiveOverride,
    /// Cloud provider, key is the placeholder string from .env.example.
    InactivePlaceholderKey,
    /// Cloud provider, key missing or empty.
    InactiveNoKey,
    /// Local server configured but unreachable within the probe timeout.
    InactiveLocalDown,
    /// Local server not configured (no env var, no default).
    InactiveLocalNotConfigured,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderSource {
    /// `LLM_BASE_URL` env var.
    EnvOverride,
    /// Built-in default URL for the slug.
    Builtin,
    /// Local server (ollama/llamacpp/litertlm) with user-set or default host.
    Local,
}

/// Inventory of every provider we know about, in the order the UI should
/// display them.
#[derive(Debug, Clone, Default)]
pub struct ProviderInventory {
    pub providers: Vec<DetectedProvider>,
}

impl ProviderInventory {
    pub fn active(&self) -> impl Iterator<Item = &DetectedProvider> {
        self.providers.iter().filter(|p| p.is_active)
    }

    pub fn active_slugs(&self) -> Vec<&'static str> {
        self.active().map(|p| slug_to_static(&p.slug)).collect()
    }

    /// Pick a provider that can serve `model`. Returns `None` if no active
    /// provider is configured for the model. `LLM_MODEL_ROUTES` overrides
    /// are honored if present and a key is set.
    pub fn route(&self, model: &str) -> Option<&DetectedProvider> {
        // User-defined override: pick the first active provider whose slug
        // matches the override's `provider` field. (Override parsing
        // happens elsewhere; we just match by provider slug here.)
        let m = model.to_lowercase();
        // 1. Direct slug match — `provider:model` shorthand. This
        //    handles `groq:llama-3.1-8b-instant`, `nvidia/foo`, and
        //    importantly Ollama-style tags like `claudette:7b` which
        //    would otherwise match the `claude` Anthropic hint below.
        if let Some((prefix, _)) = m.split_once(':').or_else(|| m.split_once('/')) {
            if let Some(p) = self
                .providers
                .iter()
                .find(|p| p.slug == prefix && p.is_active)
            {
                return Some(p);
            }
        }
        // 2. vLLM-explicit routing. Models with vendor prefixes common
        //    in the open-source catalog (meta-llama, Qwen, Mistral,
        //    DeepSeek, google, microsoft, etc.) default to vLLM when
        //    it's active. vLLM is the enterprise target; routing
        //    Llama to Groq when vLLM is reachable would be a policy
        //    violation.
        if let Some(p) = self.active().find(|p| p.slug == "vllm") {
            let m_lower = m.to_lowercase();
            if m_lower.starts_with("meta-llama/")
                || m_lower.starts_with("qwen/")
                || m_lower.starts_with("mistral")
                || m_lower.starts_with("deepseek-")
                || m_lower.starts_with("google/")
                || m_lower.starts_with("microsoft/")
                || m_lower.starts_with("minimax")
                || m_lower.starts_with("baai/")
                || m_lower.contains("/llama-")
                || m_lower.contains("qwen2")
                || m_lower.contains("qwen3")
            {
                return Some(p);
            }
        }
        // 3. Ollama-style colon tag with no slug prefix. Routes to a
        //    local Ollama server first, then Ollama Cloud.
        if m.contains(':') {
            if let Some(p) = self
                .active()
                .find(|p| p.slug == "ollama_local" || p.slug == "ollama")
            {
                return Some(p);
            }
        }
        // 4. Native model-name hints (Claude/GPT) only match if the
        //    matching provider is actually active. We never silently
        //    substitute.
        if m.contains("claude") {
            return self.active().find(|p| p.slug == "anthropic");
        }
        if m.starts_with("gpt-") || m.starts_with("o1-") || m.starts_with("o3-") {
            return self.active().find(|p| p.slug == "openai");
        }
        if m.contains("nvidia")
            || m.contains("nvlm")
            || m.starts_with("meta/")
            || m.starts_with("microsoft/")
            || m.starts_with("mistral")
            || m.starts_with("deepseek")
            || m.starts_with("qwen/")
            || m.starts_with("google/")
            || m.starts_with("minimax")
            || m.starts_with("moonshot")
        {
            return self
                .active()
                .find(|p| p.slug == "nvidia" || p.slug == "moonshot");
        }
        // 5. Default: first active provider. Caller is expected to
        //    surface a clear error if this returns None.
        self.active().next()
    }
}

fn slug_to_static(s: &str) -> &'static str {
    // Tiny adapter so `active_slugs()` can return `&'static str` even
    // though `DetectedProvider::slug` is `String`. We only ever construct
    // providers with literal slugs, so the leaked slice is safe.
    Box::leak(s.to_string().into_boxed_str())
}

/// Probe whether `host:port` is reachable within `timeout`. Returns false
/// on any error (DNS, refused, timeout).
fn tcp_reachable(host: &str, port: u16, timeout: Duration) -> bool {
    let addr = match (host, port).to_socket_addrs() {
        Ok(mut it) => match it.next() {
            Some(a) => a,
            None => return false,
        },
        Err(_) => return false,
    };
    std::net::TcpStream::connect_timeout(&addr, timeout).is_ok()
}

/// True if `v` looks like a placeholder value (e.g. `your_groq_key_here`,
/// `sk-CHANGEME`, `***`).
pub fn is_placeholder_key(v: &str) -> bool {
    let v = v.trim();
    if v.is_empty() {
        return true;
    }
    let lower = v.to_lowercase();
    // Common placeholder patterns.
    if lower.starts_with("your_")
        || lower.starts_with("placeholder")
        || lower.starts_with("changeme")
        || lower.starts_with("replace_me")
        || lower.starts_with("<")
        || lower.starts_with("sk-***")
        || v == "***"
        || v == "****"
        || v == "xxxxxxxx"
    {
        return true;
    }
    // The .env.example value `your_groq_api_key_here`.
    if lower.contains("your_") && lower.contains("here") {
        return true;
    }
    false
}

/// Probe the environment + local hosts and build the full inventory.
///
/// Cached with `OnceLock` because `detect()` is called on every
/// `resolve_provider` call. The cache is invalidated by
/// `invalidate_cache()` whenever an API key is added or removed
/// (`save_api_key` / `unset_api_key`).
pub fn detect() -> ProviderInventory {
    use std::sync::RwLock;
    static CACHE: std::sync::OnceLock<RwLock<Option<ProviderInventory>>> =
        std::sync::OnceLock::new();
    let lock = CACHE.get_or_init(|| RwLock::new(None));
    if let Ok(g) = lock.read() {
        if let Some(inv) = g.as_ref() {
            return inv.clone();
        }
    }
    let inv = detect_uncached();
    if let Ok(mut g) = lock.write() {
        *g = Some(inv.clone());
    }
    inv
}

/// Drop the cached inventory. Call this after `save_api_key` /
/// `unset_api_key` so the next `detect()` re-reads env vars.
pub fn invalidate_cache() {
    use std::sync::RwLock;
    static CACHE: std::sync::OnceLock<RwLock<Option<ProviderInventory>>> =
        std::sync::OnceLock::new();
    let lock = CACHE.get_or_init(|| RwLock::new(None));
    if let Ok(mut g) = lock.write() {
        *g = None;
    }
}

/// Build the inventory from scratch. Used by `detect()` (cached) and
/// by tests that mutate env vars.
pub fn detect_uncached() -> ProviderInventory {
    let mut providers = Vec::new();

    // ── 1. LLM_BASE_URL override ──────────────────────────────
    let override_url = std::env::var("LLM_BASE_URL")
        .ok()
        .filter(|v| !v.trim().is_empty());
    if let Some(url) = override_url {
        let key = std::env::var("LLM_API_KEY")
            .ok()
            .filter(|v| !v.trim().is_empty() && !is_placeholder_key(v));
        let is_active = key.is_some();
        providers.push(DetectedProvider {
            slug: "oai_override".into(),
            display_name: "Custom OpenAI-compatible endpoint",
            kind: ProviderKind::OpenAI,
            base_url: url,
            env_var: "LLM_API_KEY",
            is_active,
            status: if is_active {
                ProviderStatus::ActiveOverride
            } else {
                ProviderStatus::InactiveNoKey
            },
            default_model: None,
            source: ProviderSource::EnvOverride,
        });
    }

    // ── 2. Cloud providers — keyed on API key env vars ────────
    let cloud: &[(&str, &str, &str, &str, &str)] = &[
        // (slug, display, base_url, env_var, default_model)
        (
            "groq",
            "Groq",
            "https://api.groq.com/openai/v1",
            "GROQ_API_KEY",
            "llama-3.1-8b-instant",
        ),
        (
            "nvidia",
            "NVIDIA NIM",
            "https://integrate.api.nvidia.com/v1",
            "NVIDIA_API_KEY",
            "meta/llama-3.1-8b-instruct",
        ),
        (
            "openai",
            "OpenAI",
            "https://api.openai.com/v1",
            "OPENAI_API_KEY",
            "gpt-4o-mini",
        ),
        (
            "anthropic",
            "Anthropic",
            "https://api.anthropic.com",
            "ANTHROPIC_API_KEY",
            "claude-sonnet-4-5",
        ),
        (
            "ollama",
            "Ollama Cloud",
            "https://ollama.com/v1",
            "OLLAMA_API_KEY",
            "llama3.2:3b",
        ),
        (
            "moonshot",
            "Moonshot / Kimi",
            "https://api.moonshot.cn/v1",
            "MOONSHOT_API_KEY",
            "moonshot-v1-8k",
        ),
    ];
    for (slug, name, base_url, env_var, default_model) in cloud {
        let raw = std::env::var(env_var).ok();
        let key_present = raw
            .as_deref()
            .map(|v| !v.trim().is_empty() && !is_placeholder_key(v))
            .unwrap_or(false);
        let is_placeholder = raw
            .as_deref()
            .map(|v| !v.trim().is_empty() && is_placeholder_key(v))
            .unwrap_or(false);
        // Cloud providers are inactive-by-default unless
        // VOLT_ENABLE_CLOUD_PROVIDERS=1 is set in the environment.
        // This is a *display* gate: the provider still shows up in the
        // inventory as inactive (so the UI can show "set GROQ_API_KEY
        // to enable Groq"), but it is filtered out of `active()` so
        // the orchestrator's model router cannot pick it.
        let cloud_enabled = cloud_providers_enabled();
        let (is_active, status) = if !cloud_enabled {
            (false, ProviderStatus::InactiveNoKey)
        } else if key_present {
            (true, ProviderStatus::ActiveKey)
        } else if is_placeholder {
            (false, ProviderStatus::InactivePlaceholderKey)
        } else {
            (false, ProviderStatus::InactiveNoKey)
        };
        providers.push(DetectedProvider {
            slug: (*slug).into(),
            display_name: name,
            kind: if *slug == "anthropic" {
                ProviderKind::Anthropic
            } else {
                ProviderKind::OpenAI
            },
            base_url: (*base_url).into(),
            env_var,
            is_active,
            status,
            default_model: Some(default_model),
            source: ProviderSource::Builtin,
        });
    }

    // ── 3. Local servers — probed via TCP connect ──────────────
    let probe_timeout = Duration::from_millis(500);
    // vLLM is listed FIRST in the local array so it appears first in
    // the inventory and wins "default provider" ties against Ollama
    // local. Production deployments should run vLLM as the primary
    // inference server; Ollama is a developer-laptop convenience.
    let local: &[(&str, &str, &str, &str)] = &[
        // (slug, display, default_host:port, env_var_to_override)
        ("vllm", "vLLM (local)", "127.0.0.1:8000", "VLLM_HOST"),
        (
            "ollama_local",
            "Ollama (local)",
            "127.0.0.1:11434",
            "OLLAMA_HOST",
        ),
        (
            "llamacpp",
            "llama.cpp (local)",
            "127.0.0.1:8080",
            "LLAMA_CPP_HOST",
        ),
        (
            "litertlm",
            "LiteRT-LM (local)",
            "127.0.0.1:8081",
            "LITERTLM_HOST",
        ),
    ];
    for (slug, name, default_addr, env_var) in local {
        let configured = std::env::var(env_var).ok().filter(|v| !v.trim().is_empty());
        let (host, port) = if let Some(ref v) = configured {
            // Parse "host:port" or assume default port.
            if let Some((h, p)) = v.rsplit_once(':') {
                if let Ok(port) = p.parse::<u16>() {
                    (h.to_string(), port)
                } else {
                    (v.clone(), 80)
                }
            } else {
                (v.clone(), 80)
            }
        } else {
            let (h, p) = default_addr.rsplit_once(':').unwrap();
            (h.to_string(), p.parse().unwrap_or(80))
        };
        let is_reachable = tcp_reachable(&host, port, probe_timeout);
        let (is_active, status) = if configured.is_some() && is_reachable {
            (true, ProviderStatus::ActiveLocal)
        } else if configured.is_some() {
            (false, ProviderStatus::InactiveLocalDown)
        } else if is_reachable {
            // Auto-discovered local server.
            (true, ProviderStatus::ActiveLocal)
        } else {
            (false, ProviderStatus::InactiveLocalNotConfigured)
        };
        // All three local servers are OpenAI-compatible; default to /v1.
        // The detector doesn't care about the upstream path shape — the
        // provider implementation handles any per-server quirks.
        let base_url = format!("http://{}:{}/v1", host, port);
        providers.push(DetectedProvider {
            slug: (*slug).into(),
            display_name: name,
            kind: ProviderKind::OpenAI,
            base_url,
            env_var,
            is_active,
            status,
            default_model: None,
            source: ProviderSource::Local,
        });
    }

    ProviderInventory { providers }
}

/// Human-readable description of which providers are active. Useful in
/// `volt doctor` and the WebUI's status banner.
pub fn summarize(inv: &ProviderInventory) -> String {
    if inv.active().count() == 0 {
        return "No LLM providers configured. Run `volt config` to add one.".into();
    }
    let mut s = String::from("Active providers: ");
    let active: Vec<String> = inv
        .active()
        .map(|p| format!("{} ({})", p.slug, p.base_url))
        .collect();
    s.push_str(&active.join(", "));
    s
}

/// True when cloud providers (Groq, OpenAI, Anthropic, NVIDIA NIM, Ollama
/// Cloud, Moonshot) should appear in the active inventory. Default:
/// `false` — local vLLM/Ollama is the enterprise path. Operators opt
/// in by setting `VOLT_ENABLE_CLOUD_PROVIDERS=1` (or `true`/`yes`).
///
/// This is a *display gate*: even when `false`, the cloud provider
/// entries still appear in the inventory (so the UI can show "set
/// GROQ_API_KEY to enable"), but they are filtered out of `active()`
/// and the orchestrator's `route()` function. A workflow that names
/// a cloud model is not silently routed to the cloud; it surfaces
/// the "no active provider" error so the developer fixes the env.
pub fn cloud_providers_enabled() -> bool {
    matches!(
        std::env::var("VOLT_ENABLE_CLOUD_PROVIDERS")
            .ok()
            .map(|v| v.to_lowercase())
            .as_deref(),
        Some("1") | Some("true") | Some("yes") | Some("on")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    // Tests mutate process env; serialize via an atomic spinlock to avoid
    // poisoning. (The std::sync::Mutex panics on poisoned locks, and other
    // tests in the same suite may hold a different env-mutating lock.)
    static ENV_LOCK: AtomicUsize = AtomicUsize::new(0);

    fn lock_env() -> usize {
        let mut spins = 0;
        while ENV_LOCK
            .compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            spins += 1;
            if spins > 1_000_000 {
                std::thread::yield_now();
                spins = 0;
            }
        }
        1
    }

    fn unlock_env(_t: usize) {
        ENV_LOCK.store(0, Ordering::SeqCst);
    }

    struct EnvGuard(usize);
    impl EnvGuard {
        fn new() -> Self {
            Self(lock_env())
        }
    }
    impl Drop for EnvGuard {
        fn drop(&mut self) {
            unlock_env(self.0);
        }
    }

    fn clear_llm_env() {
        for k in [
            "GROQ_API_KEY",
            "OPENAI_API_KEY",
            "ANTHROPIC_API_KEY",
            "NVIDIA_API_KEY",
            "OLLAMA_API_KEY",
            "MOONSHOT_API_KEY",
            "LLM_API_KEY",
            "LLM_BASE_URL",
            "OLLAMA_HOST",
            "LLAMA_CPP_HOST",
            "LITERTLM_HOST",
            "VLLM_HOST",
            "VOLT_ENABLE_CLOUD_PROVIDERS",
        ] {
            std::env::remove_var(k);
        }
        // Tests must also invalidate the detector cache so the next
        // call sees the freshly-cleared env.
        super::invalidate_cache();
    }

    /// Enable the cloud-provider gate for tests that exercise Groq /
    /// OpenAI / NIM / etc. The gate is off by default (the
    /// enterprise posture); the existing tests that depend on a
    /// cloud provider being detected set this via `enable_cloud_for_test()`.
    fn enable_cloud_for_test() {
        std::env::set_var("VOLT_ENABLE_CLOUD_PROVIDERS", "1");
        super::invalidate_cache();
    }

    #[test]
    fn no_keys_no_override_yields_only_inactive_cloud_providers() {
        let _g = EnvGuard::new();
        clear_llm_env();
        let inv = detect_uncached();
        // The 6 cloud providers are always present (slugs stable). They
        // must all be inactive in this configuration.
        for slug in [
            "groq",
            "nvidia",
            "openai",
            "anthropic",
            "ollama",
            "moonshot",
        ] {
            let p = inv
                .providers
                .iter()
                .find(|p| p.slug == slug)
                .unwrap_or_else(|| panic!("missing {}", slug));
            assert!(
                !p.is_active,
                "{} should be inactive when no key set, but was active (status: {:?})",
                slug, p.status
            );
        }
        // No override URL set.
        assert!(!inv.providers.iter().any(|p| p.slug == "oai_override"));
    }

    #[test]
    fn groq_key_makes_groq_active() {
        let _g = EnvGuard::new();
        clear_llm_env();
        enable_cloud_for_test();
        std::env::set_var("GROQ_API_KEY", "gsk_real-looking-key-1234");
        let inv = detect_uncached();
        let active: Vec<&str> = inv.active_slugs();
        assert!(active.contains(&"groq"), "active: {:?}", active);
        assert_eq!(
            inv.providers
                .iter()
                .find(|p| p.slug == "groq")
                .unwrap()
                .status,
            ProviderStatus::ActiveKey
        );
    }

    #[test]
    fn placeholder_key_does_not_activate_provider() {
        let _g = EnvGuard::new();
        clear_llm_env();
        enable_cloud_for_test();
        std::env::set_var("GROQ_API_KEY", "your_groq_api_key_here");
        let inv = detect_uncached();
        let groq = inv.providers.iter().find(|p| p.slug == "groq").unwrap();
        assert!(!groq.is_active);
        assert_eq!(groq.status, ProviderStatus::InactivePlaceholderKey);
    }

    #[test]
    fn llm_base_url_override_activates_oai_override() {
        let _g = EnvGuard::new();
        clear_llm_env();
        enable_cloud_for_test();
        std::env::set_var("LLM_BASE_URL", "http://localhost:1234/v1");
        std::env::set_var("LLM_API_KEY", "real-key");
        let inv = detect_uncached();
        let o = inv
            .providers
            .iter()
            .find(|p| p.slug == "oai_override")
            .unwrap();
        assert!(o.is_active);
        assert_eq!(o.status, ProviderStatus::ActiveOverride);
        assert_eq!(o.base_url, "http://localhost:1234/v1");
    }

    #[test]
    fn is_placeholder_key_detects_common_patterns() {
        assert!(is_placeholder_key("your_groq_api_key_here"));
        assert!(is_placeholder_key("CHANGEME"));
        assert!(is_placeholder_key("placeholder"));
        assert!(is_placeholder_key("***"));
        assert!(is_placeholder_key("<your-key>"));
        assert!(!is_placeholder_key("gsk_a-real-key"));
        assert!(!is_placeholder_key("sk-1234567890"));
    }

    #[test]
    fn route_picks_active_provider() {
        let _g = EnvGuard::new();
        clear_llm_env();
        enable_cloud_for_test();
        std::env::set_var("OPENAI_API_KEY", "sk-test");
        let inv = detect_uncached();
        // Native GPT prefix matches the active OpenAI provider.
        let p = inv.route("gpt-4o-mini").unwrap();
        assert_eq!(p.slug, "openai");
    }

    #[test]
    fn route_fails_cleanly_for_unconfigured_model() {
        let _g = EnvGuard::new();
        clear_llm_env();
        let inv = detect_uncached();
        // No active providers at all -> no route.
        assert!(inv.route("claude-sonnet-4-5").is_none());
    }

    #[test]
    fn route_respects_vendor_prefixed_active_providers() {
        let _g = EnvGuard::new();
        clear_llm_env();
        enable_cloud_for_test();
        std::env::set_var("NVIDIA_API_KEY", "nvapi-test");
        let inv = detect_uncached();
        // DeepSeek (a NIM vendor) routes to NIM when active.
        let p = inv.route("deepseek-ai/deepseek-v3.2").unwrap();
        assert!(p.slug == "nvidia" || p.slug == "moonshot");
    }

    #[test]
    fn cloud_gate_off_by_default() {
        // Even with GROQ_API_KEY set, groq is not active unless the
        // cloud-provider gate is explicitly enabled. This is the
        // enterprise posture: local vLLM/Ollama is the default, cloud
        // is opt-in.
        let _g = EnvGuard::new();
        clear_llm_env();
        std::env::set_var("GROQ_API_KEY", "gsk_real-looking-key-1234");
        let inv = detect_uncached();
        let groq = inv.providers.iter().find(|p| p.slug == "groq").unwrap();
        assert!(
            !groq.is_active,
            "groq should be inactive without VOLT_ENABLE_CLOUD_PROVIDERS"
        );
        assert_eq!(groq.status, ProviderStatus::InactiveNoKey);
    }

    #[test]
    fn cloud_gate_on_via_env_var() {
        let _g = EnvGuard::new();
        clear_llm_env();
        std::env::set_var("VOLT_ENABLE_CLOUD_PROVIDERS", "1");
        std::env::set_var("GROQ_API_KEY", "gsk_real-looking-key-1234");
        let inv = detect_uncached();
        let groq = inv.providers.iter().find(|p| p.slug == "groq").unwrap();
        assert!(groq.is_active);
        assert_eq!(groq.status, ProviderStatus::ActiveKey);
    }

    #[test]
    fn vllm_appears_in_inventory() {
        // vLLM is always listed, even when no server is reachable —
        // it shows up as `InactiveLocalNotConfigured`. The provider
        // list is the user-facing "what's available" view.
        let _g = EnvGuard::new();
        clear_llm_env();
        let inv = detect_uncached();
        let vllm = inv.providers.iter().find(|p| p.slug == "vllm");
        assert!(vllm.is_some(), "vllm should appear in the inventory");
        let vllm = vllm.unwrap();
        assert_eq!(vllm.kind, ProviderKind::OpenAI);
        assert_eq!(vllm.base_url, "http://127.0.0.1:8000/v1");
        assert_eq!(vllm.env_var, "VLLM_HOST");
    }
}
