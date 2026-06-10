use crate::tools::litertlm::LiteRTTool;
use crate::tools::llamacpp::LlamaCppTool;
use std::path::PathBuf;

/// Multimodal Token Prediction (MTP) tool using draft model + full model.
pub struct MtpTool {
    pub draft_binary: PathBuf,
    pub full_binary: PathBuf,
    pub framework: String,
}

impl MtpTool {
    pub fn new(draft_binary: PathBuf, full_binary: PathBuf, framework: String) -> Self {
        Self {
            draft_binary,
            full_binary,
            framework,
        }
    }

    /// Run MTP: draft model generates candidate tokens, full model verifies.
    pub async fn run_with_draft(
        &self,
        draft_model: &str,
        full_model: &str,
        prompt: &str,
    ) -> anyhow::Result<String> {
        // Run draft model to generate candidates
        let draft_output = match self.framework.as_str() {
            "litertlm" => {
                let tool = LiteRTTool::new(self.draft_binary.clone());
                tool.run(draft_model, prompt, 128).await?
            }
            "llamacpp" => {
                let tool = LlamaCppTool::new(self.draft_binary.clone());
                tool.run(draft_model, prompt, 512).await?
            }
            _ => return Err(anyhow::anyhow!("Unsupported framework: {}", self.framework)),
        };

        // Run full model with the draft output as context
        let full_output = match self.framework.as_str() {
            "litertlm" => {
                let tool = LiteRTTool::new(self.full_binary.clone());
                tool.run(full_model, &draft_output, 256).await?
            }
            "llamacpp" => {
                let tool = LlamaCppTool::new(self.full_binary.clone());
                tool.run(full_model, &draft_output, 4096).await?
            }
            _ => return Err(anyhow::anyhow!("Unsupported framework: {}", self.framework)),
        };

        Ok(full_output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mtp_tool() {
        let tool = MtpTool::new(
            PathBuf::from("litert_lm.exe"),
            PathBuf::from("llama.exe"),
            "litertlm".to_string(),
        );
        assert_eq!(tool.framework, "litertlm");
    }
}
