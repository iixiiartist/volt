# VOLT CLI UX Audit — vs. Claude Code CLI and Codex CLI

**Audit date:** June 3, 2026
**Scope:** `src/main.rs`, `src/commands/*.rs`, `src/tui.rs`, `src/config.rs`, `Cargo.toml`, `README.md`
**Reference:** Claude Code CLI v1.x, OpenAI Codex CLI v0.x

---

## Executive Summary

VOLT's CLI is **functionally complete but UX-light**. It has a solid foundation (DAG workflows, blueprints, MCP server, RAG retrieval, first-run wizard) but the user-facing surface area feels like a developer's tool, not a developer experience. The TUI is a 338-line proof-of-concept, the command tree is a flat clap list, and there is no shell completion, no slash commands, no streaming in the TUI, no inline approvals, no syntax highlighting, and no cost/token HUD.

Three of the top 10 recommendations are **1-day wins** that would dramatically raise the polish bar. Five are **1–2 week efforts** that would put VOLT on par with Codex CLI's interactivity. Two are **multi-week** efforts that would rival Claude Code's polish.

---

## Part 1 — Current State: Strengths

| Strength | Evidence | Why it matters |
|---|---|---|
| **DAG multi-agent orchestration** | `src/commands/workflow.rs`, `src/orchestrator.rs:903` (`topological_sort`, `execution_levels`) | Unique to VOLT — neither Claude Code nor Codex exposes DAG execution |
| **67 production blueprints** | `blueprints/` (19 Groq + 20 NIM + 25 Ollama + 3 Edge) | Compensates for small-model tool-call quirks — proprietary value |
| **Auto-blueprint mode** | `agent_run.rs:96-120` (`route_task` via LLM) | Smart prompt → blueprint routing is unique |
| **Permission attenuation** | `src/attenuation/`, `models.rs:677` (`Allow`/`Prompt`/`ReadOnly`/`Blocked`) | Per-tool policy is more granular than Claude Code's `permissionDecision` |
| **First-run wizard** | `config.rs:80-323` | Already prompts for LLM provider + API key + DB URL |
| **AGENTS.md / SOUL.md / MEMORY.md discovery** | `worker.rs:371-407` (`seed_from_workspace`) | Workspace files auto-seeded into `ContextStore` |
| **MCP server with permission gating** | `src/mcp/server.rs`, `tools::execute_gated` | Tools are gated through the same approval layer — EU AI Act Art. 14 compliant |
| **Three context modes** | `commands/mod.rs:5-56` (`Precision`/`Balanced`/`Autonomous`) | Task-aware context ablation data backs these profiles |
| **Cross-model routing** | `agent/router.rs` (`get_active_providers`, `filter_blueprints`) | Auto-routes to live API keys only |
| **ONNX hardware acceleration** | `ort` with DirectML/OpenVINO/CUDA fallback | Local embeddings with no cloud dependency |

---

## Part 2 — Current State: Weaknesses

### Critical gaps (high visibility, low fix cost)

| Gap | Evidence | Reference |
|---|---|---|
| **No streaming in TUI** | `agent_tui.rs:55-57` does **not** call `.with_stream(...)`; `tui.rs:26,64,141-162` declares `stream_buffer` but never receives tokens from `Agent`. Compare `agent_run.rs:129-131` which does set it. | Claude Code streams every token live |
| **No slash commands in TUI** | `tui.rs:205-207` only handles `/quit`. No `/help`, `/model`, `/clear`, `/compact`, `/permissions`, `/cost`, `/init`, `/status` | Claude Code's `/help` and `/model` are first-class |
| **No shell completions** | `Cargo.toml:38` has `clap = "4"` with `derive` + `env`, but **not** `clap_complete` | Every modern CLI has them |
| **TUI is single-line, no history, no multiline** | `tui.rs:198-253` — `Char(c)` just inserts one char, no Up/Down history, no Ctrl-A/E, no Ctrl-C/V, no tab completion | Codex's composer has multiline + history |
| **First-run wizard is `println!`-based** | `config.rs:107-323` uses raw `stdin.read_line` and numbered choices; no arrow-key navigation, no masking for API keys | Codex uses arrow-key picker |
| **API key prompted in plaintext** | `config.rs:140,164,188,212` — `print!("GROQ_API_KEY: ")` then `read_line` shows input | Standard pattern is masked input |
| **Approval prompts are raw stdin** | `agent/run.rs:843-853` — `stdin.read_line` for `y/N`, with ANSI escapes piped via `println!` | Should reuse the TUI's approval widget |
| **No `--print` / `--json` non-interactive output mode** | `agent-run` always prints plain text | Claude Code has `--print` for piping; agents need JSON |
| **No token / cost / duration HUD in TUI** | `models.rs:107` (`total_completion_tokens`) tracks tokens but TUI never displays them | Codex shows it as a footer |
| **No `--resume` friendly lookup** | `--session-id <uuid>` requires pasting a raw UUID; `agent_tui.rs:92-126` lists sessions on startup but you can't resume mid-session by name | Claude Code has `claude --resume` |
| **AGENTS.md is seeded into context store, not the system prompt** | `prompt.rs:22-43` only loads `SOUL.md`/`MEMORY.md`/`USER.md`; `AGENTS.md` lives in `ContextKind::Policy` only and is RAG-retrieved, never injected | Codex/Claude inject `AGENTS.md` directly into system prompt |
| **No interactive file picker with `@`** | No `@`-prefix file completion | Claude Code has this |
| **No rich TUI for tools panel / side panel** | `tui.rs:258-262` — vertical layout only | Codex has sidebar with file list |
| **No `volt update` / self-upgrade** | Not in `Commands` enum | Codex ships `codex update` |
| **No OSC 8 clickable links** in errors | Errors just `eprintln!` to stderr | Codex emits OSC 8 for URLs |
| **No `clap` long_about / per-subcommand help** | `main.rs:9` only sets a one-liner; no `long_about`, no `after_help`, no examples | clap makes this trivial |
| **No `volt completion <shell>` subcommand** | Missing | Standard pattern |

### Subtle gaps (UX polish, medium cost)

