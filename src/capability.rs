use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;

type HmacSha256 = Hmac<Sha256>;

/// Resource scopes that a capability token can grant access to.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CapabilityScope {
    FsRead,
    FsWrite,
    System,
    Network,
    Database,
    Memory,
    Api(String),
}

/// A consumable capability token with HMAC-SHA256 signature.
/// The agent must present a valid token for each tool invocation;
/// tokens are consumed (budget decremented) after execution.
#[derive(Debug, Clone)]
pub struct CapabilityToken {
    pub scope: CapabilityScope,
    pub remaining: u64,
    pub max_budget: u64,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub nonce: String,
    signature: Vec<u8>,
}

/// Clock skew leeway in seconds — tokens within this window of expiry
/// are still accepted, preventing transient NTP drift from killing
/// distributed agent runs. Default: 10 seconds.
const CLOCK_SKEW_LEEWAY_SECONDS: i64 = 10;

/// Error type for capability operations, enabling callers to distinguish
/// between recoverable (budget exhausted, scope not found) and fatal
/// (tampered token) failures without parsing string messages.
#[derive(Debug, thiserror::Error)]
pub enum CapabilityError {
    #[error("scope not found: {0:?}")]
    ScopeNotFound(CapabilityScope),
    #[error("token expired")]
    TokenExpired,
    #[error("budget exhausted (need {0})")]
    BudgetExhausted(u64),
    #[error("scope mismatch: token={0:?} required={1:?}")]
    ScopeMismatch(CapabilityScope, CapabilityScope),
    #[error("signature mismatch — token tampered")]
    SignatureMismatch,
    #[error("internal: {0}")]
    Internal(String),
}

/// Manages issuance, verification, and consumption of capability tokens.
///
/// ### Async-safety
/// Uses `tokio::sync::Mutex` instead of `std::sync::Mutex` so that
/// contention on the token store yields to the Tokio scheduler instead
/// of blocking a worker thread. Under 40-concurrent-tool loads this
/// prevents executor starvation.
pub struct CapabilityManager {
    key: Vec<u8>,
    tokens: Arc<Mutex<HashMap<String, CapabilityToken>>>,
    clock_leeway: chrono::Duration,
}

impl CapabilityManager {
    pub fn new() -> Self {
        let key = rand::random::<[u8; 32]>().to_vec();
        Self {
            key,
            tokens: Arc::new(Mutex::new(HashMap::new())),
            clock_leeway: chrono::Duration::seconds(CLOCK_SKEW_LEEWAY_SECONDS),
        }
    }

    pub fn with_clock_leeway(mut self, seconds: i64) -> Self {
        self.clock_leeway = chrono::Duration::seconds(seconds);
        self
    }

    pub fn with_key(key: Vec<u8>) -> Self {
        Self {
            key,
            tokens: Arc::new(Mutex::new(HashMap::new())),
            clock_leeway: chrono::Duration::seconds(CLOCK_SKEW_LEEWAY_SECONDS),
        }
    }

    /// Issue a new capability token for a scope with a max budget.
    /// Async because the token store uses `tokio::sync::Mutex`.
    pub async fn issue(
        &self,
        scope: CapabilityScope,
        max_budget: u64,
        duration: chrono::Duration,
    ) -> CapabilityToken {
        let nonce = uuid::Uuid::new_v4().to_string();
        let expires_at = chrono::Utc::now() + duration;
        let payload = build_token_payload(&scope, max_budget, max_budget, &expires_at, &nonce);
        let signature = sign_payload(&self.key, &payload);

        let token = CapabilityToken {
            scope,
            remaining: max_budget,
            max_budget,
            expires_at,
            nonce,
            signature,
        };

        let token_clone = token.clone();
        self.tokens.lock().await.insert(token.nonce.clone(), token);
        token_clone
    }

    /// Verify a token is valid for the given scope and has remaining budget.
    pub fn verify(&self, token: &CapabilityToken, scope: &CapabilityScope) -> Result<(), CapabilityError> {
        if token.scope != *scope {
            return Err(CapabilityError::ScopeMismatch(token.scope.clone(), scope.clone()));
        }
        let now = chrono::Utc::now();
        let deadline = token.expires_at + self.clock_leeway;
        if now > deadline {
            return Err(CapabilityError::TokenExpired);
        }
        if token.remaining == 0 {
            return Err(CapabilityError::BudgetExhausted(1));
        }
        let payload = build_token_payload(
            &token.scope,
            token.max_budget,
            token.remaining,
            &token.expires_at,
            &token.nonce,
        );
        let expected = sign_payload(&self.key, &payload);
        if expected != token.signature {
            return Err(CapabilityError::SignatureMismatch);
        }
        Ok(())
    }

