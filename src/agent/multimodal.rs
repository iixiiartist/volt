use base64::{engine::general_purpose::STANDARD, Engine as _};
use std::path::Path;

/// Encode an image file to base64 with the appropriate Gemma-4 tag.
pub fn encode_image(path: &Path) -> anyhow::Result<String> {
    let data = std::fs::read(path)?;
    let b64 = STANDARD.encode(&data);
    Ok(format!("<image>{}</image>", b64))
}

/// Encode an audio file to base64 with the appropriate Gemma-4 tag.
pub fn encode_audio(path: &Path) -> anyhow::Result<String> {
    let data = std::fs::read(path)?;
    let b64 = STANDARD.encode(&data);
    Ok(format!("<audio>{}</audio>", b64))
}

/// Encode a video file to base64 with the appropriate Gemma-4 tag.
pub fn encode_video(path: &Path) -> anyhow::Result<String> {
    let data = std::fs::read(path)?;
    let b64 = STANDARD.encode(&data);
    Ok(format!("<video>{}</video>", b64))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn create_temp_file(name: &str, content: &[u8]) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        path.push(name);
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(content).unwrap();
        path
    }

    #[test]
    fn test_encode_image() {
        let tmp = create_temp_file("test_image.png", &[0x89, 0x50, 0x4E, 0x47]);
        let result = encode_image(&tmp);
        assert!(result.is_ok());
        let encoded = result.unwrap();
        assert!(encoded.starts_with("<image>"));
        assert!(encoded.ends_with("</image>"));
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn test_encode_audio() {
        let tmp = create_temp_file("test_audio.wav", &[0x52, 0x49, 0x46, 0x46]);
        let result = encode_audio(&tmp);
        assert!(result.is_ok());
        let encoded = result.unwrap();
        assert!(encoded.starts_with("<audio>"));
        assert!(encoded.ends_with("</audio>"));
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn test_encode_video() {
        let tmp = create_temp_file(
            "test_video.mp4",
            &[0x00, 0x00, 0x00, 0x18, 0x66, 0x74, 0x79, 0x70],
        );
        let result = encode_video(&tmp);
        assert!(result.is_ok());
        let encoded = result.unwrap();
        assert!(encoded.starts_with("<video>"));
        assert!(encoded.ends_with("</video>"));
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn test_encode_nonexistent_file() {
        let tmp = std::env::temp_dir().join("nonexistent_file.xyz");
        let result = encode_image(&tmp);
        assert!(result.is_err());
    }

    #[test]
    fn test_base64_content_encodes_image() {
        let tmp = create_temp_file("test_dot.png", b"test_content");
        let result = encode_image(&tmp).unwrap();
        assert!(result.contains("dGVzdF9jb250ZW50")); // base64 of "test_content"
        let _ = std::fs::remove_file(&tmp);
    }
}