| Gap | Reference |
|---|---|
| **No syntax highlighting** in TUI (despite `syntect` already in deps) | Claude Code highlights tool calls in olive/cyan |
| **No progress indicator** during long agent runs | `indicatif` would show "thinking..." spinner with elapsed time |
| **No fuzzy find for slash commands** | Even simple `/` then `Tab` completion would help |
| **No `--cd` flag to set working directory** (Codex has this) | Always inherits CWD |
| **No session title editing** | `session.rs:158` (`title: input.clone()`) — uses truncated input |
| **No worktree support** | Codex can run in a git worktree |
| **No `codex --oss` equivalent** for local models | VOLT does have OLLAMA detection, but no first-class `--local` flag |
| **No plan mode** (`/plan` in Claude Code, Codex `/approvals`) | Volt has `mode = "precision"` but no "read-only" mode |
| **No hook system** (PreToolUse/PostToolUse) | Volt's permission system is similar in spirit but not extensible |
| **No conversation fork** | Sessions are append-only |

---

## Part 3 — Top 10 Concrete Improvements (ranked by impact × feasibility)

### 1. Add `clap_complete` + `volt completion <shell>` subcommand  ⭐ Quick win

**Impact:** High (every dev using the CLI shells into it daily)
**Effort:** 2–4 hours
**Files:** `Cargo.toml`, `src/main.rs`, `src/commands/mod.rs`

**Why it matters:** Claude Code and Codex both ship completions. `clap_complete` already works with the existing `clap = "4"` derive setup. This is genuinely a one-evening change.

**Implementation:**

```toml
# Cargo.toml addition
clap_complete = "4"
```

```rust
// src/main.rs
use clap_complete::Shell;
use std::io;

#[derive(Subcommand, Debug)]
enum Commands {
    /// Generate shell completions (bash, zsh, fish, powershell, elvish)
    Completion {
        /// Target shell: bash, zsh, fish, powershell, elvish
        shell: Shell,
        /// Write to file instead of stdout (e.g. ~/.local/share/bash-completion/completions/volt)
        #[arg(long)]
        out: Option<PathBuf>,
    },
    // ... existing subcommands ...
}

// in main():
Commands::Completion { shell, out } => {
    let mut buf = Vec::new();
    clap_complete::generate(shell, &mut Cli::command(), "volt", &mut buf);
    if let Some(path) = out {
        std::fs::write(&path, &buf)?;
        eprintln!("wrote {} ({} bytes)", path.display(), buf.len());
    } else {
        io::stdout().write_all(&buf)?;
    }
    Ok(())
}
```

User experience:
```bash
$ volt completion bash --out ~/.local/share/bash-completion/completions/volt
$ volt agent-run --<TAB>   # expands to --allow, --auto-blueprint, --blueprint...
$ volt --<TAB>             # expands to all top-level subcommands
```

---

### 2. Stream LLM tokens into the TUI in real time  ⭐ Quick win

**Impact:** Highest (this is the *single biggest* perceived-quality improvement)
**Effort:** 4–8 hours
**Files:** `src/commands/agent_tui.rs:55-57`, `src/tui.rs:153-195`

**Why it matters:** Currently `agent_tui.rs` builds an `Agent` **without** calling `.with_stream(...)` (compare `agent_run.rs:129-131`). The TUI has a `stream_buffer: String` field at `tui.rs:26` that is declared and reset but **never receives streamed tokens** — so the user stares at a frozen screen until the full response appears, then everything flashes in at once. Claude Code and Codex both stream every token live. This is the single largest perceived-quality gap.

**Implementation:**

```rust
// src/commands/agent_tui.rs — replace lines 55-57
let (stream_tx, mut stream_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
let mut agent = Agent::new(config, provider, tools.clone())
    .await
    .with_workspace(std::env::current_dir().unwrap_or_default())
    .with_stream(Arc::new(move |token| {
        // bridge sync callback into async TUI loop
        let _ = stream_tx.send(token.to_string());
    }));

// Spawn a small task that drains the channel into a shared Mutex<String>
// that the TUI's render() loop reads on each draw.
```

```rust
// src/tui.rs — modify render() to read a shared buffer
struct TuiChat {
    // ... existing fields ...
    live_stream: Arc<tokio::sync::Mutex<String>>,
}

fn render(&self, f: &mut Frame) {
    // In the assistant area, also show live_stream content
    // Use try_lock() to avoid blocking the render thread.
}
```

The `handle_agent_response` method at `tui.rs:153-195` currently calls `agent.run(&input).await` and gets a single `String` back. We need to switch to a streaming variant: pass the stream callback through to the LLM provider, and the LLM response loop pushes tokens into the shared buffer.

---

### 3. Add slash commands to the TUI  ⭐ Quick win

**Impact:** Very high (this is what makes a TUI feel "alive")
**Effort:** 4–8 hours
**Files:** `src/tui.rs:198-254` (`handle_key_event`)

**Why it matters:** Right now only `/quit` is recognized (`tui.rs:205-207`). Users from Claude Code/Codex expect `/help`, `/model`, `/clear`, `/compact`, `/status`, `/cost`, `/init`, `/permissions`, `/resume`, `/tools`, `/sessions`. Implementation is a simple `match` on `self.input.trim().starts_with('/')` before treating input as a prompt.

**Implementation sketch:**

```rust
// In handle_key_event, before KeyCode::Enter runs the agent:
KeyCode::Enter => {
    let input = self.input.trim().to_string();
    if input.is_empty() { return false; }
    match self.execute_slash_command(&input).await {
        SlashResult::Quit => return true,
        SlashResult::Handled => {
            self.input.clear();
            self.cursor_pos = 0;
            return false;
        }
        SlashResult::NotASlash => {} // fall through to normal send
    }
    self.add_message("user", &input);
    // ... existing is_thinking = true ...
}
```

