use aegis_lang::{HighlightKind, highlight};
use eframe::egui::{self, Color32, FontId, TextFormat, text::LayoutJob};

pub fn job(source: &str) -> LayoutJob {
    let mut job = LayoutJob::default();
    let mut at = 0;
    let normal = Color32::from_rgb(212, 222, 236);
    for token in highlight(source) {
        if at < token.span.start {
            append(&mut job, &source[at..token.span.start], normal);
        }
        let color = match token.kind {
            HighlightKind::Keyword => Color32::from_rgb(188, 159, 252),
            HighlightKind::Number => Color32::from_rgb(235, 189, 124),
            HighlightKind::String => Color32::from_rgb(154, 206, 160),
            HighlightKind::Comment => Color32::from_rgb(109, 135, 125),
            HighlightKind::Invalid => Color32::from_rgb(249, 120, 137),
            _ => normal,
        };
        append(&mut job, &source[token.span.clone()], color);
        at = token.span.end;
    }
    if at < source.len() {
        append(&mut job, &source[at..], normal);
    }
    job
}
fn append(job: &mut LayoutJob, text: &str, color: Color32) {
    job.append(
        text,
        0.0,
        TextFormat {
            font_id: FontId::monospace(15.0),
            color,
            ..Default::default()
        },
    );
}
pub fn layout(
    ui: &egui::Ui,
    buffer: &dyn egui::TextBuffer,
    _width: f32,
) -> std::sync::Arc<egui::Galley> {
    ui.fonts_mut(|f| f.layout_job(job(buffer.as_str())))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_text_and_utf8_spans() {
        let source = "// русский\nfn main() { printi(42); }";
        let highlighted = job(source);
        assert_eq!(highlighted.text, source);
        assert!(highlighted.sections.len() > 5);
        for section in highlighted.sections {
            assert!(source.get(section.byte_range).is_some());
        }
    }
}
