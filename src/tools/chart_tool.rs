use crate::models::ToolResult;
use std::time::Instant;

pub async fn create_bar_chart(
    title: &str,
    labels: Vec<String>,
    values: Vec<f64>,
    output_path: &str,
) -> ToolResult {
    let started = Instant::now();
    let labels_json = serde_json::to_string(&labels).unwrap_or_default();
    let values_json = serde_json::to_string(&values).unwrap_or_default();
    let html = format!(
        r#"<!DOCTYPE html><html><head><script src="https://cdn.plot.ly/plotly-3.0.0.min.js"></script></head><body><div id="chart"></div><script>
var data = [{{type: 'bar', x: {labels}, y: {values}}}];
var layout = {{title: '{title}'}};
Plotly.newPlot('chart', data, layout);
</script></body></html>"#,
        labels = labels_json,
        values = values_json,
        title = title
    );
    match std::fs::write(output_path, &html) {
        Ok(_) => ToolResult {
            success: true,
            output: format!("bar chart saved to {}", output_path),
            error: None,
            duration_ms: started.elapsed().as_millis(),
        },
        Err(e) => ToolResult {
            success: false,
            output: String::new(),
            error: Some(format!("write failed: {}", e)),
            duration_ms: started.elapsed().as_millis(),
        },
    }
}

pub async fn create_line_chart(
    title: &str,
    labels: Vec<String>,
    values: Vec<f64>,
    output_path: &str,
) -> ToolResult {
    let started = Instant::now();
    let labels_json = serde_json::to_string(&labels).unwrap_or_default();
    let values_json = serde_json::to_string(&values).unwrap_or_default();
    let html = format!(
        r#"<!DOCTYPE html><html><head><script src="https://cdn.plot.ly/plotly-3.0.0.min.js"></script></head><body><div id="chart"></div><script>
var data = [{{type: 'scatter', mode: 'lines+markers', x: {labels}, y: {values}}}];
var layout = {{title: '{title}'}};
Plotly.newPlot('chart', data, layout);
</script></body></html>"#,
        labels = labels_json,
        values = values_json,
        title = title
    );
    match std::fs::write(output_path, &html) {
        Ok(_) => ToolResult {
            success: true,
            output: format!("line chart saved to {}", output_path),
            error: None,
            duration_ms: started.elapsed().as_millis(),
        },
        Err(e) => ToolResult {
            success: false,
            output: String::new(),
            error: Some(format!("write failed: {}", e)),
            duration_ms: started.elapsed().as_millis(),
        },
    }
}