```rust
// src/tui.rs
enum SlashResult { Quit, Handled, NotASlash }

impl TuiChat {
    async fn execute_slash_command(&mut self, raw: &str) -> SlashResult {
        let parts: Vec<&str> = raw.split_whitespace().collect();
        match parts[0] {
            "/quit" | "/exit" | "/q" => SlashResult::Quit,
            "/help" | "/?" => {
                self.add_message("system", HELP_TEXT);
                SlashResult::Handled
            }
            "/clear" => {
                self.messages.clear();
                self.add_message("system", "(conversation cleared)");
                SlashResult::Handled
            }
            "/model" => {
                let msg = match parts.get(1) {
                    Some(m) => format!("Switched model to: {}", m),
                    None => "Usage: /model <name>".into(),
                };
                self.add_message("system", &msg);
                SlashResult::Handled
            }
            "/compact" => { /* truncate history, keep system */ SlashResult::Handled }
            "/status" => {
                self.add_message("system", &format!(
                    "model: {}\nmsgs: {}\nturns: {}", self.model, self.messages.len(), self.turn_count
                ));
                SlashResult::Handled
            }
            "/cost" | "/tokens" => {
                self.add_message("system", &format!(
                    "↑ {} prompt tokens · ↓ {} completion tokens · est. ${:.4}",
                    self.total_prompt_tokens, self.total_completion_tokens,
                    (self.total_completion_tokens as f64) * 0.000_000_59
                ));
                SlashResult::Handled
            }
            "/sessions" | "/resume" => {
                // call session::list_sessions, render a numbered picker
                SlashResult::Handled
            }
            _ => SlashResult::NotASlash,
        }
    }
}

const HELP_TEXT: &str = "\
Volt Agent — slash commands
  /help              Show this help
  /clear             Clear conversation history (keeps session)
  /compact           Compress older messages to fit context
  /model [name]      Show or switch the active LLM
  /status            Model, mode, message count, session id
  /cost              Show cumulative token usage + estimate
  /sessions          List recent sessions; /resume <n> to switch
  /resume <n>        Resume session number n from /sessions
  /tools [filter]    List registered tools (filter by name substring)
  /init              Index the current directory (AGENTS.md, SOUL.md, etc.)
  /permissions       Show or set the approval policy
  /plan              Enter plan mode (read-only, propose before execute)
  /quit              Exit Volt";
```

**Important:** the existing `match` for `/quit` at `tui.rs:205-207` is a sync `&str` comparison and doesn't need to be async — but switching to a dedicated method makes the rest easy to extend.

---

### 4. Replace raw `stdin.read_line` first-run wizard with `inquire`  ⭐ Quick win

**Impact:** High (first impression matters)
**Effort:** 4–6 hours
**Files:** `src/config.rs:80-323`, `Cargo.toml`

**Why it matters:** `config.rs:107-323` uses `println!("║ ║ ║")` ASCII boxes and `read_line()` with numbered choices. Users from Codex expect arrow-key selection and **masked input for API keys** (no one wants their GROQ key in scrollback).

**Implementation:**

```toml
# Cargo.toml
inquire = "0.7"
```

```rust
// src/config.rs (replace sections 107-247)
use inquire::{Select, Text, Password, Confirm};

pub fn first_run_wizard() -> bool {
    if project_config_path().exists() { return false; }
    let has_llm = std::env::var("LLM_MODEL").is_ok() /* ... */;
    let has_db = std::env::var("DATABASE_URL").is_ok();
    if has_llm && has_db { return false; }
    if !std::io::stdin().is_tty() { return false; }

    println!("\n  Volt — first-time setup\n");

    let providers = vec!["Ollama (local)", "Groq", "OpenAI", "Anthropic", "NVIDIA NIM"];
    let choice = Select::new("LLM provider", providers)
        .with_help_message("↑↓ to choose, Enter to confirm")
        .prompt()
        .unwrap_or("Ollama (local)");

    let model = Text::new("Model")
        .with_default("llama-3.1-8b-instant")
        .prompt()
        .unwrap();

    let api_key = if choice != "Ollama (local)" {
        Some(Password::new(&format!("{} API key", choice))
            .without_confirmation()
            .prompt()
            .unwrap_or_default())
    } else { None };

    let emb = Select::new("Embedding provider", vec!["Ollama (local)", "NVIDIA NIM"])
        .prompt()
        .unwrap_or("Ollama (local)");

    let db_url = Text::new("DATABASE_URL")
        .with_default("postgres://volt:volt@localhost:5432/volt")
        .with_help_message("Press Enter for default; must be reachable for memory")
        .prompt()
        .unwrap_or_else(|_| "postgres://volt:volt@localhost:5432/volt".into());

    let _ = Confirm::new("Write config to .volt/config.toml?").prompt();

    // ... write the .toml as before
}
```

Also: replace `agent/run.rs:843-853` (the tool approval prompt) with the same `inquire::Confirm::new("Allow tool '{}' with args {:?}?").prompt()` so the TUI prompt and the standalone prompt look consistent.

---

### 5. Add `AGENTS.md` injection to the system prompt  ⭐ Quick win

**Impact:** High (this is how Codex and Claude Code win loyalty)
**Effort:** 2–3 hours
**Files:** `src/agent/prompt.rs:5-44`, possibly `src/commands/agent_tui.rs:55-57`

**Why it matters:** Codex reads `AGENTS.md` and **injects it directly into the system prompt**. Volt's `prompt.rs` currently only loads `SOUL.md` / `MEMORY.md` / `USER.md` from the workspace. `AGENTS.md` is seeded into the `ContextStore` as `ContextKind::Policy` (see `worker.rs:374`) and is only RAG-retrieved — meaning small models often don't see it. The first 5–10 minutes of using a new repo is exactly when the agent needs `AGENTS.md` most.

**Implementation:**

```rust
// src/agent/prompt.rs — add after USER.md block
let agents_path = ws.join("AGENTS.md");
if agents_path.exists() {
    if let Ok(content) = std::fs::read_to_string(&agents_path) {
        // Truncate to ~4k chars to leave room for tool definitions
        let truncated = if content.len() > 4096 {
            format!("{}\n\n[…AGENTS.md truncated at 4 KB; full content stored in RAG]", &content[..4096])
        } else { content };
        parts.push(format!("## Project Conventions (from AGENTS.md)\n{}", truncated));
    }
}
```

