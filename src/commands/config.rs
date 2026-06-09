//! `volt config` — manage LLM provider API keys and base URLs.
//!
//! Reads/writes the user's `volt_home()/.env` and the process environment
//! in lockstep. Designed to be safe in both interactive (TTY) and
//! non-interactive (CI / piped) modes:
//!
//!   volt config list                      # show all providers and their status
//!   volt config get groq                  # show one provider's key (masked) and base URL
//!   volt config set groq gsk_abc...       # set the GROQ_API_KEY
//!   volt config unset groq                # remove the GROQ_API_KEY from .env
//!   volt config doctor                    # same as `volt doctor` but provider-focused
//!   volt config wizard                    # interactive setup for first-time users
//!
//! This is the only way to set API keys from the CLI. The previous design
//! required users to hand-edit `.env`, which silently failed when a
//! placeholder value like `your_groq_api_key_here` was left in place.

use crate::config::{provider_env_var, save_api_key, volt_home};
use crate::llm::provider_detector::{self, DetectedProvider, ProviderStatus};
use std::io::Write;

/// Public entry point for `volt config <subcommand>`.
pub async fn run(sub: ConfigSubcommand) -> anyhow::Result<()> {
    match sub {
        ConfigSubcommand::List => list().await,
        ConfigSubcommand::Get { provider } => get(&provider).await,
        ConfigSubcommand::Set { provider, key } => set(&provider, &key).await,
        ConfigSubcommand::Unset { provider } => unset(&provider).await,
        ConfigSubcommand::Doctor => doctor().await,
        ConfigSubcommand::Wizard => wizard().await,
    }
}

#[derive(Debug, Clone)]
pub enum ConfigSubcommand {
    List,
    Get { provider: String },
    Set { provider: String, key: String },
    Unset { provider: String },
    Doctor,
    Wizard,
}

impl ConfigSubcommand {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "list" | "ls" => Some(Self::List),
            "doctor" => Some(Self::Doctor),
            "wizard" | "setup" => Some(Self::Wizard),
            _ => None,
        }
    }
}

async fn list() -> anyhow::Result<()> {
    let inv = provider_detector::detect();
    println!("Provider status:");
    println!();
    for p in &inv.providers {
        let status_str = match &p.status {
            ProviderStatus::ActiveKey => "active (key set)".to_string(),
            ProviderStatus::ActiveLocal => "active (local server reachable)".to_string(),
            ProviderStatus::ActiveOverride => "active (LLM_BASE_URL override)".to_string(),
            ProviderStatus::InactivePlaceholderKey => {
                "inactive (key is a placeholder value)".to_string()
            }
            ProviderStatus::InactiveNoKey => format!("inactive (set {} to enable)", p.env_var),
            ProviderStatus::InactiveLocalDown => {
                format!("inactive ({} unreachable)", p.base_url)
            }
            ProviderStatus::InactiveLocalNotConfigured => {
                format!("inactive (set {} to enable)", p.env_var)
            }
        };
        let key_display = match p.env_var {
            "" => "—".to_string(),
            env => match std::env::var(env).ok() {
                Some(v) if crate::llm::provider_detector::is_placeholder_key(&v) => {
                    "<placeholder>".to_string()
                }
                Some(v) if !v.is_empty() => mask_key(&v),
                _ => "—".to_string(),
            },
        };
        println!("  {:<10}  {}", p.slug, status_str);
        println!("    base_url : {}", p.base_url);
        if !p.env_var.is_empty() {
            println!("    env var  : {} = {}", p.env_var, key_display);
        }
        if let Some(m) = p.default_model {
            println!("    default  : {}", m);
        }
        println!();
    }
    let active_count = inv.active().count();
    if active_count == 0 {
        println!("No active providers. Run `volt config wizard` to set one up.");
    } else {
        println!("{} active provider(s).", active_count);
    }
    Ok(())
}

async fn get(provider: &str) -> anyhow::Result<()> {
    let inv = provider_detector::detect();
    let p = inv
        .providers
        .iter()
        .find(|p| p.slug == provider)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "unknown provider `{}`. Run `volt config list` for the available providers.",
                provider
            )
        })?;
    let env_var = provider_env_var(&p.slug)
        .ok_or_else(|| anyhow::anyhow!("provider `{}` has no env var", provider))?;
    let key = std::env::var(&env_var).unwrap_or_default();
    println!("provider : {}", p.slug);
    println!("base_url : {}", p.base_url);
    println!("env var  : {} = {}", env_var, mask_key(&key));
    if let Some(m) = p.default_model {
        println!("default  : {}", m);
    }
    Ok(())
}

