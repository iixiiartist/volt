/// Wrap tool output in XML to mark it as coming from a tool, not user input.
/// This acts as a prompt injection defense layer.
pub fn wrap_tool_output(tool_name: &str, output: &str) -> String {
    const MAX_LEN: usize = 500_000;
    let truncated = if output.len() > MAX_LEN {
        &output[..MAX_LEN]
    } else {
        output
    };
    let escaped = xml_escape(truncated);
    format!(
        "<tool_output name=\"{}\" sanitized=\"true\" size=\"{}\" length=\"{}\">\n{}\n</tool_output>",
        xml_escape_attr(tool_name),
        escaped.len(),
        output.len(),
        escaped
    )
}

fn xml_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn xml_escape_attr(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wrap_basic() {
        let wrapped = wrap_tool_output("bash", "hello world");
        assert!(wrapped.starts_with("<tool_output"));
        assert!(wrapped.contains("hello world"));
        assert!(wrapped.ends_with("</tool_output>"));
    }

    #[test]
    fn test_escapes_lt_gt() {
        let wrapped = wrap_tool_output("bash", "a < b & c > d");
        assert!(wrapped.contains("a &lt; b &amp; c &gt; d"));
    }

    #[test]
    fn test_truncation() {
        let big = "x".repeat(600_000);
        let wrapped = wrap_tool_output("bash", &big);
        // Should be truncated to ~500K + header/footer
        assert!(wrapped.len() < 550_000);
    }
}
