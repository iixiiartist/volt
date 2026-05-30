use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::Command;

pub struct LlamaCppTool {
    pub binary_path: PathBuf,
    pub ngl: u32, // number of GPU layers to offload
}

impl LlamaCppTool {
    pub fn new(binary_path: PathBuf) -> Self {
        Self { binary_path, ngl: 0 }
    }

    pub fn with_gpu_layers(mut self, ngl: u32) -> Self {
        self.ngl = ngl;
        self
    }

    /// Run inference via llama.cpp CLI (llama-cli or llama.exe).
    pub async fn run(&self, model_path: &str, prompt: &str, context_size: u32) -> anyhow::Result<String> {
        let output = Command::new(&self.binary_path)
            .arg("-m")
            .arg(model_path)
            .arg("-p")
            .arg(prompt)
            .arg("-c")
            .arg(context_size.to_string())
            .arg("-n")
            .arg("256")
            .args(if self.ngl > 0 {
                vec!["--ngl".to_string(), self.ngl.to_string()]
            } else {
                vec![]
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("llama.cpp failed: {}", stderr));
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_llamacpp_tool() {
        let tool = LlamaCppTool::new(PathBuf::from("llama.exe"));
        assert_eq!(tool.binary_path.to_string_lossy(), "llama.exe");
    }
}
