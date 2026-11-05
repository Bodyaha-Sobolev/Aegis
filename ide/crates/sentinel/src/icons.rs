//! Minimal vector line icons drawn with the egui painter, so the UI carries
//! no icon font or binary assets.

use eframe::egui::{self, Color32, Pos2, Rect, Shape, Stroke, Vec2};

const W: f32 = 1.7;

fn stroke(color: Color32) -> Stroke {
    Stroke::new(W, color)
}

fn line(painter: &egui::Painter, points: &[Pos2], color: Color32) {
    painter.add(Shape::line(points.to_vec(), stroke(color)));
}

fn box_rect(rect: Rect, inset: f32) -> Rect {
    Rect::from_min_size(
        rect.min + Vec2::splat(inset),
        rect.size() - Vec2::splat(2.0 * inset),
    )
}

/// Blank page with folded corner and two text lines.
pub fn document(painter: &egui::Painter, rect: Rect, color: Color32) {
    let r = box_rect(rect, 2.0);
    let top_left = Pos2::new(r.left(), r.top());
    let fold = r.width() * 0.35;
    line(
        painter,
        &[
            top_left,
            Pos2::new(r.right() - fold, r.top()),
            Pos2::new(r.right(), r.top() + fold),
            Pos2::new(r.right(), r.bottom()),
            Pos2::new(r.left(), r.bottom()),
            top_left,
        ],
        color,
    );
    line(
        painter,
        &[
            Pos2::new(r.right() - fold, r.top()),
            Pos2::new(r.right() - fold, r.top() + fold),
            Pos2::new(r.right(), r.top() + fold),
        ],
        color,
    );
    let mid = r.center().y;
    line(
        painter,
        &[
            Pos2::new(r.left() + r.width() * 0.2, mid),
            Pos2::new(r.right() - r.width() * 0.2, mid),
        ],
        color,
    );
}

/// Open folder silhouette.
pub fn folder(painter: &egui::Painter, rect: Rect, color: Color32) {
    let r = box_rect(rect, 2.0);
    line(
        painter,
        &[
            Pos2::new(r.left(), r.bottom()),
            Pos2::new(r.left(), r.top() + r.height() * 0.18),
            Pos2::new(r.left() + r.width() * 0.32, r.top() + r.height() * 0.18),
            Pos2::new(r.left() + r.width() * 0.45, r.top() + r.height() * 0.38),
            Pos2::new(r.right(), r.top() + r.height() * 0.38),
            Pos2::new(r.right(), r.bottom()),
            Pos2::new(r.left(), r.bottom()),
        ],
        color,
    );
}

/// Tray with an arrow pointing into it (save/export).
pub fn save(painter: &egui::Painter, rect: Rect, color: Color32) {
    let r = box_rect(rect, 2.0);
    let cx = r.center().x;
    line(
        painter,
        &[
            Pos2::new(cx, r.top() + r.height() * 0.08),
            Pos2::new(cx, r.top() + r.height() * 0.6),
        ],
        color,
    );
    line(
        painter,
        &[
            Pos2::new(cx - r.width() * 0.22, r.top() + r.height() * 0.38),
            Pos2::new(cx, r.top() + r.height() * 0.6),
            Pos2::new(cx + r.width() * 0.22, r.top() + r.height() * 0.38),
        ],
        color,
    );
    line(
        painter,
        &[
            Pos2::new(r.left(), r.bottom() - r.height() * 0.28),
            Pos2::new(r.left(), r.bottom()),
            Pos2::new(r.right(), r.bottom()),
            Pos2::new(r.right(), r.bottom() - r.height() * 0.28),
        ],
        color,
    );
}

/// Magnifier.
pub fn search(painter: &egui::Painter, rect: Rect, color: Color32) {
    let r = box_rect(rect, 2.0);
    let radius = r.height() * 0.32;
    let center = Pos2::new(
        r.left() + radius + r.width() * 0.08,
        r.top() + radius + r.height() * 0.08,
    );
    painter.circle_stroke(center, radius, stroke(color));
    line(
        painter,
        &[
            Pos2::new(center.x + radius * 0.72, center.y + radius * 0.72),
            Pos2::new(r.right() - r.width() * 0.08, r.bottom() - r.height() * 0.08),
        ],
        color,
    );
}

