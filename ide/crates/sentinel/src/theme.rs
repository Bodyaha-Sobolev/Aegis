//! Sentinel visual system: DeepSeek-inspired blue-on-near-black palette with a
//! slowly drifting "living" background, system typography and shared widgets.

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontId, Margin, Rect, Stroke, Vec2,
};

// Palette -----------------------------------------------------------------
/// Near-black blue base behind everything; the living blobs drift on it.
pub const BG: Color32 = Color32::from_rgb(11, 14, 22);
/// Panels sit slightly translucent so the living background stays visible.
fn panel_translucent() -> Color32 {
    Color32::from_rgba_unmultiplied(16, 21, 31, 230)
}
/// Elevated cards, toolbars, popups.
pub const RAISED: Color32 = Color32::from_rgb(22, 29, 43);
/// Hover fill on quiet controls.
pub const HOVER: Color32 = Color32::from_rgb(29, 38, 55);
/// Active/pressed fill.
pub const ACTIVE: Color32 = Color32::from_rgb(35, 46, 66);
/// DeepSeek blue accent (#4d6bfe).
pub const ACCENT: Color32 = Color32::from_rgb(77, 107, 254);
/// Accent on hover (#5686fe).
pub const ACCENT_HOVER: Color32 = Color32::from_rgb(86, 134, 254);
/// Dim accent for soft fills and selection.
pub const ACCENT_DIM: Color32 = Color32::from_rgb(46, 63, 143);
/// Secondary gradient stop for brand marks and primary buttons.
pub const ACCENT_VIOLET: Color32 = Color32::from_rgb(124, 92, 255);
/// Text hierarchy.
pub const TEXT: Color32 = Color32::from_rgb(234, 238, 246);
pub const TEXT_2: Color32 = Color32::from_rgb(154, 165, 184);
pub const TEXT_3: Color32 = Color32::from_rgb(94, 107, 128);
pub const DANGER: Color32 = Color32::from_rgb(244, 104, 126);
pub const WARNING: Color32 = Color32::from_rgb(232, 179, 106);
pub const OK: Color32 = Color32::from_rgb(82, 208, 165);
/// Editor line numbers.
pub const GUTTER: Color32 = Color32::from_rgb(61, 72, 89);

pub fn border() -> Stroke {
    Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 20))
}

pub fn border_strong() -> Stroke {
    Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 38))
}

// Typography --------------------------------------------------------------
const SEGOE_PATHS: &[&str] = &["C:\\Windows\\Fonts\\segoeui.ttf"];
const MONO_PATHS: &[&str] = &[
    "C:\\Windows\\Fonts\\CascadiaCode.ttf",
    "C:\\Windows\\Fonts\\consola.ttf",
];

fn read_font(paths: &[&str]) -> Option<Vec<u8>> {
    paths
        .iter()
        .find_map(|path| std::fs::read(path).ok())
        .filter(|bytes| bytes.len() > 1000)
}

