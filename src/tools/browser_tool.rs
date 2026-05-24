use crate::models::ToolResult;
use std::time::Instant;

#[cfg(feature = "tools-browser")]
mod browser_impl {
    use super::*;
    use headless_chrome::{Browser, LaunchOptions};

    pub async fn navigate(url: &str) -> ToolResult {
        let started = Instant::now();
        match Browser::new(LaunchOptions::default()) {
            Ok(browser) => {
                match browser.wait_for_initial_tab() {
                    Ok(tab) => {
                        match tab.navigate_to(url) {
                            Ok(t) => {
                                let url = t.get_url();
                                ToolResult { success: true, output: format!("navigated to {}", url), error: None, duration_ms: started.elapsed().as_millis() }
                            }
                            Err(e) => ToolResult { success: false, output: String::new(), error: Some(format!("navigate: {}", e)), duration_ms: started.elapsed().as_millis() },
                        }
                    }
                    Err(e) => ToolResult { success: false, output: String::new(), error: Some(format!("tab: {}", e)), duration_ms: started.elapsed().as_millis() },
                }
            }
            Err(e) => ToolResult { success: false, output: String::new(), error: Some(format!("launch: {}", e)), duration_ms: started.elapsed().as_millis() },
        }
    }

    pub async fn extract(url: &str) -> ToolResult {
        let started = Instant::now();
        match Browser::new(LaunchOptions::default()) {
            Ok(browser) => {
                match browser.wait_for_initial_tab() {
                    Ok(tab) => {
                        if let Err(e) = tab.navigate_to(url) {
                            return ToolResult { success: false, output: String::new(), error: Some(format!("navigate: {}", e)), duration_ms: started.elapsed().as_millis() };
                        }
                        match tab.get_content() {
                            Ok(html) => ToolResult { success: true, output: html.chars().take(2000).collect(), error: None, duration_ms: started.elapsed().as_millis() },
                            Err(e) => ToolResult { success: false, output: String::new(), error: Some(format!("get content: {}", e)), duration_ms: started.elapsed().as_millis() },
                        }
                    }
                    Err(e) => ToolResult { success: false, output: String::new(), error: Some(format!("tab: {}", e)), duration_ms: started.elapsed().as_millis() },
                }
            }
            Err(e) => ToolResult { success: false, output: String::new(), error: Some(format!("launch: {}", e)), duration_ms: started.elapsed().as_millis() },
        }
    }

    pub async fn screenshot(url: &str, output_path: &str) -> ToolResult {
        let started = Instant::now();
        match Browser::new(LaunchOptions::default()) {
            Ok(browser) => {
                match browser.wait_for_initial_tab() {
                    Ok(tab) => {
                        if let Err(e) = tab.navigate_to(url) {
                            return ToolResult { success: false, output: String::new(), error: Some(format!("navigate: {}", e)), duration_ms: started.elapsed().as_millis() };
                        }
                        use headless_chrome::protocol::cdp::Page::CaptureScreenshotFormatOption;
                        match tab.capture_screenshot(CaptureScreenshotFormatOption::Png, None, None, false) {
                            Ok(png_data) => {
                                match std::fs::write(output_path, &png_data) {
                                    Ok(_) => ToolResult { success: true, output: format!("screenshot saved ({} bytes)", png_data.len()), error: None, duration_ms: started.elapsed().as_millis() },
                                    Err(e) => ToolResult { success: false, output: String::new(), error: Some(format!("write: {}", e)), duration_ms: started.elapsed().as_millis() },
                                }
                            }
                            Err(e) => ToolResult { success: false, output: String::new(), error: Some(format!("capture: {}", e)), duration_ms: started.elapsed().as_millis() },
                        }
                    }
                    Err(e) => ToolResult { success: false, output: String::new(), error: Some(format!("tab: {}", e)), duration_ms: started.elapsed().as_millis() },
                }
            }
            Err(e) => ToolResult { success: false, output: String::new(), error: Some(format!("launch: {}", e)), duration_ms: started.elapsed().as_millis() },
        }
    }
}

#[cfg(feature = "tools-browser")]
pub async fn browser_navigate(url: &str) -> ToolResult { browser_impl::navigate(url).await }
#[cfg(feature = "tools-browser")]
pub async fn browser_extract(url: &str, _selector: &str) -> ToolResult { browser_impl::extract(url).await }
#[cfg(feature = "tools-browser")]
pub async fn browser_screenshot(url: &str, output_path: &str) -> ToolResult { browser_impl::screenshot(url, output_path).await }

#[cfg(not(feature = "tools-browser"))]
pub async fn browser_navigate(_url: &str) -> ToolResult { ToolResult { success: false, output: String::new(), error: Some("Browser tool not compiled".into()), duration_ms: 0 } }
#[cfg(not(feature = "tools-browser"))]
pub async fn browser_extract(_url: &str, _selector: &str) -> ToolResult { ToolResult { success: false, output: String::new(), error: Some("Browser tool not compiled".into()), duration_ms: 0 } }
#[cfg(not(feature = "tools-browser"))]
pub async fn browser_screenshot(_url: &str, _output_path: &str) -> ToolResult { ToolResult { success: false, output: String::new(), error: Some("Browser tool not compiled".into()), duration_ms: 0 } }
