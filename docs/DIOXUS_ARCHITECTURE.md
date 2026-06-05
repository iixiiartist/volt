# Dioxus 0.7 Architecture Guide for Volt

> This guide covers everything you need to add a Dioxus-based UI to an existing Rust project, specifically tailored for Volt's architecture (agent chat, tool registry, session management).

---

## 1. Project Setup

### 1.1. Adding Dioxus to an Existing Rust Project

In your existing project, Dioxus is just another dependency. No special CLI init is required.

**`Cargo.toml` additions:**

```toml
[dependencies]
# Core Dioxus framework
dioxus = { version = "0.7.0", features = ["desktop"] }

# Desktop renderer (tauri-style native window)
dioxus-desktop = "0.7.0"

# Router (separate crate)
dioxus-router = "0.7.0"

# If targeting web instead:
# dioxus-web = "0.7.0"
# wasm-bindgen = "0.2"

# Optional: for HTTP / SSE streaming
reqwest = { version = "0.12", features = ["json", "stream"] }
tokio = { version = "1.40", features = ["full"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
```

### 1.2. Desktop vs Web Renderer

| Feature | Desktop | Web |
|---------|---------|-----|
| Window | Native window (no browser needed) | Runs in browser |
| Requires `wasm32` target | No | Yes |
| File system access | Yes (native) | Limited (WASM) |
| Local SQLite/DB | Yes | No (needs API server) |
| Ideal for | Local-first tools (Volt!) | Hosted web apps |

**Recommendation for Volt:** Use the **desktop renderer**.
- Volt is a local-first CLI/automation tool
- We need file system, DB, and subprocess access
- Desktop gives a native window with no browser dependency (like Tauri)
- Electron-style architecture with our Rust core running natively

### 1.3. Entry Point

```rust
use dioxus::prelude::*;
use dioxus_desktop::{Config, WindowBuilder};

fn main() {
    // Configure the native window
    let config = Config::new()
        .with_window(
            WindowBuilder::new()
                .with_title("Volt")
                .with_inner_size(dioxus_desktop::LogicalSize::new(1280.0, 800.0))
        );

    dioxus_desktop::launch_cfg(App, config);
}

#[component]
fn App() -> Element {
    rsx! { "Volt UI Loading..." }
}
```

---

## 2. Routing

### 2.1. Defining Routes with `Routable`

Dioxus Router provides type-safe, declarative routing via a derive macro on an enum.

```rust
use dioxus::prelude::*;
use dioxus_router::prelude::*;

// Volt's route tree: /, /chat, /tools, /sessions, /settings
#[derive(Routable, Clone, Debug, PartialEq)]
#[rustfmt::skip]
pub enum Route {
    // Top-level pages
    #[route("/")]
    Home {},

    #[route("/chat")]
    Chat {},

    #[route("/tools")]
    Tools {},

    #[route("/sessions")]
    Sessions {},

    #[route("/settings")]
    Settings {},

    // Session detail with a dynamic segment
    #[route("/sessions/:session_id")]
    SessionDetail { session_id: String },

    // Catch-all for 404s
    #[route("/404")]
    NotFound {},
}
```

### 2.2. Router Setup in App

```rust
#[component]
fn App() -> Element {
    rsx! {
        // Router wraps the entire app; current route auto-rendered via Outlet
        Router::<Route> {}
    }
}
```

### 2.3. Navigation (Programmatic + Links)

```rust
use dioxus_router::prelude::*;
use crate::Route; // our Route enum

// In RSX: Link component (renders <a>, no full page reload)
rsx! {
    Link { to: Route::Chat {}, "Chat" }
    Link { to: Route::Tools {}, "Tools" }
}

// In Rust code: use_navigator hook
#[component]
fn SomeComponent() -> Element {
    let nav = use_navigator();

    let go_to_chat = move |_| {
        nav.push(Route::Chat {});
    };

    rsx! {
        button { onclick: go_to_chat, "Go to Chat" }
    }
}
```

### 2.4. Nested Routes with Layouts

Use `#[nest("/path")]` + `#[end_nest]` to group routes under a shared layout component.

