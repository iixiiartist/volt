use crate::models::ToolResult;
use std::time::Instant;

/// Maximum chart dimensions (scales down for few data points).
const CHART_WIDTH: f64 = 800.0;
const CHART_HEIGHT: f64 = 400.0;
const MARGIN: f64 = 60.0;

/// Generate HTML with an inline SVG bar chart (no external dependencies, works offline).
pub async fn create_bar_chart(
    title: &str,
    labels: Vec<String>,
    values: Vec<f64>,
    output_path: &str,
) -> ToolResult {
    let started = Instant::now();
    let svg = render_svg_bars(title, &labels, &values);
    let html = format!("<!DOCTYPE html><html><body>{}</body></html>", svg);

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

/// Generate HTML with an inline SVG line chart (no external dependencies, works offline).
pub async fn create_line_chart(
    title: &str,
    labels: Vec<String>,
    values: Vec<f64>,
    output_path: &str,
) -> ToolResult {
    let started = Instant::now();
    let svg = render_svg_line(title, &labels, &values);
    let html = format!("<!DOCTYPE html><html><body>{}</body></html>", svg);

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

fn render_svg_bars(title: &str, labels: &[String], values: &[f64]) -> String {
    let n = labels.len().max(1);
    let max_val = values
        .iter()
        .cloned()
        .fold(f64::NEG_INFINITY, f64::max)
        .max(1.0);
    let bar_w = (CHART_WIDTH - 2.0 * MARGIN) / n as f64;
    let gap = bar_w * 0.2;
    let actual_w = bar_w - gap;

    let mut bars = String::new();
    for (i, v) in values.iter().enumerate() {
        let h = (*v / max_val) * (CHART_HEIGHT - 2.0 * MARGIN);
        let x = MARGIN + i as f64 * bar_w + gap / 2.0;
        let y = CHART_HEIGHT - MARGIN - h;
        let hue = (i as f64 * 137.5) % 360.0;
        bars.push_str(&format!(
            r#"<rect x="{x:.1}" y="{y:.1}" width="{w:.1}" height="{h:.1}" fill="hsl({hue},70%,55%)" />"#,
            x = x,
            y = y,
            w = actual_w,
            h = h,
            hue = hue
        ));
        // Value label above bar
        let label_y = y - 8.0;
        bars.push_str(&format!(
            r#"<text x="{:.1}" y="{:.1}" text-anchor="middle" font-size="11">{}</text>"#,
            x + actual_w / 2.0,
            label_y,
            v
        ));
    }

    let mut x_labels = String::new();
    for (i, lbl) in labels.iter().enumerate() {
        let x = MARGIN + i as f64 * bar_w + bar_w / 2.0;
        let short = if lbl.len() > 12 {
            format!("{}…", &lbl[..11])
        } else {
            lbl.clone()
        };
        x_labels.push_str(&format!(
            r#"<text x="{:.1}" y="{:.1}" text-anchor="middle" font-size="10" transform="rotate(-30,{:.1},{:.1})">{}</text>"#,
            x,
            CHART_HEIGHT - MARGIN + 20.0,
            x,
            CHART_HEIGHT - MARGIN + 20.0,
            escape_xml(&short)
        ));
    }

    // Y-axis ticks
    let mut y_ticks = String::new();
    for t in 0..=4 {
        let val = max_val * t as f64 / 4.0;
        let y = CHART_HEIGHT - MARGIN - (t as f64 / 4.0) * (CHART_HEIGHT - 2.0 * MARGIN);
        y_ticks.push_str(&format!(
            r#"<text x="{:.1}" y="{:.1}" text-anchor="end" font-size="10">{:.0}</text>"#,
            MARGIN - 5.0,
            y + 4.0,
            val
        ));
    }

    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" style="max-width:100%;font-family:sans-serif">
<text x="{w_half}" y="30" text-anchor="middle" font-size="16" font-weight="bold">{title}</text>
{axes}
{bars}
{x_labels}
{y_ticks}
</svg>"#,
        w = CHART_WIDTH,
        h = CHART_HEIGHT,
        w_half = CHART_WIDTH / 2.0,
        title = escape_xml(title),
        axes = render_axes(),
        bars = bars,
        x_labels = x_labels,
        y_ticks = y_ticks,
    )
}

fn render_svg_line(title: &str, labels: &[String], values: &[f64]) -> String {
    let n = labels.len().max(2);
    let max_val = values
        .iter()
        .cloned()
        .fold(f64::NEG_INFINITY, f64::max)
        .max(1.0);
    let min_val = values
        .iter()
        .cloned()
        .fold(f64::INFINITY, f64::min)
        .min(0.0);
    let range = (max_val - min_val).max(1.0);
    let x_step = (CHART_WIDTH - 2.0 * MARGIN) / (n - 1) as f64;

    let mut points = String::new();
    let mut circles = String::new();
    for (i, v) in values.iter().enumerate() {
        let x = MARGIN + i as f64 * x_step;
        let y = CHART_HEIGHT - MARGIN - ((*v - min_val) / range) * (CHART_HEIGHT - 2.0 * MARGIN);
        if i == 0 {
            points.push_str(&format!("{:.1},{:.1}", x, y));
        } else {
            points.push_str(&format!(" {:.1},{:.1}", x, y));
        }
        circles.push_str(&format!(
            r#"<circle cx="{:.1}" cy="{:.1}" r="3" fill="hsl(210,80%,55%)" />"#,
            x, y
        ));
        circles.push_str(&format!(
            r#"<text x="{:.1}" y="{:.1}" text-anchor="middle" font-size="10">{}</text>"#,
            x,
            y - 10.0,
            v
        ));
    }

    let mut x_labels = String::new();
    for (i, lbl) in labels.iter().enumerate() {
        let x = MARGIN + i as f64 * x_step;
        let short = if lbl.len() > 12 {
            format!("{}…", &lbl[..11])
        } else {
            lbl.clone()
        };
        x_labels.push_str(&format!(
            r#"<text x="{:.1}" y="{:.1}" text-anchor="middle" font-size="10" transform="rotate(-25,{:.1},{:.1})">{}</text>"#,
            x,
            CHART_HEIGHT - MARGIN + 20.0,
            x,
            CHART_HEIGHT - MARGIN + 20.0,
            escape_xml(&short)
        ));
    }

    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" style="max-width:100%;font-family:sans-serif">
<text x="{w_half}" y="30" text-anchor="middle" font-size="16" font-weight="bold">{title}</text>
{axes}
<polyline points="{points}" fill="none" stroke="hsl(210,80%,55%)" stroke-width="2" />
{circles}
{x_labels}
</svg>"#,
        w = CHART_WIDTH,
        h = CHART_HEIGHT,
        w_half = CHART_WIDTH / 2.0,
        title = escape_xml(title),
        axes = render_axes(),
        points = points,
        circles = circles,
        x_labels = x_labels,
    )
}

fn render_axes() -> String {
    let color = "silver"; // avoid hex # in format strings
    format!(
        r#"<line x1="{m}" y1="{m}" x2="{m}" y2="{he}" stroke="{c}" stroke-width="1" />
<line x1="{m}" y1="{he}" x2="{we}" y2="{he}" stroke="{c}" stroke-width="1" />"#,
        m = MARGIN,
        he = CHART_HEIGHT - MARGIN,
        we = CHART_WIDTH - MARGIN,
        c = color,
    )
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
