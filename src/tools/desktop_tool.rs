use crate::models::ToolResult;
use std::time::Instant;

#[cfg(feature = "tools-desktop")]
mod desktop_impl {
    use super::*;
    use enigo::*;

    pub async fn click(x: i32, y: i32) -> ToolResult {
        let started = Instant::now();
        match Enigo::new(&Settings::default()) {
            Ok(mut enigo) => {
                let _ = enigo.move_mouse(x, y, Coordinate::Abs);
                match enigo.button(Button::Left, Direction::Click) {
                    Ok(_) => ToolResult { success: true, output: format!("clicked at ({},{})", x, y), error: None, duration_ms: started.elapsed().as_millis() },
                    Err(e) => ToolResult { success: false, output: String::new(), error: Some(format!("click: {:?}", e)), duration_ms: started.elapsed().as_millis() },
                }
            }
            Err(e) => ToolResult { success: false, output: String::new(), error: Some(format!("enigo: {:?}", e)), duration_ms: started.elapsed().as_millis() },
        }
    }

    pub async fn r#type(text: &str) -> ToolResult {
        let started = Instant::now();
        match Enigo::new(&Settings::default()) {
            Ok(mut enigo) => match enigo.text(text) {
                Ok(_) => ToolResult { success: true, output: format!("typed {} chars", text.len()), error: None, duration_ms: started.elapsed().as_millis() },
                Err(e) => ToolResult { success: false, output: String::new(), error: Some(format!("type: {:?}", e)), duration_ms: started.elapsed().as_millis() },
            },
            Err(e) => ToolResult { success: false, output: String::new(), error: Some(format!("enigo: {:?}", e)), duration_ms: started.elapsed().as_millis() },
        }
    }

    pub async fn press_key(key: &str) -> ToolResult {
        let started = Instant::now();
        match Enigo::new(&Settings::default()) {
            Ok(mut enigo) => {
                let k = match key.to_lowercase().as_str() {
                    "enter" | "return" => Key::Return, "tab" => Key::Tab, "escape" | "esc" => Key::Escape,
                    "space" => Key::Space, "backspace" => Key::Backspace, "delete" => Key::Delete,
                    "up" => Key::UpArrow, "down" => Key::DownArrow, "left" => Key::LeftArrow, "right" => Key::RightArrow,
                    _ => return ToolResult { success: false, output: String::new(), error: Some(format!("unknown key: {}", key)), duration_ms: started.elapsed().as_millis() },
                };
                match enigo.key(k, Direction::Click) {
                    Ok(_) => ToolResult { success: true, output: format!("pressed {}", key), error: None, duration_ms: started.elapsed().as_millis() },
                    Err(e) => ToolResult { success: false, output: String::new(), error: Some(format!("key: {:?}", e)), duration_ms: started.elapsed().as_millis() },
                }
            }
            Err(e) => ToolResult { success: false, output: String::new(), error: Some(format!("enigo: {:?}", e)), duration_ms: started.elapsed().as_millis() },
        }
    }

    pub async fn find_window(title: &str) -> ToolResult {
        let started = Instant::now();
        // Write C# source to temp file to avoid escaping hell
        let cs_source = format!(
            "using System;using System.Runtime.InteropServices;
class W {{
    [DllImport(\"user32.dll\")] static extern IntPtr FindWindow(string c, string n);
    [DllImport(\"user32.dll\")] static extern bool GetWindowRect(IntPtr h, out RECT r);
    struct RECT {{ public int left;public int top;public int right;public int bottom; }}
    static void Main() {{
        IntPtr h = FindWindow(null, \"{0}\");
        if (h == IntPtr.Zero) {{ Console.Write(\"not found\"); return; }}
        RECT r; GetWindowRect(h, out r);
        Console.Write($\"found at ({{r.left}},{{r.top}}) {{r.right-r.left}}x{{r.bottom-r.top}}\");
    }}
}}", title);
        let cs_path = std::env::temp_dir().join("find_window.cs");
        let exe_path = std::env::temp_dir().join("find_window.exe");
        let _ = std::fs::write(&cs_path, &cs_source);
        let _ = std::process::Command::new("csc").arg("-out:find_window.exe").arg(&cs_path).output();
        let result = if exe_path.exists() {
            match std::process::Command::new(&exe_path).output() {
                Ok(out) => {
                    let output = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    if output.contains("not found") {
                        ToolResult { success: false, output: String::new(), error: Some(format!("window '{0}' not found", title)), duration_ms: started.elapsed().as_millis() }
                    } else {
                        ToolResult { success: true, output, error: None, duration_ms: started.elapsed().as_millis() }
                    }
                }
                Err(e) => ToolResult { success: false, output: String::new(), error: Some(format!("exec: {0}", e)), duration_ms: started.elapsed().as_millis() },
            }
        } else {
            // Fallback: use get-process to find window by title
            let script = format!("$p=Get-Process|Where-Object{{$_.MainWindowTitle -like '*{0}*'}};if($p){{$p[0].MainWindowTitle}}else{{'not found'}}", title);
            match std::process::Command::new("powershell").arg("-NoProfile").arg("-Command").arg(&script).output() {
                Ok(out) => {
                    let output = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    if output.contains("not found") { ToolResult { success: false, output: String::new(), error: Some(format!("window '{0}' not found", title)), duration_ms: started.elapsed().as_millis() } }
                    else { ToolResult { success: true, output: format!("found window: {0}", output), error: None, duration_ms: started.elapsed().as_millis() } }
                }
                Err(e) => ToolResult { success: false, output: String::new(), error: Some(format!("ps: {0}", e)), duration_ms: started.elapsed().as_millis() },
            }
        };
        let _ = std::fs::remove_file(&cs_path);
        let _ = std::fs::remove_file(&exe_path);
        result
    }
}

#[cfg(feature = "tools-desktop")]
pub async fn desktop_click(x: i32, y: i32) -> ToolResult { desktop_impl::click(x, y).await }
#[cfg(feature = "tools-desktop")]
pub async fn desktop_type(text: &str) -> ToolResult { desktop_impl::r#type(text).await }
#[cfg(feature = "tools-desktop")]
pub async fn desktop_key(key: &str) -> ToolResult { desktop_impl::press_key(key).await }
#[cfg(feature = "tools-desktop")]
pub async fn desktop_find_window(title: &str) -> ToolResult { desktop_impl::find_window(title).await }

#[cfg(not(feature = "tools-desktop"))]
pub async fn desktop_click(_x: i32, _y: i32) -> ToolResult { ToolResult { success: false, output: String::new(), error: Some("Desktop tool not compiled".into()), duration_ms: 0 } }
#[cfg(not(feature = "tools-desktop"))]
pub async fn desktop_type(_text: &str) -> ToolResult { ToolResult { success: false, output: String::new(), error: Some("Desktop tool not compiled".into()), duration_ms: 0 } }
#[cfg(not(feature = "tools-desktop"))]
pub async fn desktop_key(_key: &str) -> ToolResult { ToolResult { success: false, output: String::new(), error: Some("Desktop tool not compiled".into()), duration_ms: 0 } }
#[cfg(not(feature = "tools-desktop"))]
pub async fn desktop_find_window(_title: &str) -> ToolResult { ToolResult { success: false, output: String::new(), error: Some("Desktop tool not compiled".into()), duration_ms: 0 } }