    /// List all currently tracked tokens (for capability checks).
    pub async fn list_tokens(&self) -> Vec<CapabilityToken> {
        self.tokens
            .lock()
            .await
            .values()
            .cloned()
            .collect()
    }

    /// Atomically reserve budget from a token for the given scope.
    /// Deducts `amount` from remaining *before* execution so concurrent
    /// tool calls don't over-allocate the same budget.
    /// Returns the token with updated remaining on success.
    pub async fn reserve(&self, scope: &CapabilityScope, amount: u64) -> Result<CapabilityToken, CapabilityError> {
        let mut tokens = self.tokens.lock().await;
        let deadline = chrono::Utc::now() + self.clock_leeway;
        let token = tokens
            .values_mut()
            .find(|t| {
                t.scope == *scope
                    && t.remaining >= amount
                    && deadline <= t.expires_at + self.clock_leeway
            })
            .ok_or_else(|| CapabilityError::BudgetExhausted(amount))?;

        token.remaining -= amount;
        let payload = build_token_payload(
            &token.scope,
            token.max_budget,
            token.remaining,
            &token.expires_at,
            &token.nonce,
        );
        token.signature = sign_payload(&self.key, &payload);
        Ok(token.clone())
    }

    /// Soft-reserve: deducts up to `amount` but never fails.
    /// Returns `(nonce, actual_deducted)` or `None` if no budget left at all.
    /// Use this for dynamic allocation windows — concurrent parallel tools
    /// won't trigger false out-of-budget errors from sibling escrow.
    pub async fn reserve_if_available(&self, scope: &CapabilityScope, amount: u64) -> Option<(String, u64)> {
        let mut tokens = self.tokens.lock().await;
        let deadline = chrono::Utc::now() + self.clock_leeway;
        let token = tokens.values_mut().find(|t| {
            t.scope == *scope
                && t.remaining > 0
                && deadline <= t.expires_at + self.clock_leeway
        })?;

        let actual = amount.min(token.remaining);
        token.remaining -= actual;
        let nonce = token.nonce.clone();
        let payload = build_token_payload(
            &token.scope,
            token.max_budget,
            token.remaining,
            &token.expires_at,
            &token.nonce,
        );
        token.signature = sign_payload(&self.key, &payload);
        Some((nonce, actual))
    }

    /// Refund budget back to a token (called when a tool fails after reserve).
    pub async fn refund(&self, nonce: &str, amount: u64) {
        let mut tokens = self.tokens.lock().await;
        if let Some(token) = tokens.get_mut(nonce) {
            token.remaining = token.remaining.saturating_add(amount);
            let payload = build_token_payload(
                &token.scope,
                token.max_budget,
                token.remaining,
                &token.expires_at,
                &token.nonce,
            );
            token.signature = sign_payload(&self.key, &payload);
        }
    }

    /// Merged atomic reserve + guard creation.
    ///
    /// Deducts `requested_amount` from the budget and returns a
    /// `RefundGuard` in a single async transaction. This eliminates
    /// the budget-leak window where a panic between `reserve()` and
    /// `RefundGuard::new()` would orphan the deducted budget.
    pub async fn acquire_execution_guard(
        &self,
        scope: &CapabilityScope,
        requested_amount: u64,
    ) -> Result<RefundGuard, CapabilityError> {
        let mut tokens = self.tokens.lock().await;
        let deadline = chrono::Utc::now() + self.clock_leeway;

        // First try exact reservation
        if let Some(nonce) = self.try_deduct(&mut tokens, scope, deadline, requested_amount) {
            return Ok(RefundGuard::new(
                self.tokens.clone(),
                nonce,
                requested_amount,
                self.key.clone(),
                self.clock_leeway,
            ));
        }

        // Fallback: soft-reserve whatever is available
        for t in tokens.values_mut() {
            if t.scope == *scope && t.remaining > 0 && deadline <= t.expires_at + self.clock_leeway {
                let actual = requested_amount.min(t.remaining);
                t.remaining -= actual;
                let payload = build_token_payload(
                    &t.scope,
                    t.max_budget,
                    t.remaining,
                    &t.expires_at,
                    &t.nonce,
                );
                t.signature = sign_payload(&self.key, &payload);
                return Ok(RefundGuard::new(
                    self.tokens.clone(),
                    t.nonce.clone(),
                    actual,
                    self.key.clone(),
                    self.clock_leeway,
                ));
            }
        }

        Err(CapabilityError::BudgetExhausted(requested_amount))
    }

