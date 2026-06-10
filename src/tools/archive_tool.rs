use crate::models::ToolResult;
use std::path::{Component, Path};
use std::time::Instant;

pub async fn archive_extract(path: &str, dest: &str) -> ToolResult {
    let started = Instant::now();
    let path_lower = path.to_lowercase();

    let result = if path_lower.ends_with(".tar.gz") || path_lower.ends_with(".tgz") {
        extract_tar_gz(path, dest).await
    } else if path_lower.ends_with(".tar") {
        extract_tar(path, dest).await
    } else if path_lower.ends_with(".gz") {
        extract_gz(path, dest).await
    } else {
        ToolResult {
            success: false,
            output: String::new(),
            error: Some(format!(
                "unsupported archive format: {}",
                Path::new(path)
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("?")
            )),
            duration_ms: started.elapsed().as_millis(),
        }
    };

    result
}

pub async fn archive_create(path: &str, sources: &[String], format: &str) -> ToolResult {
    let started = Instant::now();

    match format {
        "tar" | "tar.gz" | "tgz" => {
            create_tar_gz(path, sources, format.ends_with("gz") || format == "tgz").await
        }
        _ => ToolResult {
            success: false,
            output: String::new(),
            error: Some(format!("unsupported format: {}", format)),
            duration_ms: started.elapsed().as_millis(),
        },
    }
}

async fn extract_tar_gz(path: &str, dest: &str) -> ToolResult {
    let started = Instant::now();
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) => return fail(started, &format!("open failed: {}", e)),
    };
    let decoder = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);
    match safe_unpack(&mut archive, dest) {
        Ok(count) => ToolResult {
            success: true,
            output: format!("Extracted {} entries from {} to {}", count, path, dest),
            error: None,
            duration_ms: started.elapsed().as_millis(),
        },
        Err(e) => fail_tool(started, &format!("extract failed: {}", e)),
    }
}

async fn extract_tar(path: &str, dest: &str) -> ToolResult {
    let started = Instant::now();
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) => return fail(started, &format!("open failed: {}", e)),
    };
    let mut archive = tar::Archive::new(file);
    match safe_unpack(&mut archive, dest) {
        Ok(count) => ToolResult {
            success: true,
            output: format!("Extracted tar to {} ({} entries)", dest, count),
            error: None,
            duration_ms: started.elapsed().as_millis(),
        },
        Err(e) => fail_tool(started, &format!("extract failed: {}", e)),
    }
}

fn fail(started: Instant, msg: &str) -> ToolResult {
    ToolResult {
        success: false,
        output: String::new(),
        error: Some(msg.to_string()),
        duration_ms: started.elapsed().as_millis(),
    }
}

fn fail_tool(started: Instant, msg: &str) -> ToolResult {
    fail(started, msg)
}

fn safe_unpack<R: std::io::Read>(
    archive: &mut tar::Archive<R>,
    dest: &str,
) -> Result<usize, String> {
    let dest_abs = std::fs::canonicalize(dest).map_err(|e| format!("canonicalize dest: {}", e))?;
    std::fs::create_dir_all(&dest_abs).map_err(|e| format!("mkdir dest: {}", e))?;

    let mut count = 0usize;
    let entries = archive
        .entries()
        .map_err(|e| format!("read entries: {}", e))?;
    for entry_res in entries {
        let mut entry = entry_res.map_err(|e| format!("read entry: {}", e))?;
        let entry_path = entry
            .path()
            .map_err(|e| format!("read entry path: {}", e))?
            .into_owned();
        if is_unsafe_entry(&entry_path) {
            return Err(format!(
                "refusing to extract entry with unsafe path: {}",
                entry_path.display()
            ));
        }
        let target = dest_abs.join(&entry_path);
        let target_canon = std::fs::canonicalize(target.parent().unwrap_or(&dest_abs))
            .unwrap_or_else(|_| dest_abs.clone());
        if !target_canon.starts_with(&dest_abs) {
            return Err(format!(
                "refusing zip-slip: entry '{}' escapes dest",
                entry_path.display()
            ));
        }
        entry
            .unpack(&target)
            .map_err(|e| format!("unpack '{}': {}", entry_path.display(), e))?;
        count += 1;
    }
    Ok(count)
}

