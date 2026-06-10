//! First-run setup wizard — shown as a full-screen overlay when the
//! runtime boots without any LLM API key configured. Lets the user pick
//! a provider, enter a key (or skip for local Ollama), and submit. The
//! runtime hot-swaps the provider so the rest of the app works
//! immediately, no restart needed.

use super::commands::UiCommand;
use super::state::{
    ToastLevel, VoltState, COLOR_ACCENT, COLOR_BORDER, COLOR_PANEL, COLOR_PANEL_HOVER,
    COLOR_SUCCESS, COLOR_TEXT, COLOR_TEXT_DIM, COLOR_TEXT_MUTED,
};
use dioxus::prelude::*;

#[component]
pub fn SetupWizard() -> Element {
    let mut state: VoltState = use_context();

    // Hooks must be called unconditionally (Rules of Hooks) — move
    // them all to the top of the component so the early-return path
    // below doesn't change the hook count and trigger a Dioxus panic
    // on re-render.
    let mut selected_idx = use_signal(|| 0usize);
    let mut api_key = use_signal(String::new);
    let mut model = use_signal(String::new);
    let mut show_key = use_signal(|| false);
    let mut submitting = use_signal(|| false);

    if !*state.show_setup_wizard.read() {
        return rsx! { div {} };
    }

    let providers = state.setup_providers.read().clone();
    if providers.is_empty() {
        return rsx! { div {} };
    }

    let current = providers
        .get(*selected_idx.read())
        .cloned()
        .unwrap_or_else(|| providers[0].clone());
    let requires_key = current.env_var.is_some();
    let model_value = {
        let mv = model.read().clone();
        if mv.is_empty() {
            current.default_model.clone()
        } else {
            mv
        }
    };

    let on_submit = {
        let mut state = state.clone();
        let provider_slug = current.slug.clone();
        let env_var_name = current.env_var.clone();
        let key = api_key.read().clone();
        let mdl = model_value.clone();
        move |_: MouseEvent| {
            if *submitting.read() {
                return;
            }
            let key_to_send = if env_var_name.is_some() {
                if key.trim().is_empty() {
                    return;
                }
                key.trim().to_string()
            } else {
                String::new()
            };
            submitting.set(true);
            // Reset the `submitting` flag on the next event from
            // the runtime — success (SetupReady) closes the wizard,
            // error (Error) leaves it open and re-enables the button.
            // The handler in app.rs clears the flag.
            state.fire(UiCommand::SubmitApiKey {
                provider: provider_slug.clone(),
                api_key: key_to_send,
                model: mdl.clone(),
            });
        }
    };

    rsx! {
        div {
            style: "position: fixed; inset: 0; background-color: rgba(0, 0, 0, 0.78); z-index: 2000; display: flex; align-items: center; justify-content: center; padding: 24px;",
            div {
                style: "width: 560px; max-width: 100%; max-height: 90vh; overflow-y: auto; background-color: {COLOR_PANEL}; border: 1px solid {COLOR_BORDER}; border-radius: 12px; box-shadow: 0 24px 48px rgba(0,0,0,0.5);",

                // Header
                div {
                    style: "padding: 24px 28px; border-bottom: 1px solid {COLOR_BORDER};",
                    div {
                        style: "display: flex; align-items: center; gap: 12px;",
                        div { style: "width: 36px; height: 36px; background: linear-gradient(135deg, #a855f7, #3b82f6); border-radius: 8px; display: flex; align-items: center; justify-content: center; font-weight: 700; font-size: 18px; color: white;", "V" }
                        div {
                            h2 { style: "margin: 0; font-size: 18px; font-weight: 700; color: {COLOR_TEXT};", "Welcome to Volt" }
                            p { style: "margin: 4px 0 0; font-size: 13px; color: {COLOR_TEXT_DIM};", "Choose an LLM provider to get started." }
                        }
                    }
                }

                // Provider list
                div {
                    style: "padding: 20px 28px;",
                    div {
                        style: "font-size: 11px; text-transform: uppercase; letter-spacing: 0.06em; color: {COLOR_TEXT_MUTED}; margin-bottom: 8px;",
                        "Provider"
                    }
                    for (i, p) in providers.iter().enumerate() {
                        {
                            let is_selected = *selected_idx.read() == i;
                            let bg = if is_selected {
                                "background-color: rgba(168, 85, 247, 0.18); border-color: #a855f7;"
                            } else {
                                "background-color: {COLOR_PANEL_HOVER}; border-color: {COLOR_BORDER};"
                            };
                            let slug = p.slug.clone();
                            let env_disp = p.env_var.clone().unwrap_or_else(|| "—".to_string());
                            rsx! {
                                div {
                                    style: "padding: 12px 14px; border: 1px solid {COLOR_BORDER}; border-radius: 8px; margin-bottom: 8px; cursor: pointer; {bg}",
                                    onclick: move |_| {
                                        selected_idx.set(i);
                                        // Reset the model field so it shows the new provider's default.
                                        model.set(String::new());
                                    },
                                    div {
                                        style: "display: flex; align-items: center; justify-content: space-between;",
                                        div {
                                            span { style: "font-size: 14px; font-weight: 600; color: {COLOR_TEXT};", "{p.label}" }
                                        }
                                        if is_selected {
                                            span { style: "color: {COLOR_SUCCESS}; font-size: 14px;", "✓" }
                                        }
                                    }
                                    div {
                                        style: "margin-top: 4px; font-size: 11px; color: {COLOR_TEXT_MUTED}; font-family: monospace;",
                                        "{slug} · {env_disp} · {p.default_model}"
                                    }
                                }
                            }
                        }
                    }
                }

                // API key field
                if requires_key {
                    div {
                        style: "padding: 0 28px 20px;",
                        div {
                            style: "font-size: 11px; text-transform: uppercase; letter-spacing: 0.06em; color: {COLOR_TEXT_MUTED}; margin-bottom: 8px;",
                            "{current.env_var.clone().unwrap_or_default()} API key"
                        }
                        div {
                            style: "display: flex; gap: 8px;",
                            input {
                                style: "flex: 1; padding: 10px 12px; background-color: {COLOR_PANEL_HOVER}; border: 1px solid {COLOR_BORDER}; border-radius: 6px; color: {COLOR_TEXT}; font-size: 13px; font-family: monospace; outline: none;",
                                r#type: if *show_key.read() { "text" } else { "password" },
                                placeholder: "sk-...",
                                value: "{api_key.read()}",
                                oninput: move |e| {
                                    api_key.set(e.value().to_string());
                                    // Re-enable the button if it was
                                    // stuck after a failed submit.
                                    if *submitting.read() {
                                        submitting.set(false);
                                    }
                                },
                            }
                            button {
                                style: "padding: 10px 12px; background-color: transparent; border: 1px solid {COLOR_BORDER}; color: {COLOR_TEXT_DIM}; border-radius: 6px; cursor: pointer; font-size: 12px;",
                                onclick: move |_| {
                                    let cur = *show_key.read();
                                    show_key.set(!cur);
                                },
                                if *show_key.read() { "Hide" } else { "Show" }
                            }
                        }
                        div {
                            style: "margin-top: 6px; font-size: 11px; color: {COLOR_TEXT_MUTED};",
                            "Your key is stored locally in "
                            code { style: "font-family: monospace; color: {COLOR_TEXT_DIM};", "%APPDATA%\\volt\\.env" }
                            " and never sent anywhere except the provider."
                        }
                    }
                } else {
                    div {
                        style: "padding: 0 28px 20px;",
                        div {
                            style: "padding: 12px 14px; background-color: rgba(34, 197, 94, 0.08); border: 1px solid rgba(34, 197, 94, 0.3); border-radius: 6px; font-size: 12px; color: {COLOR_SUCCESS};",
                            "No API key needed. Make sure Ollama is running on your machine (default: http://localhost:11434)."
                        }
                    }
                }

                // Model field
                div {
                    style: "padding: 0 28px 20px;",
                    div {
                        style: "font-size: 11px; text-transform: uppercase; letter-spacing: 0.06em; color: {COLOR_TEXT_MUTED}; margin-bottom: 8px;",
                        "Model"
                    }
                    input {
                        style: "width: 100%; padding: 10px 12px; background-color: {COLOR_PANEL_HOVER}; border: 1px solid {COLOR_BORDER}; border-radius: 6px; color: {COLOR_TEXT}; font-size: 13px; font-family: monospace; outline: none; box-sizing: border-box;",
                        placeholder: "{current.default_model}",
                        value: "{model_value}",
                        oninput: move |e| model.set(e.value().to_string()),
                    }
                }

                // Actions
                div {
                    style: "padding: 20px 28px; border-top: 1px solid {COLOR_BORDER}; display: flex; justify-content: space-between; gap: 8px;",
                    button {
                        style: "padding: 10px 18px; background-color: transparent; border: 1px solid {COLOR_BORDER}; color: {COLOR_TEXT_DIM}; border-radius: 6px; cursor: pointer; font-size: 13px;",
                        onclick: move |_| {
                            // Don't strand the user — route them to
                            // Settings so they can resume setup
                            // without remembering this URL.
                            state.show_setup_wizard.set(false);
                            state.navigate(crate::webui::routes::Page::Settings);
                            state.toast(
                                ToastLevel::Info,
                                "Configure your provider in Settings \u{2192} API Key Setup.",
                            );
                        },
                        "Skip \u{2014} open Settings"
                    }
                    button {
                        style: "padding: 10px 18px; background-color: {COLOR_ACCENT}; border: none; color: white; border-radius: 6px; cursor: pointer; font-size: 13px; font-weight: 600;",
                        disabled: *submitting.read() || (requires_key && api_key.read().trim().is_empty()),
                        onclick: on_submit,
                        if *submitting.read() { "Connecting\u{2026}" } else { "Connect" }
                    }
                }
            }
        }
    }
}