**Why truncate:** system prompts with 12K+ tokens starve the context window. Codex does the same — full `AGENTS.md` is still available via the RAG store, but the **headlines** always reach the model.

---

### 6. Add a token / cost / duration HUD footer to the TUI  ⭐ Quick win

**Impact:** High (visible to user every turn)
**Effort:** 4–6 hours
**Files:** `src/tui.rs:256-265` (`render`)

**Why it matters:** `models.rs:107` already tracks `total_completion_tokens` and `models.rs:103-109` has `total_prompt_tokens`. The TUI just doesn't display them. Codex has a 1-line footer like `↑ 1.2k ↓ 432  •  $0.0003  •  3.4s  •  /help`.

**Implementation:**

```rust
// src/tui.rs — extend the constraints
let chunks = Layout::default()
    .direction(Direction::Vertical)
    .constraints([
        Constraint::Min(3),     // messages
        Constraint::Length(1),  // HUD footer  ← NEW
        Constraint::Length(3),  // input
    ])
    .split(area);

self.render_messages(f, chunks[0]);
self.render_hud(f, chunks[1]);           // ← NEW
self.render_input(f, chunks[2]);

fn render_hud(&self, f: &mut Frame, area: Rect) {
    let cost = (self.total_completion_tokens as f64) * 0.000_000_59;  // Groq default
    let text = format!(
        " ↑ {}{} tok  ↓ {}{} tok  ·  ~${:.4}  ·  {:.1}s  ·  /help for commands",
        abbreviate(self.total_prompt_tokens),
        abbreviate(self.total_completion_tokens),
        self.last_turn_ms as f64 / 1000.0,
    );
    let p = Paragraph::new(text)
        .style(Style::default().fg(Color::DarkGray))
        .alignment(ratatui::layout::Alignment::Right);
    f.render_widget(p, area);
}

fn abbreviate(n: u64) -> String {
    if n < 1000 { n.to_string() }
    else if n < 1_000_000 { format!("{:.1}k", n as f64 / 1000.0) }
    else { format!("{:.1}m", n as f64 / 1_000_000.0) }
}
```

You'll also need to feed `last_turn_ms` and the token counts from the agent's state into the `TuiChat` struct — easy: just `agent.state().lock().await.total_prompt_tokens`.

---

### 7. Upgrade the TUI line editor with `reedline`  ⭐⭐ Medium effort

**Impact:** Highest
**Effort:** 1–2 days
**Files:** `src/tui.rs` (the 100-line `handle_key_event` at lines 198-254)