    /// Helper: find a token with sufficient budget and deduct, returns nonce on success.
    fn try_deduct(
        &self,
        tokens: &mut HashMap<String, CapabilityToken>,
        scope: &CapabilityScope,
        deadline: chrono::DateTime<chrono::Utc>,
        amount: u64,
    ) -> Option<String> {
        for t in tokens.values_mut() {
            if t.scope == *scope
                && t.remaining >= amount
                && deadline <= t.expires_at + self.clock_leeway
            {
                t.remaining -= amount;
                let payload = build_token_payload(
                    &t.scope,
                    t.max_budget,
                    t.remaining,
                    &t.expires_at,
                    &t.nonce,
                );
                t.signature = sign_payload(&self.key, &payload);
                return Some(t.nonce.clone());
            }
        }
        None
    }

    /// Find a valid token for the given scope with remaining budget.
    pub async fn find_token(&self, scope: &CapabilityScope) -> Option<CapabilityToken> {
        let tokens = self.tokens.lock().await;
        tokens
            .values()
            .find(|t| {
                t.scope == *scope
                    && t.remaining > 0
                    && chrono::Utc::now() <= t.expires_at
            })
            .cloned()
    }

    /// Consume one unit of budget from a token. Returns remaining budget.
    pub fn consume(&self, token: &mut CapabilityToken, amount: u64) -> Result<u64, CapabilityError> {
        if token.remaining < amount {
            return Err(CapabilityError::BudgetExhausted(amount));
        }
        token.remaining -= amount;
        let payload = build_token_payload(
            &token.scope,
            token.max_budget,
            token.remaining,
            &token.expires_at,
            &token.nonce,
        );
        token.signature = sign_payload(&self.key, &payload);
        Ok(token.remaining)
    }
}

fn build_token_payload(
    scope: &CapabilityScope,
    max_budget: u64,
    remaining: u64,
    expires_at: &chrono::DateTime<chrono::Utc>,
    nonce: &str,
) -> String {
    format!(
        "{:?}|{}|{}|{}|{}",
        scope,
        max_budget,
        remaining,
        expires_at.to_rfc3339(),
        nonce
    )
}

fn sign_payload(key: &[u8], payload: &str) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC key should be 32 bytes");
    mac.update(payload.as_bytes());
    mac.finalize().into_bytes().to_vec()
}

/// A Drop guard that refunds budget on drop.
/// Ensures budget is returned even if the tool panics or times out,
/// preventing the "refund hole" where panicking async tasks skip refund.
///
/// ### Safety
/// This guard uses `tokio::sync::Mutex` for the token store access
/// on drop. The `try_lock` approach means that if the mutex is
/// poisoned (another task panicked while holding it), the refund
/// is silently skipped rather than causing a double-panic abort.
pub struct RefundGuard {
    tokens: Option<Arc<Mutex<HashMap<String, CapabilityToken>>>>,
    nonce: Option<String>,
    amount: u64,
    armed: AtomicBool,
    key: Option<Vec<u8>>,
}

impl RefundGuard {
    fn new(
        tokens: Arc<Mutex<HashMap<String, CapabilityToken>>>,
        nonce: String,
        amount: u64,
        key: Vec<u8>,
        _clock_leeway: chrono::Duration,
    ) -> Self {
        Self {
            tokens: Some(tokens),
            nonce: Some(nonce),
            amount,
            armed: AtomicBool::new(true),
            key: Some(key),
        }
    }

    /// Prevent the refund — call when the tool succeeds and budget should stay deducted.
    pub fn defuse(&mut self) {
        self.armed.store(false, Ordering::Release);
    }

    /// Take ownership and prevent the refund (alternative to defuse for move semantics).
    pub fn disarm(self) {
        self.armed.store(false, Ordering::Release);
    }
}

impl Drop for RefundGuard {
    fn drop(&mut self) {
        if !self.armed.load(Ordering::Acquire) {
            return;
        }
        let tokens_arc = self.tokens.take();
        let nonce = self.nonce.take();
        let key = self.key.take();
        let amount = self.amount;
        let nonce_debug = nonce.clone();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            if let (Some(ref tokens_arc), Some(ref nonce), Some(ref key)) = (&tokens_arc, &nonce, &key) {
                let nonce_clone = nonce.clone();
                let tokens_clone = Arc::clone(tokens_arc);
                let key_clone = key.clone();
                tokio::task::block_in_place(|| {
                    let mut guard = tokens_clone.blocking_lock();
                    if let Some(token) = guard.get_mut(&nonce_clone) {
                        token.remaining = token.remaining.saturating_add(amount);
                        let payload = build_token_payload(
                            &token.scope,
                            token.max_budget,
                            token.remaining,
                            &token.expires_at,
                            &token.nonce,
                        );
                        token.signature = sign_payload(&key_clone, &payload);
                    }
                });
            }
        }));
        if result.is_err() {
            eprintln!(
                "CRITICAL: RefundGuard dropped during an unstable panic unwind state \
                 (budget for nonce={:?} amount={} may be lost)",
                nonce_debug, amount
            );
        }
    }
}