/// Install system typography (Segoe UI + Cascadia Code when present) and the
/// widget visuals. Missing fonts silently keep the built-in egui defaults.
pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    if let Some(bytes) = read_font(SEGOE_PATHS) {
        fonts
            .font_data
            .insert("segoe_ui".into(), FontData::from_owned(bytes).into());
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, "segoe_ui".into());
    }
    if let Some(bytes) = read_font(MONO_PATHS) {
        fonts
            .font_data
            .insert("code_mono".into(), FontData::from_owned(bytes).into());
        fonts
            .families
            .entry(egui::FontFamily::Monospace)
            .or_default()
            .insert(0, "code_mono".into());
    }
    ctx.set_fonts(fonts);

    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = panel_translucent();
    visuals.window_fill = RAISED;
    // The editor surface is a touch translucent so the drift is visible in code view.
    visuals.extreme_bg_color = Color32::from_rgba_unmultiplied(11, 14, 22, 214);
    visuals.faint_bg_color = RAISED;
    visuals.selection.bg_fill = ACCENT_DIM;
    visuals.selection.stroke.color = TEXT;
    visuals.hyperlink_color = ACCENT_HOVER;
    visuals.window_corner_radius = CornerRadius::same(12);
    visuals.menu_corner_radius = CornerRadius::same(10);
    for state in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
    ] {
        state.corner_radius = CornerRadius::same(8);
    }
    visuals.window_stroke = border_strong();
    visuals.widgets.noninteractive.bg_fill = RAISED;
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_2);
    visuals.widgets.noninteractive.bg_stroke = border();
    visuals.widgets.inactive.bg_fill = HOVER;
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    visuals.widgets.inactive.bg_stroke = Stroke::NONE;
    visuals.widgets.hovered.bg_fill = ACTIVE;
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT);
    visuals.widgets.hovered.bg_stroke = border_strong();
    visuals.widgets.active.bg_fill = ACCENT_DIM;
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, TEXT);
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
    ctx.set_visuals(visuals);

    ctx.style_mut(|style| {
        style.spacing.item_spacing = egui::vec2(10.0, 8.0);
        style.spacing.button_padding = egui::vec2(12.0, 6.0);
        style.spacing.menu_margin = Margin::same(6);
        style
            .text_styles
            .insert(egui::TextStyle::Body, FontId::proportional(14.0));
        style
            .text_styles
            .insert(egui::TextStyle::Button, FontId::proportional(14.0));
        style
            .text_styles
            .insert(egui::TextStyle::Small, FontId::proportional(12.0));
        style
            .text_styles
            .insert(egui::TextStyle::Heading, FontId::proportional(19.0));
        style
            .text_styles
            .insert(egui::TextStyle::Monospace, FontId::monospace(14.5));
        style.text_styles.insert(
            egui::TextStyle::Name("Caption".into()),
            FontId::proportional(11.0),
        );
        style.text_styles.insert(
            egui::TextStyle::Name("Brand".into()),
            FontId::proportional(17.0),
        );
    });
}

pub fn caption(text: &str) -> egui::RichText {
    egui::RichText::new(text).text_style(egui::TextStyle::Name("Caption".into()))
}

/// Frame for elevated cards inside panels.
pub fn card() -> egui::Frame {
    egui::Frame::new()
        .fill(RAISED)
        .stroke(border())
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::same(10))
}

// Living background --------------------------------------------------------
struct Blob {
    radius: f32,
    color: Color32,
    speed: f32,
    phase: f32,
    reach: f32,
}