fn is_unsafe_entry(path: &Path) -> bool {
    path.components().any(|c| {
        matches!(c, Component::ParentDir) || matches!(c, Component::Prefix(_) | Component::RootDir)
    })
}

async fn extract_gz(path: &str, dest: &str) -> ToolResult {
    let started = Instant::now();
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) => {
            return ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("open failed: {}", e)),
                duration_ms: started.elapsed().as_millis(),
            }
        }
    };
    let mut decoder = flate2::read::GzDecoder::new(file);

    let out_path = Path::new(dest);
    if out_path.is_dir() {
        let stem = Path::new(path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("output");
        let out_file = out_path.join(stem);
        let mut out = match std::fs::File::create(&out_file) {
            Ok(f) => f,
            Err(e) => {
                return ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("create failed: {}", e)),
                    duration_ms: started.elapsed().as_millis(),
                }
            }
        };
        match std::io::copy(&mut decoder, &mut out) {
            Ok(n) => ToolResult {
                success: true,
                output: format!(
                    "Decompressed {} to {} ({} bytes)",
                    path,
                    out_file.display(),
                    n
                ),
                error: None,
                duration_ms: started.elapsed().as_millis(),
            },
            Err(e) => ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("decompress failed: {}", e)),
                duration_ms: started.elapsed().as_millis(),
            },
        }
    } else {
        let mut out = match std::fs::File::create(dest) {
            Ok(f) => f,
            Err(e) => {
                return ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("create failed: {}", e)),
                    duration_ms: started.elapsed().as_millis(),
                }
            }
        };
        match std::io::copy(&mut decoder, &mut out) {
            Ok(n) => ToolResult {
                success: true,
                output: format!("Decompressed {} to {} ({} bytes)", path, dest, n),
                error: None,
                duration_ms: started.elapsed().as_millis(),
            },
            Err(e) => ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("decompress failed: {}", e)),
                duration_ms: started.elapsed().as_millis(),
            },
        }
    }
}

async fn create_tar_gz(path: &str, sources: &[String], gzip: bool) -> ToolResult {
    let started = Instant::now();
    let file = match std::fs::File::create(path) {
        Ok(f) => f,
        Err(e) => {
            return ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("create failed: {}", e)),
                duration_ms: started.elapsed().as_millis(),
            }
        }
    };

    let writer: Box<dyn std::io::Write> = if gzip {
        Box::new(flate2::write::GzEncoder::new(
            file,
            flate2::Compression::default(),
        ))
    } else {
        Box::new(file)
    };

    let mut archive = tar::Builder::new(writer);
    for src in sources {
        let src_path = Path::new(src);
        if src_path.is_file() {
            let name = src_path
                .file_name()
                .unwrap_or(std::ffi::OsStr::new("unknown"));
            if let Err(e) = archive.append_path_with_name(src_path, name) {
                return ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("add failed: {}", e)),
                    duration_ms: started.elapsed().as_millis(),
                };
            }
        } else if src_path.is_dir() {
            let dir_name = src_path
                .file_name()
                .unwrap_or(std::ffi::OsStr::new("unknown"));
            if let Err(e) = archive.append_dir(dir_name, src_path) {
                return ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("add dir failed: {}", e)),
                    duration_ms: started.elapsed().as_millis(),
                };
            }
        }
    }

    match archive.finish() {
        Ok(_) => ToolResult {
            success: true,
            output: format!("Created {} with {} entries", path, sources.len()),
            error: None,
            duration_ms: started.elapsed().as_millis(),
        },
        Err(e) => ToolResult {
            success: false,
            output: String::new(),
            error: Some(format!("finalize failed: {}", e)),
            duration_ms: started.elapsed().as_millis(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_unsafe_entry_rejects_parent_traversal() {
        assert!(is_unsafe_entry(Path::new("../etc/passwd")));
        assert!(is_unsafe_entry(Path::new("foo/../../bar")));
        assert!(is_unsafe_entry(Path::new("/absolute/path")));
        assert!(!is_unsafe_entry(Path::new("foo/bar.txt")));
        assert!(!is_unsafe_entry(Path::new("inner-dir/file.rs")));
    }
}