**Why it matters:** The current line editor is a 50-line `match` on `KeyCode::Char(c)`. It has no:
- Command history (Up/Down only scrolls the *chat*, not the input)
- Multiline input (no `\`-continuation, no Shift-Enter)
- Kill ring (no Ctrl-W, Ctrl-U, Ctrl-K, Ctrl-Y)
- Word movement (no Ctrl-A, Ctrl-E, Alt-B/F)
- Bracketed paste (paste of multiline code is broken)
- Tab completion

**Claude Code and Codex both use reedline or rustyline internally.** Rust's `reedline` crate is purpose-built for this. Adding it gets you all of the above for ~150 lines of glue.

**Implementation:**

```toml
# Cargo.toml
reedline = "0.38"
```

```rust
// src/tui.rs — replace the entire key handling with:
use reedline::{Reedline, ReedlineEvent, EditCommand, Signal};
use reedline::{FileBackedHistory, ReedlineMenu};

pub async fn run(agent: &Agent) -> anyhow::Result<()> {
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let mut terminal = ratatui::Terminal::new(ratatui::backend::CrosstermBackend::new(stdout()))?;

    // Persist history to ~/.local/share/volt/history.txt
    let history_path = dirs::data_dir()
        .unwrap_or_default()
        .join("volt")
        .join("history.txt");
    let _ = std::fs::create_dir_all(history_path.parent().unwrap());

    let history = Box::new(FileBackedHistory::new(1000, history_path).unwrap());
    let mut editor = Reedline::create()
        .with_history(history)
        .with_menu(CommandMenu::default())  // Ctrl-R reverse search
        // Optional: .with_edit_mode(EditMode::Vi)  if user has VISUAL/EDITOR=vi
        ;

    loop {
        let sig = editor.read_line(&prompt);   // prompt is " ▌ "
        match sig {
            Signal::Success(buf) => {
                if buf.trim() == "/quit" { break; }
                if buf.trim().is_empty() { continue; }
                self.submit(buf).await;          // dispatch to agent
            }
            Signal::CtrlD | Signal::CtrlC => break,
            Signal::CtrlL => { terminal.clear()?; }
            _ => {}
        }
    }
    Ok(())
}
```

**What you get for free from reedline:**
- Ctrl-R reverse search across history
- Up/Down history (independent of chat scroll)
- Ctrl-A / Ctrl-E / Ctrl-W / Ctrl-U / Ctrl-K
- Multiline input with proper paste handling
- Optional Vi mode (`VISUAL=vi`)
- Optional Emacs mode (default)

---

### 8. Add a `--print` / `--json` non-interactive mode to `agent-run`  ⭐ Quick win

**Impact:** High (every CI / script user wants this)
**Effort:** 3–5 hours
**Files:** `src/main.rs:53-90` (`AgentRun` subcommand), `src/commands/agent_run.rs:248-271`

**Why it matters:** Claude Code has `--print` for piping into shell. Volt's `agent-run` already exits with the result printed to stdout, but it's mixed with `eprintln!` chatter (`[mode] balanced`, `[session] created new session`, `[blueprint] loaded …`). For scripting, you want a single `--json` flag that emits one JSON envelope and **suppresses all eprintln output** (or routes to log file).

**Implementation:**

```rust
// src/main.rs — add to AgentRun variant
#[arg(long)]
json: bool,
#[arg(long, short = 'p', help = "Print response to stdout, suppress progress chatter")]
print: bool,
```

```rust
// src/commands/agent_run.rs — gate the eprintln calls
fn log(level: LogLevel, msg: &str) {
    if options.json { return; }                       // suppress in --json mode
    if options.print && level < LogLevel::Result { return; }  // print mode keeps only final result
    eprintln!("[{}] {}", level, msg);
}
```

Output:
```bash
$ volt agent-run --input "what is 2+2" --print
4

$ volt agent-run --input "compute fib(10)" --json
{
  "session_id": "9b4f...",
  "model": "llama-3.1-8b-instant",
  "tool_calls": [{"name": "bash", "args": {"command": "python -c 'print(55)'"}}],
  "result": "55",
  "prompt_tokens": 1240,
  "completion_tokens": 47,
  "duration_ms": 3420,
  "cost_estimate_usd": 0.000027
}
```

---

### 9. `volt update` self-upgrade + `volt doctor` health check  ⭐⭐ Medium effort

**Impact:** Medium (quality-of-life)
**Effort:** 1–2 days combined
**Files:** `src/main.rs` (new subcommands)

**Why it matters:** Codex has `codex update` (picks latest GitHub release, downloads binary, replaces in place). Volt ships releases but has no in-CLI upgrade path. `codex doctor` (and a new `volt doctor`) is a health-check command that prints:
- Detected platform + Rust version
- API keys present (masked)
- Database reachable?
- Embedder model loaded?
- ONNX runtime version + selected Execution Provider
- Active permission policies
- Disk space
- Last 5 tool failures with error codes

**Implementation sketch:**

```rust
// src/main.rs
#[derive(Subcommand)]
enum Commands {
    /// Check environment health and configuration
    Doctor,
    /// Self-upgrade to latest GitHub release
    Update {
        /// Specific version to install (default: latest)
        #[arg(long)]
        version: Option<String>,
    },
    // ...
}
```

```rust
// src/commands/doctor.rs
pub async fn run() -> anyhow::Result<()> {
    println!("VOLT Doctor — version {}, platform {}", env!("CARGO_PKG_VERSION"), std::env::consts::OS);
    println!();

    // API keys (mask all but last 4)
    for k in &["GROQ_API_KEY", "OPENAI_API_KEY", "ANTHROPIC_API_KEY", "NVIDIA_API_KEY", "OLLAMA_API_KEY", "HF_TOKEN"] {
        let v = std::env::var(k).unwrap_or_default();
        let status = if v.is_empty() { "✗ not set" } else { "✓ set" };
        let masked = if v.len() > 8 { format!("…{}", &v[v.len()-4..]) } else { "***".into() };
        println!("  {} {:<20} {}", status, k, masked);
    }
    println!();

    // Database
    match db::connect(&settings.database_url).await {
        Ok(_) => println!("  ✓ PostgreSQL reachable at {}", redact_url(&settings.database_url)),
        Err(e) => println!("  ✗ Database unreachable: {}", e),
    }

    // ONNX / embedder
    // ... etc

    Ok(())
}
```

**`volt update`:** Use `self_update` crate, or just `reqwest::get` to GitHub releases API + atomic file replace. Trivial.

---

### 10. `volt init` (project setup) + auto-load `AGENTS.md` / `SOUL.md` from CWD  ⭐⭐ Medium effort

**Impact:** High (matches `codex init` / `claude init` flow)
**Effort:** 1 day
**Files:** new `src/commands/init.rs`, `src/worker.rs:371-407`, `src/agent/prompt.rs`

**Why it matters:** Both Claude Code and Codex have a `init` command that walks the current project and writes a starter `AGENTS.md` / `SOUL.md` / `CLAUDE.md` with inferred build/test commands, project structure, etc. Volt's `first_run_wizard` only does global config, not per-project. Combined with **#5 above** (load `AGENTS.md` into the system prompt), this closes the "first 5 minutes in a new repo" loop that defines modern CLI UX.

**Implementation:**

```rust
// src/commands/init.rs
pub async fn run(yes: bool) -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;
    println!("Indexing project at {}", cwd.display());

    // 1. Detect project type
    let toml_path = cwd.join("Cargo.toml");
    let pkg_json = cwd.join("package.json");
    let pyproject = cwd.join("pyproject.toml");
    let project_type = if toml_path.exists() { "rust" }
        else if pkg_json.exists() { "node" }
        else if pyproject.exists() { "python" }
        else { "unknown" };

    // 2. Walk for AGENTS.md, CLAUDE.md, SOUL.md, MEMORY.md
    // 3. If AGENTS.md missing, scaffold one from heuristics

    let scaffold = match project_type {
        "rust" => include_str!("../../templates/AGENTS.rust.md"),
        "node" => include_str!("../../templates/AGENTS.node.md"),
        "python" => include_str!("../../templates/AGENTS.python.md"),
        _ => include_str!("../../templates/AGENTS.generic.md"),
    };

    let target = cwd.join("AGENTS.md");
    if target.exists() && !yes {
        let overwrite = Confirm::new("AGENTS.md already exists. Overwrite?")
            .with_default(false).prompt()?;
        if !overwrite { return Ok(()); }
    }
    std::fs::write(&target, scaffold)?;
    println!("✓ Wrote {}", target.display());

    // 4. Offer to seed into context store
    println!("✓ Project indexed. Next: `volt agent-run --input \"...\"` will use AGENTS.md.");
    Ok(())
}
```

The `AGENTS.md` from item #5 is then automatically picked up.

---

## Part 4 — Effort Matrix

### Quick wins (1–2 days each, ship one per week)

| # | Item | Effort | Files | Net new deps |
|---|---|---|---|---|
| 1 | `volt completion <shell>` | 2–4h | main.rs, Cargo.toml | `clap_complete` |
| 2 | Stream tokens into TUI | 4–8h | agent_tui.rs, tui.rs | — |
| 3 | Slash commands in TUI | 4–8h | tui.rs | — |
| 4 | `inquire`-based first-run wizard | 4–6h | config.rs | `inquire` |
| 5 | Inject `AGENTS.md` into system prompt | 2–3h | agent/prompt.rs | — |
| 6 | Token/cost HUD footer in TUI | 4–6h | tui.rs | — |
| 8 | `--print` / `--json` flags on `agent-run` | 3–5h | main.rs, agent_run.rs | — |

**Total quick wins:** ~3 days of work for a 10× UX improvement.

### Medium effort (1–2 weeks each)

| # | Item | Effort | Files | Net new deps |
|---|---|---|---|---|
| 7 | `reedline` upgrade for TUI input | 1–2d | tui.rs | `reedline` |
| 9 | `volt doctor` + `volt update` | 1–2d | main.rs, new doctor.rs | `self_update` (optional) |
| 10 | `volt init` + `AGENTS.md` scaffolding | 1d | new init.rs | `inquire` (already) |
| 11 | Syntax-highlighted tool calls in TUI | 1d | tui.rs | — (`syntect` already in deps!) |
| 12 | Plan mode (`/plan` — read-only agent) | 2d | agent.rs, prompt.rs | — |
| 13 | `volt resume <n>` and `volt sessions` (TUI launches) | 1d | main.rs, session.rs | — |
| 14 | Per-tool TUI approval widget (replacing `agent/run.rs:843-853` stdin prompt) | 2d | tui.rs, agent/run.rs | — |

### Large effort (1+ month)

| # | Item | Effort | Why it matters |
|---|---|---|---|
| 15 | Hook system (PreToolUse/PostToolUse, configurable shell scripts) | 2–4 weeks | Power-user feature; matches Claude Code |
| 16 | `volt serve` daemon + Web UI for managing sessions | 1+ month | Complements CLI; out of scope for pure CLI audit |
| 17 | Worktree-per-session mode (`--worktree`) | 1–2 weeks | Codex feature; nice-to-have |
| 18 | Subagent spawning from inside TUI (`/delegate <task>`) | 2 weeks | Already have `delegate` tool; needs TUI integration |
| 19 | Conversation fork (`/fork <n>`) | 1 week | Already have session storage; just need UI |

---

## Part 5 — Existing Crates to Leverage (already in `Cargo.toml`)

| Crate | Version | Already in deps? | What to use it for |
|---|---|---|---|
| `clap` | "4" | ✅ (with `derive` + `env`) | All CLI definitions; just add `clap_complete` for completions |
| `ratatui` | "0.29" | ✅ | TUI rendering; underused (only 338 lines of TUI code) |
| `crossterm` | "0.28" | ✅ | Already used; needed for `inquire` integration |
| `syntect` | "5" | ✅ (with `default-syntaxes`, `default-themes`, `regex-fancy`) | **Syntax highlighting — but currently UNUSED.** Item #11 should leverage this. |
| `anyhow` | "1" | ✅ | Already used; perfect for error chains in TUI |
| `tokio` | "1" | ✅ (with `sync`) | mpsc channels for token streaming (item #2) |
| `chrono` | "0.4" | ✅ | Timestamp display in HUD |
| `serde` + `serde_json` | "1" | ✅ | For `--json` mode (item #8) |
| `dotenvy` | "0.15" | ✅ | Already loading .env in main.rs:211 |
| `tiktoken-rs` | "0.11" | ✅ | Could power the cost HUD; `estimate_tokens` exists |
| `url` | "2" | ✅ | Redact URLs in `volt doctor` (item #9) |

### Crates to ADD (lowest-friction wins first)

| Crate | Purpose | Approx cost |
|---|---|---|
| `clap_complete` | Shell completions | 0 deps, ~0 compile time |
| `inquire` | Interactive prompts (arrow keys, masking) | 5 deps, fast |
| `reedline` | Proper line editor (history, multiline, completions) | ~20 deps, ~3s extra compile |
| `indicatif` | Progress bars (long agent runs, embedding downloads) | 0 deps, fast |
| `self_update` | `volt update` | 5 deps |
| `dirs` | Find `~/.local/share` for history file | 0 deps |
| `anstyle` | ANSI styling for output (cleaner than `eprintln!("\x1b[33m…")`) | 0 deps |

---

## Part 6 — Concrete Code for Top 3 Recommendations

### Top 1: `volt completion <shell>`

```rust
// Cargo.toml
clap-complete = "4"

