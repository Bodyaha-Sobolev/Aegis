//! Sentinel visual system: the light DeepSeek landing palette — near-white
//! base, soft periwinkle-blue clouds and a faint grid — plus system
//! typography and shared widgets.

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontId, Margin, Rect, Stroke, Vec2,
};

// Palette -----------------------------------------------------------------
/// Near-white base behind everything; the blue clouds drift on it.
pub const BG: Color32 = Color32::from_rgb(243, 245, 249);
/// Panels sit slightly translucent so the living background stays visible.
fn panel_translucent() -> Color32 {
    Color32::from_rgba_unmultiplied(252, 253, 255, 235)
}
/// Elevated cards, toolbars, popups.
pub const RAISED: Color32 = Color32::from_rgb(255, 255, 255);
/// Hover fill on quiet controls.
pub const HOVER: Color32 = Color32::from_rgb(237, 240, 246);
/// Active/pressed fill.
pub const ACTIVE: Color32 = Color32::from_rgb(226, 232, 243);
/// DeepSeek blue accent (#4d6bfe).
pub const ACCENT: Color32 = Color32::from_rgb(77, 107, 254);
/// Accent on hover (#5686fe).
pub const ACCENT_HOVER: Color32 = Color32::from_rgb(86, 134, 254);
/// Soft accent tint for selection and quiet accent fills.
pub const ACCENT_DIM: Color32 = Color32::from_rgb(214, 226, 255);
/// Secondary gradient stop for brand marks and primary buttons.
pub const ACCENT_VIOLET: Color32 = Color32::from_rgb(124, 92, 255);
/// Text hierarchy — the exact alias values from DeepSeek desktop CSS.
pub const TEXT: Color32 = Color32::from_rgb(15, 17, 21);
pub const TEXT_2: Color32 = Color32::from_rgb(97, 102, 107);
pub const TEXT_3: Color32 = Color32::from_rgb(129, 133, 140);
pub const DANGER: Color32 = Color32::from_rgb(209, 36, 47);
pub const WARNING: Color32 = Color32::from_rgb(154, 103, 0);
pub const OK: Color32 = Color32::from_rgb(26, 127, 55);
/// Editor line numbers.
pub const GUTTER: Color32 = Color32::from_rgb(176, 183, 195);

pub fn border() -> Stroke {
    Stroke::new(1.0, Color32::from_rgba_unmultiplied(0, 0, 0, 26))
}

pub fn border_strong() -> Stroke {
    Stroke::new(1.0, Color32::from_rgba_unmultiplied(0, 0, 0, 41))
}

// Typography --------------------------------------------------------------
/// DM Sans is DeepSeek's interface font (SIL OFL-1.1, latin subset); the
/// embedded copies keep the IDE looking right without any system dependency.
const DM_SANS_400: &[u8] = include_bytes!("../assets/fonts/dm-sans-400.ttf");
const DM_SANS_500: &[u8] = include_bytes!("../assets/fonts/dm-sans-500.ttf");
const DM_SANS_700: &[u8] = include_bytes!("../assets/fonts/dm-sans-700.ttf");
const SEGOE_PATHS: &[&str] = &["C:\\Windows\\Fonts\\segoeui.ttf"];
const SEGOE_BOLD_PATHS: &[&str] = &["C:\\Windows\\Fonts\\segoeuib.ttf"];
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

/// Install typography — embedded DM Sans with a Segoe UI fallback for the
/// glyphs DM Sans does not cover (Cyrillic UI strings) — and the widget visuals.
pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "dm_sans_400".into(),
        FontData::from_owned(DM_SANS_400.to_vec()).into(),
    );
    fonts.font_data.insert(
        "dm_sans_500".into(),
        FontData::from_owned(DM_SANS_500.to_vec()).into(),
    );
    fonts.font_data.insert(
        "dm_sans_700".into(),
        FontData::from_owned(DM_SANS_700.to_vec()).into(),
    );
    let mut regular = vec!["dm_sans_400".to_owned()];
    if let Some(bytes) = read_font(SEGOE_PATHS) {
        fonts
            .font_data
            .insert("segoe_ui".into(), FontData::from_owned(bytes).into());
        regular.push("segoe_ui".into());
    }
    fonts
        .families
        .insert(egui::FontFamily::Proportional, regular.clone());
    fonts.families.insert(
        egui::FontFamily::Name("Medium".into()),
        vec!["dm_sans_500".into()]
            .into_iter()
            .chain(regular.iter().skip(1).cloned())
            .collect(),
    );
    let mut bold = vec!["dm_sans_700".to_owned()];
    if let Some(bytes) = read_font(SEGOE_BOLD_PATHS) {
        fonts
            .font_data
            .insert("segoe_ui_bold".into(), FontData::from_owned(bytes).into());
        bold.push("segoe_ui_bold".into());
    } else if regular.len() > 1 {
        bold.push("segoe_ui".into());
    }
    fonts
        .families
        .insert(egui::FontFamily::Name("Bold".into()), bold);
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

    let mut visuals = egui::Visuals::light();
    visuals.panel_fill = panel_translucent();
    visuals.window_fill = RAISED;
    // The editor surface is a touch translucent so the clouds stay visible.
    visuals.extreme_bg_color = Color32::from_rgba_unmultiplied(245, 246, 250, 178);
    visuals.faint_bg_color = Color32::from_rgb(240, 242, 247);
    visuals.selection.bg_fill = ACCENT_DIM;
    visuals.selection.stroke.color = ACCENT;
    visuals.hyperlink_color = ACCENT;
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
    visuals.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    visuals.widgets.inactive.bg_stroke = Stroke::NONE;
    visuals.widgets.hovered.bg_fill = ACTIVE;
    visuals.widgets.hovered.weak_bg_fill = HOVER;
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT);
    visuals.widgets.hovered.bg_stroke = border();
    visuals.widgets.active.bg_fill = ACCENT_DIM;
    visuals.widgets.active.weak_bg_fill = ACTIVE;
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
        style.text_styles.insert(
            egui::TextStyle::Button,
            FontId::new(14.0, egui::FontFamily::Name("Medium".into())),
        );
        style
            .text_styles
            .insert(egui::TextStyle::Small, FontId::proportional(12.0));
        style.text_styles.insert(
            egui::TextStyle::Heading,
            FontId::new(19.0, egui::FontFamily::Name("Bold".into())),
        );
        style
            .text_styles
            .insert(egui::TextStyle::Monospace, FontId::monospace(14.5));
        style.text_styles.insert(
            egui::TextStyle::Name("Caption".into()),
            FontId::proportional(11.0),
        );
        style.text_styles.insert(
            egui::TextStyle::Name("Brand".into()),
            FontId::new(16.0, egui::FontFamily::Name("Bold".into())),
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
    fn palette_is_light_and_layered() {
        assert!(BG.r() > 240 && BG.g() > 240 && BG.b() > 244);
        assert!(RAISED.r() == 255, "panels and cards are pure white");
        assert!(TEXT.r() < 30, "dark text on the light surface");
        assert!(ACCENT.b() > 200, "DeepSeek-style blue");
    }
}