/// Map a tool name to the required capability scope for authorization.
/// Fail-closed default: unknown/unmapped tools require System scope (maximum protection).
pub fn tool_required_scope(tool_name: &str) -> CapabilityScope {
    match tool_name {
        "read" | "glob" | "grep" | "list_files" | "file_system" => CapabilityScope::FsRead,
        "write" | "edit" | "create" | "delete" | "rename" | "mkdir" | "rmdir" | "move_file"
        | "copy_file" => CapabilityScope::FsWrite,
        "bash" | "sh" | "powershell" | "cmd" | "run" | "execute" | "run_command"
        | "run_shell" | "spawn_process" => CapabilityScope::System,
        "web_search" | "web_fetch" | "browser_navigate" | "browser_extract" | "web_scrape"
        | "you_research" | "you_contents" | "http_get" | "http_post" => {
            CapabilityScope::Network
        }
        "db_query" | "sql" | "database" | "pg_query" => CapabilityScope::Database,
        "memory_read" | "memory_write" | "memory_search" | "memory_save" => {
            CapabilityScope::Memory
        }
        // Tools with API namespaced scopes
        name if name.starts_with("api_") || name.starts_with("mcp_") => {
            CapabilityScope::Api(name.to_string())
        }
        // Fail-closed: every unmapped tool requires System scope.
        // A developer adding a new tool MUST add it to this match or accept
        // that it will require System-level authorization.
        _ => CapabilityScope::System,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_issue_and_verify() {
        let mgr = CapabilityManager::new();
        let token = mgr.issue(
            CapabilityScope::FsRead,
            10,
            chrono::Duration::hours(1),
        ).await;
        assert!(mgr.verify(&token, &CapabilityScope::FsRead).is_ok());
    }

    #[tokio::test]
    async fn test_wrong_scope_fails() {
        let mgr = CapabilityManager::new();
        let token = mgr.issue(
            CapabilityScope::FsRead,
            10,
            chrono::Duration::hours(1),
        ).await;
        assert!(mgr.verify(&token, &CapabilityScope::FsWrite).is_err());
    }

    #[tokio::test]
    async fn test_consume_reduces_budget() {
        let mgr = CapabilityManager::new();
        let mut token = mgr.issue(
            CapabilityScope::FsRead,
            5,
            chrono::Duration::hours(1),
        ).await;
        assert_eq!(token.remaining, 5);
        mgr.consume(&mut token, 3).unwrap();
        assert_eq!(token.remaining, 2);
        mgr.consume(&mut token, 2).unwrap();
        assert_eq!(token.remaining, 0);
        assert!(mgr.verify(&token, &CapabilityScope::FsRead).is_err());
    }

    #[tokio::test]
    async fn test_exceed_budget_fails() {
        let mgr = CapabilityManager::new();
        let mut token = mgr.issue(
            CapabilityScope::FsRead,
            3,
            chrono::Duration::hours(1),
        ).await;
        assert!(mgr.consume(&mut token, 5).is_err());
    }

    #[tokio::test]
    async fn test_tampered_token_detected() {
        let mgr = CapabilityManager::new();
        let mut token = mgr.issue(
            CapabilityScope::FsRead,
            10,
            chrono::Duration::hours(1),
        ).await;
        token.remaining = 999;
        assert!(mgr.verify(&token, &CapabilityScope::FsRead).is_err());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_reserve_and_guard() {
        let mgr = CapabilityManager::new();
        mgr.issue(
            CapabilityScope::FsRead,
            10,
            chrono::Duration::hours(1),
        ).await;
        let guard = mgr.acquire_execution_guard(&CapabilityScope::FsRead, 3).await.unwrap();
        let remaining = mgr.find_token(&CapabilityScope::FsRead).await.unwrap().remaining;
        assert_eq!(remaining, 7);
        drop(guard);
        let after_refund = mgr.find_token(&CapabilityScope::FsRead).await.unwrap().remaining;
        assert_eq!(after_refund, 10);
    }

    #[tokio::test]
    async fn test_tool_scope_mapping() {
        assert_eq!(tool_required_scope("read"), CapabilityScope::FsRead);
        assert_eq!(tool_required_scope("write"), CapabilityScope::FsWrite);
        assert_eq!(tool_required_scope("bash"), CapabilityScope::System);
        assert_eq!(tool_required_scope("web_search"), CapabilityScope::Network);
        assert_eq!(tool_required_scope("db_query"), CapabilityScope::Database);
        assert_eq!(tool_required_scope("memory_read"), CapabilityScope::Memory);
        assert_eq!(tool_required_scope("final_answer"), CapabilityScope::System);
        assert!(matches!(
            tool_required_scope("api_foo"),
            CapabilityScope::Api(_)
        ));
    }
}