// src/main.rs (add to Commands enum)
use clap_complete::Shell;

#[derive(Subcommand, Debug)]
enum Commands {
    /// Generate shell completions and write to stdout or file
    Completion {
        /// bash, zsh, fish, powershell, elvish
        shell: Shell,
        /// Write to file (e.g. ~/.local/share/bash-completion/completions/volt)
        #[arg(long, short = 'o')]
        out: Option<PathBuf>,
    },
    // ... existing subcommands ...
}

// In match cli.command { ... } block:
Commands::Completion { shell, out } => {
    use clap::CommandFactory;
    let mut cmd = Cli::command();
    let bin = cmd.get_name().to_string();
    let mut buf: Vec<u8> = Vec::new();
    clap_complete::generate(shell, &mut cmd, bin, &mut buf);
    match out {
        Some(p) => {
            if let Some(parent) = p.parent() { std::fs::create_dir_all(parent)?; }
            std::fs::write(&p, &buf)?;
            eprintln!("wrote {} ({} bytes)", p.display(), buf.len());
        }
        None => {
            use std::io::Write;
            std::io::stdout().lock().write_all(&buf)?;
        }
    }
    Ok(())
}
```

User-facing install instructions (add to README):
```bash
# bash
volt completion bash > ~/.local/share/bash-completion/completions/volt
# zsh — first entry in ~/.zshrc:
autoload -U compinit; compinit
volt completion zsh > "${fpath[1]}/_volt"
# fish
volt completion fish > ~/.config/fish/completions/volt.fish
# powershell
volt completion powershell | Out-String | Invoke-Expression
```

---

### Top 2: Stream tokens into the TUI

The minimal version (10 lines of glue code):

```rust
// src/commands/agent_tui.rs — replace lines 55-57
let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
let live_stream: Arc<tokio::sync::Mutex<String>> =
    Arc::new(tokio::sync::Mutex::new(String::new()));

let mut agent = Agent::new(config, provider, tools.clone())
    .await
    .with_workspace(std::env::current_dir().unwrap_or_default())
    .with_stream(Arc::new(move |token| {
        let _ = tx.send(token.to_string());
    }));