```rust
#[derive(Routable, Clone, Debug, PartialEq)]
#[rustfmt::skip]
pub enum Route {
    #[route("/")]
    Home {},

    // Everything under /app shares the AppLayout (sidebar + header)
    #[nest("/app")]
        #[route("/")]
        Dashboard {},

        #[route("/chat")]
        Chat {},

        #[route("/tools")]
        Tools {},

        #[route("/sessions")]
        Sessions {},

        #[route("/settings")]
        Settings {},
    #[end_nest]
}

// The layout component uses `Outlet` to render the matched child route
#[component]
fn AppLayout() -> Element {
    rsx! {
        div { class: "flex h-screen",
            // Sidebar Navigation
            nav { class: "w-64 bg-gray-900 text-white p-4",
                Link { to: Route::Dashboard {}, "Dashboard" }
                Link { to: Route::Chat {}, "Chat" }
                Link { to: Route::Tools {}, "Tools" }
                Link { to: Route::Sessions {}, "Sessions" }
                Link { to: Route::Settings {}, "Settings" }
            }
            // Main content area: renders the matched child route
            main { class: "flex-1 p-6 overflow-auto",
                Outlet::<Route> {}
            }
        }
    }
}
```

---

## 3. State Management

### 3.1. The Tools: `use_signal`, `use_context`, `use_ref`

| Hook | Use Case | Lifetime |
|------|----------|----------|
| `use_signal(|| T)` | Reactive state that triggers re-render | Component |
| `use_signal` (Global) | `Signal::global(|| T)` -- app-wide state | App |
| `use_context` | Share complex state structs down the tree | Component + children |
| `use_ref(|| T)` | Imperative ref (e.g. DOM, non-reactive storage) | Component |

### 3.2. Global State for "Volt State"

For core Volt state (agent, tool registry, context store), use a **Global Signal** created at module level and accessed via `Signal::global` or `use_context` with a wrapping struct.

```rust
use dioxus::prelude::*;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// 1. Define your application state
// ---------------------------------------------------------------------------
use volt_core::{Agent, ToolRegistry, ContextStore}; // hypothetical

#[derive(Clone)]
pub struct VoltState {
    pub agent: Signal<Agent>,
    pub tool_registry: Signal<ToolRegistry>,
    pub context_store: Signal<ContextStore>,
    pub current_session_id: Signal<Option<String>>,
}

impl VoltState {
    pub fn new() -> Self {
        Self {
            agent: Signal::global(|| Agent::default()),
            tool_registry: Signal::global(|| ToolRegistry::default()),
            context_store: Signal::global(|| ContextStore::default()),
            current_session_id: Signal::global(|| None),
        }
    }
}
// ---------------------------------------------------------------------------
// 2. Provide it once at the root of the app
// ---------------------------------------------------------------------------
#[component]
fn App() -> Element {
    use_hook(|| VoltState::new());

    rsx! {
        Router::<Route> {}
    }
}
```

> **Accessing:** Any component deeper in the tree can call `use_signal` on the global directly, or you can wrap access in helper functions.

### 3.3. Pattern: "Service Layer" via Context

Instead of passing raw signals, create a service struct and provide it via `use_context` for type safety and encapsulation.

```rust
use dioxus::prelude::*;

#[derive(Clone)]
pub struct VoltService {
    pub state: VoltState,
}

impl VoltService {
    pub fn new() -> Self {
        Self { state: VoltState::new() }
    }

    pub fn current_agent(&self) -> Signal<Agent> {
        self.state.agent
    }

    pub fn set_session(&self, id: String) {
        self.state.current_session_id.set(Some(id));
    }
}

#[component]
fn App() -> Element {
    let service = use_signal(|| VoltService::new());
    use_context_provider(|| service);

    rsx! { Router::<Route> {} }
}

// In any child component:
#[component]
fn ChatPage() -> Element {
    let service: Signal<VoltService> = use_context();
    let agent = service.read().current_agent();
    let session_id = service.read().state.current_session_id;

    rsx! {
        div {
            "Agent: {agent.read().name}"
            if let Some(id) = session_id.read().as_ref() {
                " | Session: {id}"
            }
        }
    }
}
```

### 3.4. When to Use What for Volt

| Volt Data | Recommended Storage | Why |
|-----------|---------------------|-----|
| Agent config, tool registry | `Signal::global` or context | Shared across many components |
| Chat messages | `Vec<Signal<Message>>` or `Signal<Vec<Message>>` | Frequently updated list; use `use_signal` |
| Current input text | `use_signal(|| String)` | Local to chat component |
| Settings form state | `use_signal` | Local to settings page |
| Session list | `use_resource` + `Signal` | Async fetch + cache |

---

## 4. Async Patterns

### 4.1. `use_future` -- run async on component mount