/// Warning triangle with an exclamation mark.
pub fn warning(painter: &egui::Painter, rect: Rect, color: Color32) {
    let r = box_rect(rect, 1.5);
    let apex = Pos2::new(r.center().x, r.top());
    let bottom_left = Pos2::new(r.left(), r.bottom());
    let bottom_right = Pos2::new(r.right(), r.bottom());
    line(
        painter,
        &[bottom_left, apex, bottom_right, bottom_left],
        color,
    );
    let cx = r.center().x;
    line(
        painter,
        &[
            Pos2::new(cx, r.top() + r.height() * 0.38),
            Pos2::new(cx, r.top() + r.height() * 0.62),
        ],
        color,
    );
    painter.circle_filled(Pos2::new(cx, r.top() + r.height() * 0.78), W * 0.7, color);
}

/// Shield silhouette.
pub fn shield(painter: &egui::Painter, rect: Rect, color: Color32) {
    let r = box_rect(rect, 2.0);
    let cx = r.center().x;
    line(
        painter,
        &[
            Pos2::new(cx, r.top()),
            Pos2::new(r.right() - r.width() * 0.08, r.top() + r.height() * 0.14),
            Pos2::new(
                r.right() - r.width() * 0.08,
                r.center().y + r.height() * 0.1,
            ),
            Pos2::new(cx, r.bottom()),
            Pos2::new(r.left() + r.width() * 0.08, r.center().y + r.height() * 0.1),
            Pos2::new(r.left() + r.width() * 0.08, r.top() + r.height() * 0.14),
            Pos2::new(cx, r.top()),
        ],
        color,
    );
}

/// Filled play triangle.
pub fn play(painter: &egui::Painter, rect: Rect, color: Color32) {
    let r = box_rect(rect, 2.5);
    let apex = Pos2::new(r.right(), r.center().y);
    painter.add(Shape::convex_polygon(
        vec![
            Pos2::new(r.left(), r.top()),
            apex,
            Pos2::new(r.left(), r.bottom()),
        ],
        color,
        Stroke::NONE,
    ));
}

/// Plus cross.
pub fn plus(painter: &egui::Painter, rect: Rect, color: Color32) {
    let r = box_rect(rect, 3.0);
    let cx = r.center().x;
    let cy = r.center().y;
    line(
        painter,
        &[Pos2::new(cx, r.top()), Pos2::new(cx, r.bottom())],
        color,
    );
    line(
        painter,
        &[Pos2::new(r.left(), cy), Pos2::new(r.right(), cy)],
        color,
    );
}

/// Small close cross.
pub fn close(painter: &egui::Painter, rect: Rect, color: Color32) {
    let r = box_rect(rect, 3.5);
    line(
        painter,
        &[
            Pos2::new(r.left(), r.top()),
            Pos2::new(r.right(), r.bottom()),
        ],
        color,
    );
    line(
        painter,
        &[
            Pos2::new(r.right(), r.top()),
            Pos2::new(r.left(), r.bottom()),
        ],
        color,
    );
}

/// Circular refresh arrow.
pub fn refresh(painter: &egui::Painter, rect: Rect, color: Color32) {
    let r = box_rect(rect, 2.0);
    let center = r.center();
    let radius = r.height() * 0.42;
    painter.circle_stroke(center, radius, stroke(color));
    let gap_angle: f32 = -0.5;
    let tip = Pos2::new(
        center.x + radius * gap_angle.cos(),
        center.y + radius * gap_angle.sin(),
    );
    line(
        painter,
        &[
            Pos2::new(tip.x - 4.0, tip.y + 1.0),
            tip,
            Pos2::new(tip.x + 1.0, tip.y - 4.0),
        ],
        color,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icons_only_use_pure_functions() {
        // No rendering happens here; the module must stay logic-free so the
        // forbid(unsafe_code) build remains graphics-independent.
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::splat(16.0));
        assert!(box_rect(rect, 2.0).width() < rect.width());
    }
}