// Drain the channel into the live_stream buffer (cheap because unbounded)
let drain_buf = live_stream.clone();
tokio::spawn(async move {
    while let Some(token) = rx.recv().await {
        drain_buf.lock().await.push_str(&token);
    }
});
```

```rust
// src/tui.rs — in TuiChat
struct TuiChat {
    // ... existing fields ...
    live_stream: Arc<tokio::sync::Mutex<String>>,
}

impl TuiChat {
    pub fn new(cancel: CancelToken, live_stream: Arc<tokio::sync::Mutex<String>>) -> Self {
        Self { /* ... */ live_stream, /* ... */ }
    }

    // In render_messages, when showing the last "assistant" message,
    // if is_thinking, append the live_stream content.
    fn render_messages(&self, f: &mut Frame, area: Rect) {
        // ... existing loop building ListItem ...
        // but read live_stream with try_lock — never block the render thread
        let live = if self.is_thinking {
            self.live_stream.try_lock().map(|g| g.clone()).unwrap_or_default()
        } else { String::new() };
        // ... append `live` to the last assistant ListItem if is_thinking ...
    }
}
```

```rust
// src/commands/agent_tui.rs — after agent.run() completes, clear the stream
match result {
    Ok(output) => {
        *live_stream.lock().await = String::new();  // clear
        self.add_message("assistant", &output);
    }
    Err(e) => { /* ... */ }
}
```

---

### Top 3: Slash commands

```rust
// src/tui.rs — add this entire block
#[derive(Debug, Clone, PartialEq)]
enum SlashResult {
    Quit,
    Handled,
    NotASlash,
}

const HELP_TEXT: &str = "\
Volt Agent — slash commands

  Conversation:
    /clear             Clear visible messages (keeps session)
    /compact           Compress older messages to fit context window
    /sessions          List recent sessions
    /resume <n|name>   Resume session N from /sessions

  Model:
    /model             Show current model
    /model <name>      Switch to model (e.g. qwen/qwen3-32b)
    /mode              Show current mode (precision/balanced/autonomous)
    /mode <m>          Switch mode

  Status:
    /status            Model, mode, session id, message count
    /cost              Token usage + estimated cost
    /tools [filter]    List registered tools (filter by name substring)
    /init              Index current directory (AGENTS.md, SOUL.md, …)

  Permissions:
    /permissions       Show current approval policy
    /allow             Toggle autonomous mode (--allow flag)
    /plan              Enter read-only plan mode (propose before execute)

  Other:
    /help              This help
    /quit              Exit Volt
";