```rust
use dioxus::prelude::*;

#[component]
fn SessionsPage() -> Element {
    let sessions = use_signal(|| Vec::<Session>::new());
    let loading = use_signal(|| true);

    // Like React useEffect: runs once, fetches data
    use_future(move || async move {
        // In a real app, call your core API / DB layer
        let fetched = fetch_sessions_from_db().await;
        sessions.set(fetched);
        loading.set(false);
    });

    rsx! {
        if loading() {
            "Loading sessions..."
        } else {
            for session in sessions.iter() {
                div { "{session.name}" }
            }
        }
    }
}
```

### 4.2. `use_resource` -- cached async with auto-cancellation

```rust
use dioxus::prelude::*;

#[component]
fn ToolRegistryPage() -> Element {
    // Automatically re-fetches when dependencies change
    let tools = use_resource(move || async move {
        fetch_tool_registry().await
    });

    // `tools` is a Resource<Vec<Tool>> with .value(), .loading(), etc.
    match tools.value() {
        Some(Ok(data)) => rsx! {
            for tool in data.iter() {
                div { "{tool.name}" }
            }
        },
        Some(Err(e)) => rsx! { "Error: {e}" },
        None => rsx! { "Loading..." },
    }
}
```

### 4.3. Manual `spawn` in Event Handlers

```rust
let mut response_text = use_signal(|| String::new());

rsx! {
    button {
        onclick: move |_| async move {
            // This async block is automatically spawned by Dioxus
            response_text.set("Loading...".into());
            let result = send_message_to_agent("Hello").await;
            response_text.set(result);
        },
        "Send"
    }
}
```

### 4.4. Streaming SSE Events into the UI

For Volt, the most critical async pattern is **streaming agent responses** (SSE).

```rust
use dioxus::prelude::*;
use futures::StreamExt;

#[component]
fn ChatStream() -> Element {
    let mut messages = use_signal(|| Vec::<String>::new());
    let mut is_streaming = use_signal(|| false);

    let start_stream = move |_| async move {
        is_streaming.set(true);

        // Open SSE connection to your local API / core layer
        let mut stream = open_sse_stream("http://localhost:3000/agent/stream").await;

        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(text) => {
                    // Append to the last message or push a new one
                    messages.write().push(text);
                }
                Err(e) => {
                    eprintln!("Stream error: {e}");
                    break;
                }
            }
        }

        is_streaming.set(false);
    };

    rsx! {
        div {
            for msg in messages.iter() {
                p { "{msg}" }
            }
            if is_streaming() {
                span { "Agent is typing..." }
            }
            button { onclick: start_stream, "Start Stream" }
        }
    }
}
```

**Alternative: use `Channel`/`Coroutines`** (if Dioxus provides them) or simply `tokio::spawn` alongside signals.

---

## 5. Component Architecture

### 5.1. Multi-Page App Structure for Volt

```
src/
  webui/
    mod.rs                          # Module entry, exports components
    main.rs                         # Desktop entry point
    router.rs                       # Route enum + layout
    state.rs                        # VoltState, VoltService
    components/
      layout.rs                     # AppLayout (sidebar, header, main)
      nav.rs                        # Sidebar navigation
      header.rs                     # Top bar with session info
    pages/
      home.rs                       # / - Dashboard
      chat.rs                       # /chat - Agent chat interface
      tools.rs                      # /tools - Tool registry viewer
      sessions.rs                   # /sessions - Session list + detail
      settings.rs                   # /settings - Config editor
    hooks/
      use_agent.rs                  # Custom hook for agent state
      use_sse.rs                    # Custom hook for SSE streaming
```

### 5.2. Layout Component

```rust
// src/webui/components/layout.rs
use dioxus::prelude::*;
use crate::webui::router::Route;

#[component]
pub fn AppLayout() -> Element {
    rsx! {
        div { class: "flex h-screen bg-gray-50",
            // Sidebar: always visible
            aside { class: "w-64 bg-gray-900 text-white flex flex-col",
                div { class: "p-4 text-xl font-bold", "Volt" }
                nav { class: "flex-1 p-2 space-y-1",
                    NavLink { route: Route::Chat {}, icon: "💬", label: "Chat" }
                    NavLink { route: Route::Tools {}, icon: "🔧", label: "Tools" }
                    NavLink { route: Route::Sessions {}, icon: "📁", label: "Sessions" }
                    NavLink { route: Route::Settings {}, icon: "⚙️", label: "Settings" }
                }
            }
            // Main content: route outlet
            div { class: "flex-1 flex flex-col overflow-hidden",
                Header {}
                main { class: "flex-1 overflow-auto p-6",
                    Outlet::<Route> {}
                }
            }
        }
    }
}

#[component]
fn NavLink(route: Route, icon: &'static str, label: &'static str) -> Element {
    rsx! {
        Link {
            to: route,
            class: "block px-3 py-2 rounded hover:bg-gray-700",
            "{icon} {label}"
        }
    }
}
```

