use crate::models::ToolResult;
use std::time::Instant;

#[cfg(feature = "tools-pdf")]
pub async fn create_pdf(content: &str, output_path: &str) -> ToolResult {
    let started = Instant::now();
    use printpdf::*;
    use std::fs::File;
    use std::io::BufWriter;

    let mut doc = PdfDocument::new("Volt Document");
    let lines: Vec<&str> = content.lines().collect();
    let mut ops = Vec::new();
    ops.push(Op::StartTextSection);
    ops.push(Op::SetTextCursor { pos: Point::new(Mm(20.0), Mm(280.0)) });
    ops.push(Op::SetFontSizeBuiltinFont { size: Pt(11.0), font: BuiltinFont::Helvetica });
    ops.push(Op::SetLineHeight { lh: Pt(14.0) });
    for _line in &lines {
        ops.push(Op::WriteTextBuiltinFont {
            items: vec![TextItem::Text((*_line).to_string())],
            font: BuiltinFont::Helvetica,
        });
    }
    ops.push(Op::EndTextSection);

    let page = PdfPage::new(Mm(210.0), Mm(297.0), ops);
    let pdf = doc.with_pages(vec![page]);
    let mut warnings = Vec::new();
    let bytes = pdf.save(&PdfSaveOptions::default(), &mut warnings);
    match File::create(output_path) {
        Ok(f) => {
            let mut writer = BufWriter::new(f);
            use std::io::Write;
            match writer.write_all(&bytes) {
                Ok(_) => ToolResult { success: true, output: format!("PDF saved to {}", output_path), error: None, duration_ms: started.elapsed().as_millis() },
                Err(e) => ToolResult { success: false, output: String::new(), error: Some(format!("write: {}", e)), duration_ms: started.elapsed().as_millis() },
            }
        }
        Err(e) => ToolResult { success: false, output: String::new(), error: Some(format!("file: {}", e)), duration_ms: started.elapsed().as_millis() },
    }
}

#[cfg(not(feature = "tools-pdf"))]
pub async fn create_pdf(_content: &str, _output_path: &str) -> ToolResult {
    ToolResult { success: false, output: String::new(), error: Some("PDF tool not compiled".into()), duration_ms: 0 }
}