/// One smooth radial glow: a triangle fan from a colored center vertex to
/// fully transparent edge vertices (premultiplied interpolation = clean falloff).
fn glow(painter: &egui::Painter, center: egui::Pos2, radius: f32, color: Color32) {
    const SEGMENTS: usize = 36;
    let mut mesh = egui::Mesh::default();
    let center_index = mesh.vertices.len() as u32;
    mesh.colored_vertex(center, color);
    let transparent = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 0);
    for step in 0..=SEGMENTS {
        let angle = step as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
        mesh.colored_vertex(
            center + Vec2::new(angle.cos(), angle.sin()) * radius,
            transparent,
        );
    }
    for step in 0..SEGMENTS as u32 {
        mesh.indices.extend([
            center_index,
            center_index + 1 + step,
            center_index + 2 + step,
        ]);
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// Slowly drifting soft blue glows painted inside the central work area,
/// DeepSeek-style. The painter is clipped to the calling panel.
pub fn blobs(painter: &egui::Painter, rect: Rect, time: f32) {
    painter.rect_filled(rect, 0.0, BG);
    let blobs = [
        Blob {
            radius: 0.42,
            color: Color32::from_rgba_unmultiplied(77, 107, 254, 96),
            speed: 0.11,
            phase: 0.0,
            reach: 0.2,
        },
        Blob {
            radius: 0.34,
            color: Color32::from_rgba_unmultiplied(124, 92, 255, 76),
            speed: 0.07,
            phase: 2.1,
            reach: 0.28,
        },
        Blob {
            radius: 0.3,
            color: Color32::from_rgba_unmultiplied(47, 191, 166, 48),
            speed: 0.09,
            phase: 4.2,
            reach: 0.34,
        },
    ];
    let min_side = rect.height().min(rect.width());
    for blob in &blobs {
        let angle = time * blob.speed + blob.phase;
        let center =
            rect.center() + Vec2::new(angle.cos(), angle.sin() * 0.7) * min_side * blob.reach;
        glow(painter, center, min_side * blob.radius, blob.color);
    }
}

// Gradient painting --------------------------------------------------------
/// Diagonal two-color gradient inside a rounded rectangle.
pub fn gradient_rect(
    painter: &egui::Painter,
    rect: Rect,
    corner_radius: u8,
    from: Color32,
    to: Color32,
) {
    let mut mesh = egui::Mesh::default();
    let tl = rect.left_top();
    let tr = rect.right_top();
    let bl = rect.left_bottom();
    let br = rect.right_bottom();
    let index = mesh.vertices.len() as u32;
    mesh.colored_vertex(tl, from);
    mesh.colored_vertex(tr, to);
    mesh.colored_vertex(bl, to);
    mesh.colored_vertex(br, from);
    mesh.indices = vec![index, index + 1, index + 2, index + 2, index + 1, index + 3];
    painter.add(egui::Shape::mesh(mesh));
    // Rounded mask: stroke the same rect with the panel border to keep edges crisp.
    painter.rect_stroke(
        rect,
        CornerRadius::same(corner_radius),
        border_strong(),
        egui::StrokeKind::Outside,
    );
}

/// Primary pill button with the accent gradient, DeepSeek CTA style.
/// The optional icon draws left of the label.
pub fn gradient_button(
    ui: &mut egui::Ui,
    label: &str,
    icon: Option<fn(&egui::Painter, Rect, Color32)>,
) -> egui::Response {
    let text_width = ui.fonts_mut(|f| {
        f.layout_no_wrap(label.to_owned(), FontId::proportional(14.0), Color32::WHITE)
            .size()
            .x
    });
    let width = text_width + 34.0 + if icon.is_some() { 20.0 } else { 0.0 };
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 32.0), egui::Sense::click());
    let painter = ui.painter();
    let fill = if response.hovered() {
        ACCENT_HOVER
    } else {
        ACCENT
    };
    painter.rect_filled(rect, CornerRadius::same(16), fill);
    let right = rect.right_center() + Vec2::new(-rect.height() * 0.28, 0.0);
    let small = Rect::from_center_size(right, Vec2::splat(rect.height() * 0.5));
    gradient_rect(
        painter,
        small,
        99,
        Color32::from_rgba_unmultiplied(255, 255, 255, 70),
        Color32::from_rgba_unmultiplied(255, 255, 255, 20),
    );
    let mut text_x = rect.left() + 16.0;
    if let Some(draw) = icon {
        let icon_rect = Rect::from_center_size(
            egui::pos2(rect.left() + 20.0, rect.center().y),
            Vec2::splat(rect.height() - 14.0),
        );
        draw(painter, icon_rect, Color32::WHITE);
        text_x += 20.0;
    }
    painter.text(
        egui::pos2(text_x, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        FontId::proportional(14.0),
        Color32::WHITE,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Rounded brand mark: gradient square with the product initial.
pub fn brand_mark(painter: &egui::Painter, rect: Rect) {
    gradient_rect(painter, rect, 8, ACCENT, ACCENT_VIOLET);
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "S",
        FontId::proportional(rect.height() * 0.62),
        Color32::WHITE,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_is_dark_and_layered() {
        assert!(BG.r() < 30 && BG.g() < 30 && BG.b() < 40);
        assert!(panel_translucent().r() > BG.r());
        assert!(RAISED.r() > panel_translucent().r());
        assert!(ACCENT.b() > 200, "DeepSeek-style blue");
    }
}