async fn set(provider: &str, key: &str) -> anyhow::Result<()> {
    let inv = provider_detector::detect();
    let p = inv
        .providers
        .iter()
        .find(|p| p.slug == provider)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "unknown provider `{}`. Run `volt config list` for the available providers.",
                provider
            )
        })?;
    if p.env_var.is_empty() {
        anyhow::bail!("provider `{}` has no API key (it's a local server)", provider);
    }
    if crate::llm::provider_detector::is_placeholder_key(key) {
        anyhow::bail!(
            "the value you provided looks like a placeholder (e.g. `your_*_here`). \
             Paste a real API key from the provider's dashboard."
        );
    }
    let env_var = save_api_key(&p.slug, key)?;
    println!("saved: {} (env var `{}` set in process + .env)", p.slug, env_var);
    println!("use it: volt agent run --input \"...\"");
    Ok(())
}

async fn unset(provider: &str) -> anyhow::Result<()> {
    let inv = provider_detector::detect();
    let p = inv
        .providers
        .iter()
        .find(|p| p.slug == provider)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "unknown provider `{}`. Run `volt config list` for the available providers.",
                provider
            )
        })?;
    let env_var = provider_env_var(&p.slug)
        .ok_or_else(|| anyhow::anyhow!("provider `{}` has no env var", provider))?;
    let home = volt_home();
    let env_path = home.join(".env");
    if !env_path.exists() {
        println!("nothing to remove; .env does not exist at {}", env_path.display());
        return Ok(());
    }
    let existing = std::fs::read_to_string(&env_path)?;
    let mut lines: Vec<String> = existing
        .lines()
        .filter(|l| {
            let trimmed = l.trim_start();
            !trimmed.starts_with(&format!("{}=", env_var))
        })
        .map(|s| s.to_string())
        .collect();
    if !lines.is_empty() && lines.last().map(|l| !l.is_empty()).unwrap_or(false) {
        lines.push(String::new());
    }
    std::fs::write(&env_path, lines.join("\n") + "\n")?;
    std::env::remove_var(&env_var);
    println!("removed: {} (env var `{}` cleared)", p.slug, env_var);
    Ok(())
}

async fn doctor() -> anyhow::Result<()> {
    let inv = provider_detector::detect();
    println!("Provider diagnostics:");
    println!();
    for p in &inv.providers {
        let mark = if p.is_active { "OK " } else { ".. " };
        println!("  [{}] {:<10}  {:?}", mark, p.slug, p.status);
    }
    let active = inv.active().count();
    if active == 0 {
        println!();
        println!("No active providers.");
        println!("To enable one:");
        println!("  1. Get an API key from your provider's dashboard.");
        println!("  2. Run: volt config set <slug> <key>");
        println!("  3. Or run: volt config wizard");
    }
    Ok(())
}

async fn wizard() -> anyhow::Result<()> {
    use crossterm::tty::IsTty;
    if !std::io::stdin().is_tty() {
        eprintln!("`volt config wizard` is interactive. Run it from a terminal,");
        eprintln!("or use `volt config set <slug> <key>` in scripts / CI.");
        anyhow::bail!("non-interactive terminal");
    }
    println!("Volt setup wizard");
    println!("=================");
    println!();
    let inv = provider_detector::detect();
    let inactive: Vec<&DetectedProvider> = inv.providers.iter().filter(|p| !p.is_active).collect();
    if inactive.is_empty() {
        println!("All known providers are active. Nothing to do.");
        return Ok(());
    }
    println!("Pick a provider to configure (or Ctrl-C to quit):");
    for (i, p) in inactive.iter().enumerate() {
        let default_str = p
            .default_model
            .map(|m| format!(" (default: {})", m))
            .unwrap_or_default();
        println!("  {}. {}{}", i + 1, p.slug, default_str);
    }
    let choice = prompt("> ")?;
    let n: usize = choice
        .trim()
        .parse()
        .map_err(|_| anyhow::anyhow!("not a number"))?;
    let pick = inactive
        .get(n - 1)
        .ok_or_else(|| anyhow::anyhow!("out of range"))?;
    if pick.env_var.is_empty() {
        println!("Provider `{}` is a local server. Set {} to its base URL.", pick.slug, pick.env_var);
        return Ok(());
    }
    let key = prompt(&format!("Paste your {} API key: ", pick.display_name))?;
    if key.trim().is_empty() {
        anyhow::bail!("empty key");
    }
    let env_var = save_api_key(&pick.slug, &key)?;
    println!();
    println!("Saved {} to .env (and process env).", env_var);
    println!();
    println!("Quick test:");
    println!("  volt agent run --input \"hello\" --model {}", pick.default_model.unwrap_or("default-model"));
    Ok(())
}

fn prompt(label: &str) -> anyhow::Result<String> {
    print!("{}", label);
    std::io::stdout().flush()?;
    let mut s = String::new();
    std::io::stdin().read_line(&mut s)?;
    Ok(s)
}

fn mask_key(s: &str) -> String {
    let len = s.chars().count();
    if len <= 4 {
        return "****".to_string();
    }
    let tail: String = s.chars().rev().take(4).collect::<String>().chars().rev().collect();
    format!("****{}", tail)
}