### 5.3. Conditional Rendering

```rust
rsx! {
    if is_loading() {
        div { class: "spinner", "Loading..." }
    } else if let Some(error) = error_message.read().as_ref() {
        div { class: "text-red-500", "{error}" }
    } else {
        div { "Content here" }
    }
}
```

---

## 6. Component Communication

### 6.1. Summary Table

| Mechanism | Best For | Volt Example |
|-----------|----------|--------------|
| **Props** | Parent -> Child data | `MessageCard { msg: message }` |
| **Signals** | Cross-component reactive state (siblings, deep trees) | `Signal::global` for agent state |
| **Context** | Shared services / dependency injection | `VoltService` provided at root |
| **Callbacks** | Child -> Parent events | `on_delete: EventHandler<String>` |

### 6.2. Example: Passing Data Down via Props

```rust
#[derive(Clone, PartialEq)]
struct ChatMessage {
    role: String,
    content: String,
}

#[component]
fn MessageCard(msg: ChatMessage) -> Element {
    rsx! {
        div { class: "p-2 rounded bg-white shadow",
            strong { "{msg.role}:" }
            p { "{msg.content}" }
        }
    }
}

// Usage:
rsx! {
    for msg in messages.iter() {
        MessageCard { msg: msg.clone() }
    }
}
```

### 6.3. Example: Child -> Parent via Callbacks

```rust
#[component]
fn SessionList(sessions: Vec<Session>, on_select: EventHandler<Session>) -> Element {
    rsx! {
        for session in sessions {
            let session_clone = session.clone();
            div {
                class: "cursor-pointer p-2 hover:bg-gray-100",
                onclick: move |_| on_select.call(session_clone.clone()),
                "{session.name}"
            }
        }
    }
}

// Parent:
rsx! {
    SessionList {
        sessions: sessions(),
        on_select: move |session: Session| {
            selected_session.set(session.id);
        }
    }
}
```

---

## 7. Styling

### 7.1. Options Compared

| Approach | Speed to Build | Run-Time Cost | Best For |
|----------|---------------|---------------|----------|
| **Tailwind CSS** | Very fast | Zero (compiled) | Rapid UI iteration; design system consistency |
| **Inline `style:`** | Fast | Zero | Quick one-off overrides |
| **Separate CSS file** | Medium | Zero | Complex, reusable animations and themes |

### 7.2. Recommended: Tailwind CSS

Dioxus's `class:` attribute works well with Tailwind's utility classes.

Set up `tailwindcss` in your project:

```bash
npm install -D tailwindcss postcss autoprefixer
npx tailwindcss init -p
```

**`tailwind.config.js`:**

```js
/** @type {import('tailwindcss').Config} */
module.exports = {
  content: [
    "./src/**/*.rs",
    "./index.html", // if using web
  ],
  theme: { extend: {} },
  plugins: [],
}
```

**`src/webui/styles.css`:**

```css
@tailwind base;
@tailwind components;
@tailwind utilities;
```

**Import in your main file and use in RSX:**

```rust
// In main.rs or mod.rs
// #[cfg(target_arch = "wasm32")]
// dioxus::web::launch(App);

#[component]
fn ChatPage() -> Element {
    rsx! {
        div { class: "flex flex-col h-full bg-gray-100",
            div { class: "flex-1 overflow-y-auto p-4 space-y-2",
                // Messages here
            }
            div { class: "p-4 bg-white border-t",
                input {
                    class: "w-full px-4 py-2 border rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500",
                    placeholder: "Ask Volt..."
                }
            }
        }
    }
}
```

### 7.3. Inline Styles (for dynamic values)

```rust
rsx! {
    div {
        style: "width: {progress}%; background-color: {color};",
        "Progress: {progress}%"
    }
}
```

---

## 8. Embedding in an Existing Rust Crate

### 8.1. Two Approaches

1. **Subdirectory (`src/webui/`)**
2. **Separate crate (`volt-webui/`)**

