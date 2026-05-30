use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::Command;

pub struct LiteRTTool {
    pub binary_path: PathBuf,
}

impl LiteRTTool {
    pub fn new(binary_path: PathBuf) -> Self {
        Self { binary_path }
    }

    /// Run inference using LiteRT-LM CLI.
    pub async fn run(&self, model_path: &str, prompt: &str, max_tokens: u32) -> anyhow::Result<String> {
        let output = Command::new(&self.binary_path)
            .arg("run")
            .arg("--model")
            .arg(model_path)
            .arg("--prompt")
            .arg(prompt)
            .arg("--max-tokens")
            .arg(max_tokens.to_string())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("LiteRT-LM failed: {}", stderr));
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_litert_tool() {
        // Placeholder test: create dummy binary for testing.
        let tool = LiteRTTool::new(PathBuf::from("litert_lm.exe"));
        // We won't actually run it here since we don't have a real model.
        assert_eq!(tool.binary_path.to_string_lossy(), "litert_lm.exe");
    }
}
