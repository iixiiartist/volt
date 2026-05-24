use crate::models::ToolResult;
use std::sync::mpsc;
use std::time::Instant;

#[cfg(feature = "tools-screenshot")]
mod capture_impl {
    use super::*;
    use windows_capture::capture::{Context, GraphicsCaptureApiHandler};
    use windows_capture::frame::Frame;
    use windows_capture::graphics_capture_api::InternalCaptureControl;
    use windows_capture::monitor::Monitor;
    use windows_capture::settings::{
        ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
        MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
    };

    struct ScreenshotCapture {
        result_tx: Option<mpsc::Sender<(Vec<u8>, u32, u32)>>,
    }

    impl GraphicsCaptureApiHandler for ScreenshotCapture {
        type Flags = mpsc::Sender<(Vec<u8>, u32, u32)>;
        type Error = Box<dyn std::error::Error + Send + Sync>;

        fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
            Ok(Self { result_tx: Some(ctx.flags) })
        }

        fn on_frame_arrived(
            &mut self,
            frame: &mut Frame,
            capture_control: InternalCaptureControl,
        ) -> Result<(), Self::Error> {
            let w = frame.width();
            let h = frame.height();
            let pixel_size: u32 = 4;
            if let Ok(mut fb) = frame.buffer() {
                let row_pitch = fb.row_pitch();
                let raw = fb.as_raw_buffer();
                let row_width = (w * pixel_size) as usize;

                let mut pixels = Vec::with_capacity((w * h * pixel_size) as usize);
                for y in 0..h {
                    let start = (y * row_pitch) as usize;
                    let end = start + row_width;
                    if end <= raw.len() {
                        pixels.extend_from_slice(&raw[start..end]);
                    }
                }
                for chunk in pixels.chunks_exact_mut(4) {
                    chunk.swap(0, 2);
                }

                if let Some(tx) = self.result_tx.take() {
                    let _ = tx.send((pixels, w, h));
                }
            }
            capture_control.stop();
            Ok(())
        }

        fn on_closed(&mut self) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    pub fn capture_primary_monitor() -> ToolResult {
        let started = Instant::now();

        match Monitor::primary() {
            Ok(monitor) => {
                let (tx, rx) = mpsc::channel();

                let settings = Settings::new(
                    monitor,
                    CursorCaptureSettings::Default,
                    DrawBorderSettings::Default,
                    SecondaryWindowSettings::Default,
                    MinimumUpdateIntervalSettings::Default,
                    DirtyRegionSettings::Default,
                    ColorFormat::Bgra8,
                    tx,
                );

                match ScreenshotCapture::start(settings) {
                    Ok(_) => match rx.recv_timeout(std::time::Duration::from_secs(5)) {
                        Ok((rgba_data, w, h)) => {
                            let img = image::RgbaImage::from_raw(w, h, rgba_data)
                                .unwrap_or_else(|| image::RgbaImage::new(1, 1));
                            let mut png_bytes = Vec::new();
                            img.write_to(
                                &mut std::io::Cursor::new(&mut png_bytes),
                                image::ImageFormat::Png,
                            )
                            .ok();
                            let b64 = base64::Engine::encode(
                                &base64::engine::general_purpose::STANDARD,
                                &png_bytes,
                            );
                            ToolResult {
                                success: true,
                                output: format!("data:image/png;base64,{}", b64),
                                error: None,
                                duration_ms: started.elapsed().as_millis(),
                            }
                        }
                        Err(_) => ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some("screenshot capture timed out".into()),
                            duration_ms: started.elapsed().as_millis(),
                        },
                    },
                    Err(e) => ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("capture start failed: {}", e)),
                        duration_ms: started.elapsed().as_millis(),
                    },
                }
            }
            Err(e) => ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("no monitor found: {}", e)),
                duration_ms: started.elapsed().as_millis(),
            },
        }
    }
}

#[cfg(feature = "tools-screenshot")]
pub async fn capture_screenshot() -> ToolResult {
    tokio::task::spawn_blocking(|| capture_impl::capture_primary_monitor())
        .await
        .unwrap_or_else(|e| ToolResult {
            success: false,
            output: String::new(),
            error: Some(format!("screenshot thread failed: {}", e)),
            duration_ms: 0,
        })
}

#[cfg(not(feature = "tools-screenshot"))]
pub async fn capture_screenshot() -> ToolResult {
    ToolResult {
        success: false,
        output: String::new(),
        error: Some("screenshot tool not compiled (feature 'tools-screenshot' disabled)".into()),
        duration_ms: 0,
    }
}
