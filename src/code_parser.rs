#[cfg(feature = "tools-ast")]
use tree_sitter::Parser;

#[cfg(feature = "tools-ast")]
pub struct CodeArtifact {
    pub file_path: String,
    pub language: String,
    pub functions: Vec<String>,
    pub imports: Vec<String>,
    pub classes: Vec<String>,
}

/// Parse a source file and extract structural signatures.
/// Requires the `tools-ast` feature flag.
#[cfg(feature = "tools-ast")]
pub fn parse_file(file_path: &str, content: &str) -> Option<CodeArtifact> {
    let ext = std::path::Path::new(file_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    let (language, mut parser) = match ext {
        "rs" => {
            let mut p = Parser::new();
            p.set_language(&tree_sitter_rust::LANGUAGE.into()).ok()?;
            ("Rust", p)
        }
        "py" => {
            let mut p = Parser::new();
            p.set_language(&tree_sitter_python::LANGUAGE.into()).ok()?;
            ("Python", p)
        }
        "ts" | "tsx" | "js" | "jsx" => {
            let mut p = Parser::new();
            p.set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
                .ok()?;
            ("TypeScript/JavaScript", p)
        }
        _ => return None,
    };

    let tree = parser.parse(content, None)?;
    let root = tree.root_node();

    let mut artifact = CodeArtifact {
        file_path: file_path.to_string(),
        language: language.to_string(),
        functions: Vec::new(),
        imports: Vec::new(),
        classes: Vec::new(),
    };

    extract_symbols(root, content, &mut artifact);
    Some(artifact)
}

#[cfg(feature = "tools-ast")]
fn extract_symbols(node: tree_sitter::Node, source: &str, artifact: &mut CodeArtifact) {
    let kind = node.kind();

    match kind {
        "function_declaration" | "function_item" | "method_declaration" => {
            if let Some(name) = node.child_by_field_name("name") {
                artifact
                    .functions
                    .push(name.utf8_text(source.as_bytes()).unwrap_or("").to_string());
            }
        }
        "class_declaration" | "class_item" => {
            if let Some(name) = node.child_by_field_name("name") {
                artifact
                    .classes
                    .push(name.utf8_text(source.as_bytes()).unwrap_or("").to_string());
            }
        }
        "import_statement" | "import_declaration" | "use_declaration" => {
            artifact
                .imports
                .push(node.utf8_text(source.as_bytes()).unwrap_or("").to_string());
        }
        _ => {}
    }

    for child in node.children(&mut node.walk()) {
        extract_symbols(child, source, artifact);
    }
}

/// Stub for when feature is disabled.
#[cfg(not(feature = "tools-ast"))]
pub fn parse_file(_file_path: &str, _content: &str) -> Option<CodeArtifact> {
    None
}

#[cfg(not(feature = "tools-ast"))]
pub struct CodeArtifact {
    pub file_path: String,
    pub language: String,
    pub functions: Vec<String>,
    pub imports: Vec<String>,
    pub classes: Vec<String>,
}