impl TuiChat {
    async fn execute_slash(&mut self, raw: &str) -> SlashResult {
        let mut parts = raw.split_whitespace();
        let cmd = parts.next().unwrap_or("");
        let arg = parts.collect::<Vec<_>>().join(" ");

        match cmd {
            "/quit" | "/exit" | "/q" => SlashResult::Quit,

            "/help" | "/?" => {
                self.add_message("system", HELP_TEXT);
                SlashResult::Handled
            }
            "/clear" => {
                self.messages.clear();
                self.add_message("system", "(conversation cleared)");
                SlashResult::Handled
            }
            "/status" => {
                let state_msg = format!(
                    "model: {}\nmode: {}\nsession: {}\nmsgs: {}",
                    self.model_name, self.mode, self.session_id, self.messages.len()
                );
                self.add_message("system", &state_msg);
                SlashResult::Handled
            }
            "/cost" | "/tokens" => {
                let cost_est =
                    (self.total_completion_tokens as f64) * 0.000_000_59;
                let msg = format!(
                    "↑ {} prompt · ↓ {} completion\n~${:.4} USD (Groq rate)",
                    self.total_prompt_tokens,
                    self.total_completion_tokens,
                    cost_est
                );
                self.add_message("system", &msg);
                SlashResult::Handled
            }
            "/model" => {
                if arg.is_empty() {
                    self.add_message("system", &format!("current model: {}", self.model_name));
                } else {
                    self.model_name = arg.clone();
                    self.add_message("system", &format!("model → {}", arg));
                }
                SlashResult::Handled
            }
            "/mode" => {
                if arg.is_empty() {
                    self.add_message("system", &format!("current mode: {}", self.mode));
                } else {
                    self.mode = arg.clone();
                    self.add_message("system", &format!("mode → {}", arg));
                }
                SlashResult::Handled
            }
            "/sessions" | "/resume" => {
                // Hook this up to session::list_sessions
                self.add_message("system", "loading sessions…");
                SlashResult::Handled
            }
            "/init" => {
                self.add_message("system", "Indexing workspace — this is a no-op stub; see 'volt init'");
                SlashResult::Handled
            }
            "/allow" => {
                self.allow_all = !self.allow_all;
                self.add_message("system", &format!(
                    "autonomous mode: {}",
                    if self.allow_all { "ON" } else { "OFF" }
                ));
                SlashResult::Handled
            }
            "/plan" => {
                self.add_message("system", "plan mode requested — next turn will be read-only");
                SlashResult::Handled
            }
            _ => {
                self.add_message("system", &format!(
                    "unknown slash command: {}. Try /help.", cmd
                ));
                SlashResult::Handled
            }
        }
    }
}
```

```rust
// src/tui.rs — modify handle_key_event for KeyCode::Enter
KeyCode::Enter => {
    let input = self.input.trim().to_string();
    if input.is_empty() { return false; }
    if input.starts_with('/') {
        match self.execute_slash(&input).await {
            SlashResult::Quit => return true,
            SlashResult::Handled => {
                self.input.clear();
                self.cursor_pos = 0;
                return false;
            }
            SlashResult::NotASlash => { /* fall through */ }
        }
    }
    self.add_message("user", &input);
    self.input.clear();
    self.cursor_pos = 0;
    self.is_thinking = true;
    self.stream_buffer.clear();
    false
}
```

---

## Part 7 — Recommendation Roadmap

A pragmatic 4-week plan that gets VOLT to **Codex-class UX**:

**Week 1 — Polish foundations** (1-day items, 5 days of work)
- Day 1: `clap_complete` + `volt completion` (item #1)
- Day 2: `inquire` upgrade of first-run wizard (item #4)
- Day 3: `--print` / `--json` for `agent-run` (item #8)
- Day 4: AGENTS.md injected into system prompt (item #5)
- Day 5: Token/cost HUD footer (item #6)

**Week 2 — TUI v2**
- Day 1–3: Streaming tokens into the TUI (item #2)
- Day 4–5: Slash commands (item #3)

**Week 3 — TUI v3**
- Day 1–2: `reedline` upgrade (item #7)
- Day 3: Syntax highlighting with `syntect` (item #11)
- Day 4–5: Per-tool TUI approval widget (item #14)

**Week 4 — Project & health**
- Day 1–2: `volt init` (item #10)
- Day 3: `volt doctor` (part of item #9)
- Day 4: `volt update` (rest of item #9)
- Day 5: `volt resume <n>` (item #13), README updates

**By end of month 2:** Plan mode (item #12), subagent spawning (item #18)

**By end of month 3:** Hook system (item #15) — this is the only feature that requires deep changes to `agent/run.rs` because the agent loop needs to be interrupted at PreToolUse/PostToolUse boundaries.

---

## Part 8 — What's Already Good (Don't Break)

A few things to **preserve** as you polish:

1. **`AutoSeedWorker` design** (`src/worker.rs`) — background MPSC channel seeding is elegant. Don't rewrite it.
2. **`attenuation` module** — the per-tool permission system is more granular than Claude Code's. Document it better; don't replace it.
3. **Blueprint system** — 67 production blueprints is a unique asset. The `auto-blueprint` flag is the killer feature.
4. **DAG orchestrator** — neither Claude Code nor Codex have this. Surface it more prominently (a `volt dag` subcommand with a higher-level syntax would be nice).
5. **First-run wizard already exists** (`config.rs:80`) — it just needs to be `inquire`-powered. Don't delete and re-write; refactor.
6. **JSON output for workflow** (`workflow.rs:53-64`) — `volt workflow --pattern dag --agents ... --tasks ...` already emits JSON. Mirror this pattern for `agent-run --json` (item #8).
7. **Skill importer** (`src/skills/importer.rs`) — VOLT can already import from Claude / Cursor / Copilot / OpenCode. The README should shout about this.

---

## Implementation Tracker

This section tracks actual implementation work. Each item gets a ✅ when shipped, a 🔄 when in progress, and a ⬜ when not started.

| # | Item | Status | Commit | Notes |
|---|---|---|---|---|
| 1 | `volt completion <shell>` | ✅ | `6aa6eb6` | `clap_complete = 4` added; bash/zsh/fish/powershell/elvish supported; stdout or `--out <file>` |
| 2 | Stream tokens into TUI | ⬜ | — | Single biggest perceived-quality win |
| 3 | Slash commands in TUI | ⬜ | — | `/help`, `/model`, `/clear`, `/cost`, etc. |
| 4 | `inquire`-based first-run wizard | ⬜ | — | API key masking + arrow-key nav |
| 5 | Inject `AGENTS.md` into system prompt | ⬜ | — | 2-3h change in `prompt.rs` |
| 6 | Token/cost HUD footer in TUI | ⬜ | — | Wire `total_prompt_tokens` to TUI render |
| 7 | `reedline` upgrade for TUI input | ⬜ | — | History, multiline, kill ring |
| 8 | `--print` / `--json` flags on `agent-run` | ⬜ | — | CI/script-friendly output |
| 9 | `volt doctor` + `volt update` | ⬜ | — | Health check + self-upgrade |
| 10 | `volt init` + `AGENTS.md` scaffolding | ⬜ | — | Project-aware first-run |
| 11 | Syntax-highlighted tool calls in TUI | ⬜ | — | `syntect` already in deps |
| 12 | Plan mode (`/plan` — read-only agent) | ⬜ | — | |
| 13 | `volt resume <n>` and `volt sessions` | ⬜ | — | |
| 14 | Per-tool TUI approval widget | ⬜ | — | Replace stdin prompt with TUI widget |
| 15 | Hook system (PreToolUse/PostToolUse) | ⬜ | — | Large effort |
| 16 | `volt serve` daemon + Web UI | ⬜ | — | Out of scope for pure CLI |
| 17 | Worktree-per-session mode | ⬜ | — | |
| 18 | Subagent spawning from TUI | ⬜ | — | |
| 19 | Conversation fork | ⬜ | — | |

### Quick Wins Sprint Plan

To get to "Codex-class UX" in one week:

| Day | Item | New dep | Risk |
|---|---|---|---|
| 1 | ✅ #1: `volt completion <shell>` (`6aa6eb6`) | `clap_complete` | Shipped — bash/zsh/fish/powershell/elvish |
| 2 | #4: `inquire` first-run wizard | `inquire` | Low — UI-only refactor |
| 3 | #8: `--print` / `--json` flags | none | Low — output-only change |
| 4 | #5: AGENTS.md into system prompt | none | None |
| 5 | #6: Token/cost HUD footer | none | Low — TUI render change |

Then:

| Day | Item | Risk |
|---|---|---|
| 6–8 | #2: Stream tokens into TUI | Medium — async channel plumbing |
| 9–10 | #3: Slash commands | Low — pure TUI logic |

---

## Sources

- [Claude Code CLI Reference](https://code.claude.com/docs/en/cli-reference)
- [Claude Code Best Practices](https://www.anthropic.com/engineering/claude-code-best-practices)
- [Shipyard Claude Code Cheatsheet](https://shipyard.build/blog/claude-code-cheat-sheet/)
- [Codex CLI Features](https://developers.openai.com/codex/cli/features)
- [Codex Config Basics](https://developers.openai.com/codex/config-basic)
- [Rust CLI/TUI Developer Skill (clap + inquire + ratatui)](https://playbooks.com/skills/bahayonghang/my-claude-code-settings/rust-cli-tui-developer)
- [Ratatui CLI Arguments Recipe](https://ratatui.rs/recipes/apps/cli-arguments/)
- [clap-repl crate](https://lib.rs/crates/clap-repl)
