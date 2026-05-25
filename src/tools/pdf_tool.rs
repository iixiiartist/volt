use crate::models::ToolResult;
use std::time::Instant;

pub async fn create_pdf(content: &str, output_path: &str) -> ToolResult {
    let started = Instant::now();
    match generate_pdf(content, output_path) {
        Ok(()) => ToolResult {
            success: true,
            output: format!("PDF saved to {}", output_path),
            error: None,
            duration_ms: started.elapsed().as_millis(),
        },
        Err(e) => ToolResult {
            success: false,
            output: String::new(),
            error: Some(e),
            duration_ms: started.elapsed().as_millis(),
        },
    }
}

fn generate_pdf(content: &str, output_path: &str) -> Result<(), String> {
    let lines: Vec<&str> = content.lines().collect();
    let mut objects = Vec::new();
    let mut offsets = Vec::new();

    // Object 1: Catalog
    objects.push(format!(
        "1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj"
    ));

    // Object 2: Pages
    objects.push(format!(
        "2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj"
    ));

    // Object 3: Page
    objects.push(format!("3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>\nendobj"));

    // Object 4: Content stream
    let mut stream = String::new();
    stream.push_str("BT\n/F1 12 Tf\n");
    for (i, line) in lines.iter().enumerate() {
        let y = 750.0 - (i as f32 * 16.0);
        let escaped = line
            .replace("\\", "\\\\")
            .replace("(", "\\(")
            .replace(")", "\\)")
            .replace("\n", "\\n");
        stream.push_str(&format!("1 0 0 1 50 {} Tm\n({}) Tj\n", y, escaped));
    }
    stream.push_str("ET");
    objects.push(format!(
        "4 0 obj\n<< /Length {} >>\nstream\n{}\nendstream\nendobj",
        stream.len(),
        stream
    ));

    // Object 5: Font (Helvetica)
    objects.push(format!(
        "5 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj"
    ));

    // Build file
    let mut pdf = String::from("%PDF-1.4\n");
    let mut pos = pdf.len() as u64;
    for obj in &objects {
        offsets.push(pos);
        let bytes = obj.as_bytes();
        pdf.push_str(obj);
        pdf.push('\n');
        pos += bytes.len() as u64 + 1;
    }

    // Cross-reference table
    let xref_offset = pos;
    pdf.push_str("xref\n");
    pdf.push_str(&format!("{} {}\n", 0, objects.len() + 1));
    pdf.push_str("0000000000 65535 f \n");
    for off in &offsets {
        pdf.push_str(&format!("{:010} 00000 n \n", off));
    }

    // Trailer
    pdf.push_str("trailer\n<< /Size ");
    pdf.push_str(&format!("{}", objects.len() + 1));
    pdf.push_str(" /Root 1 0 R >>\nstartxref\n");
    pdf.push_str(&format!("{}\n", xref_offset));
    pdf.push_str("%%EOF");

    std::fs::write(output_path, pdf).map_err(|e| format!("write: {}", e))
}
