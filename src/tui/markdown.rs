use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

/// Render markdown text to ratatui Spans with basic formatting.
/// Supports: **bold**, *italic*, `code`, ```code blocks```, # headings, - lists
pub fn render_to_spans(text: &str) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let mut in_code_block = false;

    for raw_line in text.lines() {
        if raw_line.trim_start().starts_with("```") {
            in_code_block = !in_code_block;
            if in_code_block {
                lines.push(Line::from(Span::styled(
                    "---".to_string(),
                    Style::default().fg(Color::DarkGray),
                )));
            }
            continue;
        }

        if in_code_block {
            lines.push(Line::from(Span::styled(
                format!("  {raw_line}"),
                Style::default().fg(Color::Green),
            )));
            continue;
        }

        // Headings
        if let Some(heading) = raw_line.strip_prefix("### ") {
            lines.push(Line::from(Span::styled(
                heading.to_string(),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )));
            continue;
        }
        if let Some(heading) = raw_line.strip_prefix("## ") {
            lines.push(Line::from(Span::styled(
                heading.to_string(),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            )));
            continue;
        }
        if let Some(heading) = raw_line.strip_prefix("# ") {
            lines.push(Line::from(Span::styled(
                heading.to_uppercase(),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            )));
            continue;
        }

        // List items
        let line_text = if let Some(item) = raw_line.strip_prefix("- ") {
            format!("  * {item}")
        } else if let Some(item) = raw_line.strip_prefix("* ") {
            format!("  * {item}")
        } else {
            raw_line.to_string()
        };

        // Inline formatting
        lines.push(Line::from(render_inline(&line_text)));
    }

    lines
}

/// Parse inline markdown: **bold**, *italic*, `code`
fn render_inline(text: &str) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut remaining = text;

    while !remaining.is_empty() {
        // Inline code
        if let Some(pos) = remaining.find('`') {
            if pos > 0 {
                spans.push(Span::raw(remaining[..pos].to_string()));
            }
            let after = &remaining[pos + 1..];
            if let Some(end) = after.find('`') {
                spans.push(Span::styled(
                    after[..end].to_string(),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ));
                remaining = &after[end + 1..];
                continue;
            }
            // No closing backtick
            spans.push(Span::raw(remaining.to_string()));
            break;
        }

        // Bold **text**
        if let Some(pos) = remaining.find("**") {
            if pos > 0 {
                spans.push(Span::raw(remaining[..pos].to_string()));
            }
            let after = &remaining[pos + 2..];
            if let Some(end) = after.find("**") {
                spans.push(Span::styled(
                    after[..end].to_string(),
                    Style::default().add_modifier(Modifier::BOLD),
                ));
                remaining = &after[end + 2..];
                continue;
            }
            spans.push(Span::raw(remaining.to_string()));
            break;
        }

        // Italic *text*
        if let Some(pos) = remaining.find('*') {
            if pos > 0 {
                spans.push(Span::raw(remaining[..pos].to_string()));
            }
            let after = &remaining[pos + 1..];
            if let Some(end) = after.find('*') {
                spans.push(Span::styled(
                    after[..end].to_string(),
                    Style::default().add_modifier(Modifier::ITALIC),
                ));
                remaining = &after[end + 1..];
                continue;
            }
            spans.push(Span::raw(remaining.to_string()));
            break;
        }

        // Plain text (no special chars)
        spans.push(Span::raw(remaining.to_string()));
        break;
    }

    if spans.is_empty() {
        spans.push(Span::raw(String::new()));
    }

    spans
}