| | `src/webui/` | Separate crate |
|---|---|---|
| **Coupling** | Tight (shares `Cargo.toml`) | Loose (via `volt-core` shared lib) |
| **Build Time** | Single cargo build | Separate build, slower initial compile |
| **Code Reuse** | Easy (same `crate::` paths) | Requires extracting shared types to a `core` crate |
| **Deployment** | Single binary | Single binary or separate lib |
| **Best For** | Rapid prototyping, tight integration | Large teams, clean architecture boundaries |

### 8.2. Recommendation for Volt: `src/webui/` Subdirectory

Volt's core and UI are tightly coupled by design (the UI *is* the CLI front-end). Keep the UI in `src/webui/`:

```
volt/
  Cargo.toml            # Main crate: includes dioxus deps + all core deps
  src/
    main.rs             # CLI entry point (subprocess, headless)
    lib.rs              # Core library exports
    webui/
      mod.rs            # Registers modules, defines App
      main.rs           # Desktop launch fn
      router.rs         # Route enum
      state.rs          # VoltState, contexts
      components/
      pages/
      hooks/
```

**Why this is better for Volt:**
- The UI is primarily an alternate entry point, not a separate product.
- Both CLI and UI share 100% of the business logic.
- No Rust workspace complexity; `cargo run` works for both targets.

**Build the UI:**

```bash
# Desktop build
cargo run --bin volt-desktop

# Or if using a feature flag to toggle CLI vs UI
cargo run --features desktop-ui
```

---

## 9. Build Targets: Desktop vs Web

### 9.1. Desktop Renderer (Recommended for Volt)

**How it works:** Dioxus Desktop opens a native OS window (via WebView2 on Windows, WebKit on macOS/Linux) and runs your Rust code natively.

**Pros:**
- No browser installation required for users
- Full native API access (file system, subprocess, DB)
- Single binary distribution via `cargo bundle`
- Same HTML/CSS/RSX for layout

**Build and run:**

```bash
# Run in development (with hot reload)
dx serve --platform desktop

# Build release binary
cargo build --release

# Bundle (DMG, MSI, DEB)
dx bundle --release
```

### 9.2. Web Renderer

**How it works:** Compiles to `wasm32-unknown-unknown` and runs in the browser.

**Pros:**
- Accessible from any browser
- Easy deployment (static files)

**Cons:**
- Requires a backend server for DB / subprocess access
- Volt's architecture (local file, CLI gateway) does not map well to WASM sandbox

### 9.3. Build Target Decision Matrix

| Volt Feature | Desktop | Web |
|-------------|---------|-----|
| Read/write local DB | Direct | Via API server |
| Run `bash` tool | `std::process::Command` | Not possible |
| Access file system | Native path | File picker only |
| Native window | Yes | Browser tab |
| Distribution | `.exe`, `.app`, `.deb` | URL |

**Conclusion:** For Volt, the **Desktop renderer is the clear choice**. It provides a native window, no browser dependency, and full access to the existing Rust code (tool registry, agent loop, DB).

---

## Appendix: Quick Reference

### Launch Desktop App with Window Config

```rust
use dioxus_desktop::{Config, WindowBuilder};
use dioxus::prelude::*;

fn main() {
    let cfg = Config::new()
        .with_window(
            WindowBuilder::new()
                .with_title("Volt")
                .with_inner_size(dioxus_desktop::LogicalSize::new(1400.0, 900.0))
        );
    dioxus_desktop::launch_cfg(App, cfg);
}
```

### Global Signal Pattern

```rust
static AGENT: GlobalSignal<Agent> = Signal::global(|| Agent::default());

// Read
let name = AGENT.read().name.clone();

// Write
AGENT.write().name = "Volt Agent".into();
```

### Custom Hook for SSE

```rust
use dioxus::prelude::*;
use futures::StreamExt;

fn use_sse(url: &str) -> Signal<Vec<String>> {
    let messages = use_signal(|| Vec::new());
    let url = url.to_string();

    use_future(move || {
        let messages = messages.clone();
        async move {
            let client = reqwest::Client::new();
            let res = client.get(&url).send().await.unwrap();
            let mut stream = res.bytes_stream();

            while let Some(item) = stream.next().await {
                if let Ok(bytes) = item {
                    let text = String::from_utf8_lossy(&bytes);
                    messages.write().push(text.to_string());
                }
            }
        }
    });

    messages
}
```

---

*End of Guide*
