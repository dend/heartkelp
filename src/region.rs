use std::time::{Duration, Instant};

use eframe::egui;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Handle {
    TopLeft,
    Top,
    TopRight,
    Left,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

const HANDLE_SIZE: f32 = 8.0;
const HANDLE_HIT_RADIUS: f32 = 14.0;
const MIN_SEL_SIZE: f32 = 10.0;

fn handle_positions(r: egui::Rect) -> [(Handle, egui::Pos2); 8] {
    [
        (Handle::TopLeft, r.left_top()),
        (Handle::Top, egui::pos2(r.center().x, r.min.y)),
        (Handle::TopRight, r.right_top()),
        (Handle::Left, egui::pos2(r.min.x, r.center().y)),
        (Handle::Right, egui::pos2(r.max.x, r.center().y)),
        (Handle::BottomLeft, r.left_bottom()),
        (Handle::Bottom, egui::pos2(r.center().x, r.max.y)),
        (Handle::BottomRight, r.right_bottom()),
    ]
}

fn hit_test_handle(pos: egui::Pos2, sel_rect: egui::Rect) -> Option<Handle> {
    let mut best: Option<(Handle, f32)> = None;
    for (handle, center) in handle_positions(sel_rect) {
        let d = pos.distance(center);
        if d <= HANDLE_HIT_RADIUS {
            if best.is_none_or(|(_, bd)| d < bd) {
                best = Some((handle, d));
            }
        }
    }
    best.map(|(h, _)| h)
}

fn apply_resize(rect: egui::Rect, handle: Handle, pos: egui::Pos2) -> egui::Rect {
    let mut min = rect.min;
    let mut max = rect.max;

    match handle {
        Handle::TopLeft => {
            min.x = pos.x;
            min.y = pos.y;
        }
        Handle::Top => {
            min.y = pos.y;
        }
        Handle::TopRight => {
            max.x = pos.x;
            min.y = pos.y;
        }
        Handle::Left => {
            min.x = pos.x;
        }
        Handle::Right => {
            max.x = pos.x;
        }
        Handle::BottomLeft => {
            min.x = pos.x;
            max.y = pos.y;
        }
        Handle::Bottom => {
            max.y = pos.y;
        }
        Handle::BottomRight => {
            max.x = pos.x;
            max.y = pos.y;
        }
    }

    // Enforce minimum size
    if max.x - min.x < MIN_SEL_SIZE {
        match handle {
            Handle::TopLeft | Handle::Left | Handle::BottomLeft => {
                min.x = max.x - MIN_SEL_SIZE
            }
            _ => max.x = min.x + MIN_SEL_SIZE,
        }
    }
    if max.y - min.y < MIN_SEL_SIZE {
        match handle {
            Handle::TopLeft | Handle::Top | Handle::TopRight => {
                min.y = max.y - MIN_SEL_SIZE
            }
            _ => max.y = min.y + MIN_SEL_SIZE,
        }
    }

    egui::Rect::from_min_max(min, max)
}

fn cursor_for_handle(handle: Handle) -> egui::CursorIcon {
    match handle {
        Handle::TopLeft | Handle::BottomRight => egui::CursorIcon::ResizeNwSe,
        Handle::TopRight | Handle::BottomLeft => egui::CursorIcon::ResizeNeSw,
        Handle::Left | Handle::Right => egui::CursorIcon::ResizeHorizontal,
        Handle::Top | Handle::Bottom => egui::CursorIcon::ResizeVertical,
    }
}

pub struct RegionSelector {
    texture: Option<egui::TextureHandle>,
    drag_start: Option<egui::Pos2>,
    current_pos: Option<egui::Pos2>,
    selected_region: Option<(i32, i32, u32, u32)>,
    viewport_open: bool,
    screenshot: Option<egui::ColorImage>,
    /// Logical-coordinate rect of the selection after drag release.
    pending_rect: Option<egui::Rect>,
    /// Whether the selection has been confirmed (Enter pressed).
    confirmed: bool,
    /// When the confirmation animation started.
    confirmed_at: Option<Instant>,
    /// Physical-pixel region computed on confirmation.
    confirmed_region: Option<(i32, i32, u32, u32)>,
    /// Which handle is currently being dragged for resize.
    resize_handle: Option<Handle>,
    /// Physical-pixel region to pre-load on next overlay open.
    initial_region: Option<(i32, i32, u32, u32)>,
}

impl RegionSelector {
    pub fn new() -> Self {
        Self {
            texture: None,
            drag_start: None,
            current_pos: None,
            selected_region: None,
            viewport_open: false,
            screenshot: None,
            pending_rect: None,
            confirmed: false,
            confirmed_at: None,
            confirmed_region: None,
            resize_handle: None,
            initial_region: None,
        }
    }

    /// Store a physical-pixel region to pre-load when the overlay next opens.
    pub fn set_initial_region(&mut self, region: Option<(i32, i32, u32, u32)>) {
        self.initial_region = region;
    }

    pub fn set_screenshot(&mut self, image: egui::ColorImage) {
        self.screenshot = Some(image);
        self.texture = None;
        self.drag_start = None;
        self.current_pos = None;
        self.selected_region = None;
        self.viewport_open = true;
        self.pending_rect = None;
        self.confirmed = false;
        self.confirmed_at = None;
        self.confirmed_region = None;
        self.resize_handle = None;
        // initial_region is intentionally preserved — it was set before
        // the screenshot arrived and will be consumed on the first frame.
    }

    pub fn take_selected_region(&mut self) -> Option<(i32, i32, u32, u32)> {
        self.selected_region.take()
    }

    pub fn is_open(&self) -> bool {
        self.viewport_open
    }

    fn close(&mut self) {
        self.viewport_open = false;
        self.texture = None;
        self.screenshot = None;
        self.pending_rect = None;
        self.confirmed = false;
        self.confirmed_at = None;
        self.confirmed_region = None;
        self.drag_start = None;
        self.current_pos = None;
        self.resize_handle = None;
    }

    pub fn show(&mut self, ctx: &egui::Context) {
        if !self.viewport_open {
            return;
        }

        // Handle confirmation delay (close after animation)
        if self.confirmed {
            if let Some(at) = self.confirmed_at {
                if at.elapsed() > Duration::from_millis(350) {
                    self.selected_region = self.confirmed_region.take();
                    self.close();
                    return;
                }
            }
            ctx.request_repaint();
        }

        let Some(screenshot) = self.screenshot.as_ref() else {
            return;
        };

        let texture = self.texture.get_or_insert_with(|| {
            ctx.load_texture("screenshot", screenshot.clone(), egui::TextureOptions::LINEAR)
        });

        let viewport_id = egui::ViewportId::from_hash_of("region_selector");

        let tex_id = texture.id();
        let tex_size = texture.size_vec2();
        let drag_start = self.drag_start;
        let current_pos = self.current_pos;
        let pending_rect = self.pending_rect;
        let confirmed = self.confirmed;
        let resize_handle = self.resize_handle;

        let mut new_drag_start = self.drag_start;
        let mut new_current_pos = self.current_pos;
        let mut new_pending_rect = self.pending_rect;
        let mut new_resize_handle = self.resize_handle;
        let mut new_initial_region = self.initial_region;
        let mut should_close = false;
        let mut should_confirm = false;
        let mut confirm_region: Option<(i32, i32, u32, u32)> = None;

        ctx.show_viewport_immediate(
            viewport_id,
            egui::ViewportBuilder::default()
                .with_title("Select Region")
                .with_fullscreen(true)
                .with_decorations(false),
            |ctx, _class| {
                if ctx.input(|i| i.viewport().close_requested()) {
                    should_close = true;
                    return;
                }

                let panel_frame = egui::Frame::NONE;
                egui::CentralPanel::default().frame(panel_frame).show(ctx, |ui| {
                    let available = ui.available_size();
                    let (response, painter) =
                        ui.allocate_painter(available, egui::Sense::click_and_drag());
                    let rect = response.rect;

                    // Draw the screenshot
                    painter.image(
                        tex_id,
                        rect,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );

                    // Pre-load a previous region (physical → logical conversion)
                    if let Some((rx, ry, rw, rh)) = new_initial_region {
                        let ppp = ctx.pixels_per_point();
                        let sx = tex_size.x / available.x;
                        let sy = tex_size.y / available.y;
                        new_pending_rect = Some(egui::Rect::from_min_size(
                            egui::pos2(
                                rx as f32 / (ppp * sx),
                                ry as f32 / (ppp * sy),
                            ),
                            egui::vec2(
                                rw as f32 / (ppp * sx),
                                rh as f32 / (ppp * sy),
                            ),
                        ));
                        new_initial_region = None;
                    }

                    // Handle drag interaction (disabled during confirmation)
                    if !confirmed {
                        // Decide drag mode on drag start
                        if response.drag_started() {
                            if let Some(pos) = response.interact_pointer_pos() {
                                let current_pending = new_pending_rect.or(pending_rect);
                                let hit = current_pending
                                    .and_then(|pr| hit_test_handle(pos, pr));
                                if hit.is_some() {
                                    // Start handle resize
                                    new_resize_handle = hit;
                                } else {
                                    // Start new selection
                                    new_drag_start = Some(pos);
                                    new_current_pos = Some(pos);
                                    new_pending_rect = None;
                                    new_resize_handle = None;
                                }
                            }
                        }

                        if new_resize_handle.is_some() {
                            // Resize mode
                            if response.dragged() {
                                if let Some(pos) = response.interact_pointer_pos() {
                                    let handle = new_resize_handle.unwrap();
                                    if let Some(pr) = new_pending_rect.or(pending_rect) {
                                        new_pending_rect =
                                            Some(apply_resize(pr, handle, pos));
                                    }
                                }
                            }
                            if response.drag_stopped() {
                                new_resize_handle = None;
                            }
                        } else {
                            // New selection mode
                            if response.dragged() {
                                if let Some(pos) = response.interact_pointer_pos() {
                                    new_current_pos = Some(pos);
                                }
                            }
                            if response.drag_stopped() {
                                if let (Some(start), Some(end)) = (
                                    new_drag_start.or(drag_start),
                                    new_current_pos.or(current_pos),
                                ) {
                                    let r = egui::Rect::from_two_pos(start, end);
                                    if r.width() > 1.0 && r.height() > 1.0 {
                                        new_pending_rect = Some(r);
                                    }
                                }
                                new_drag_start = None;
                                new_current_pos = None;
                            }
                        }
                    }

                    // Determine what selection rect to display
                    let is_new_drag = new_drag_start.is_some();
                    let sel = if is_new_drag {
                        match (new_drag_start, new_current_pos) {
                            (Some(s), Some(e)) => Some(egui::Rect::from_two_pos(s, e)),
                            _ => None,
                        }
                    } else {
                        new_pending_rect.or(pending_rect)
                    };

                    let darkening = egui::Color32::from_black_alpha(160);
                    let has_pending = !is_new_drag && sel.is_some() && new_resize_handle.is_none();

                    if let Some(sel_rect) = sel {
                        // Darken areas outside selection (four edge strips)
                        // Top
                        painter.rect_filled(
                            egui::Rect::from_min_max(
                                rect.min,
                                egui::pos2(rect.max.x, sel_rect.min.y),
                            ),
                            0.0,
                            darkening,
                        );
                        // Bottom
                        painter.rect_filled(
                            egui::Rect::from_min_max(
                                egui::pos2(rect.min.x, sel_rect.max.y),
                                rect.max,
                            ),
                            0.0,
                            darkening,
                        );
                        // Left
                        painter.rect_filled(
                            egui::Rect::from_min_max(
                                egui::pos2(rect.min.x, sel_rect.min.y),
                                egui::pos2(sel_rect.min.x, sel_rect.max.y),
                            ),
                            0.0,
                            darkening,
                        );
                        // Right
                        painter.rect_filled(
                            egui::Rect::from_min_max(
                                egui::pos2(sel_rect.max.x, sel_rect.min.y),
                                egui::pos2(rect.max.x, sel_rect.max.y),
                            ),
                            0.0,
                            darkening,
                        );

                        if confirmed {
                            // Confirmation style: solid teal border
                            let confirm_color = egui::Color32::from_rgb(0, 200, 140);
                            painter.rect_stroke(
                                sel_rect,
                                0.0,
                                egui::Stroke::new(3.0, confirm_color),
                                egui::StrokeKind::Outside,
                            );

                            let label_pos =
                                egui::pos2(sel_rect.center().x, sel_rect.min.y - 14.0);
                            painter.text(
                                label_pos,
                                egui::Align2::CENTER_BOTTOM,
                                "\u{2713} Selected",
                                egui::FontId::proportional(14.0),
                                confirm_color,
                            );

                            ctx.request_repaint();
                        } else {
                            // Normal selection style: dashed border with handles
                            let white = egui::Color32::WHITE;
                            let shadow = egui::Color32::from_black_alpha(120);
                            draw_dashed_rect(
                                &painter, sel_rect, white, shadow, 2.0, 6.0, 4.0,
                            );

                            // Draw all 8 handles (corners + edge midpoints)
                            for (_, center) in handle_positions(sel_rect) {
                                painter.rect_filled(
                                    egui::Rect::from_center_size(
                                        center,
                                        egui::vec2(HANDLE_SIZE, HANDLE_SIZE),
                                    ),
                                    1.0,
                                    white,
                                );
                            }

                            // Cursor feedback when hovering a handle
                            if !response.dragged() {
                                if let Some(hover) = ui.input(|i| i.pointer.hover_pos()) {
                                    if let Some(h) = hit_test_handle(hover, sel_rect) {
                                        ui.ctx().set_cursor_icon(cursor_for_handle(h));
                                    }
                                }
                            } else if let Some(h) = new_resize_handle.or(resize_handle) {
                                ui.ctx().set_cursor_icon(cursor_for_handle(h));
                            }

                            // Size label
                            let w = sel_rect.width().abs();
                            let h = sel_rect.height().abs();
                            if w > 30.0 && h > 20.0 {
                                let ppp = ctx.pixels_per_point();
                                let sx = tex_size.x / available.x;
                                let sy = tex_size.y / available.y;
                                let label = format!(
                                    "{}x{}",
                                    (w * ppp * sx) as u32,
                                    (h * ppp * sy) as u32
                                );
                                let label_pos =
                                    egui::pos2(sel_rect.center().x, sel_rect.min.y - 12.0);
                                painter.text(
                                    label_pos,
                                    egui::Align2::CENTER_BOTTOM,
                                    label,
                                    egui::FontId::proportional(13.0),
                                    white,
                                );
                            }

                            // Hint text when pending (not actively dragging)
                            if has_pending {
                                let hint_pos =
                                    egui::pos2(sel_rect.center().x, sel_rect.max.y + 16.0);
                                painter.text(
                                    hint_pos,
                                    egui::Align2::CENTER_TOP,
                                    "Enter to confirm \u{2022} Escape to cancel \u{2022} Drag to redraw",
                                    egui::FontId::proportional(13.0),
                                    egui::Color32::from_white_alpha(160),
                                );
                            }
                        }
                    } else {
                        // No selection yet — darken the entire screen
                        painter.rect_filled(rect, 0.0, darkening);

                        // Show hint text in the center
                        painter.text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "Drag to select a region",
                            egui::FontId::proportional(22.0),
                            egui::Color32::WHITE,
                        );
                        painter.text(
                            rect.center() + egui::vec2(0.0, 30.0),
                            egui::Align2::CENTER_CENTER,
                            "Press Escape to cancel",
                            egui::FontId::proportional(14.0),
                            egui::Color32::from_white_alpha(140),
                        );

                        // Draw crosshairs at cursor position for feedback
                        if let Some(pos) = ui.input(|i| i.pointer.hover_pos()) {
                            let hair_color = egui::Color32::from_white_alpha(80);
                            let hair_stroke = egui::Stroke::new(1.0, hair_color);
                            painter.line_segment(
                                [egui::pos2(pos.x, rect.min.y), egui::pos2(pos.x, rect.max.y)],
                                hair_stroke,
                            );
                            painter.line_segment(
                                [egui::pos2(rect.min.x, pos.y), egui::pos2(rect.max.x, pos.y)],
                                hair_stroke,
                            );
                        }
                    }

                    // Confirmation via Enter key
                    if !confirmed && has_pending {
                        if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
                            let final_rect = sel.unwrap();
                            let ppp = ctx.pixels_per_point();
                            let scale_x = tex_size.x / available.x;
                            let scale_y = tex_size.y / available.y;

                            let x = (final_rect.min.x * ppp * scale_x) as i32;
                            let y = (final_rect.min.y * ppp * scale_y) as i32;
                            let w = (final_rect.width() * ppp * scale_x) as u32;
                            let h = (final_rect.height() * ppp * scale_y) as u32;

                            if w > 0 && h > 0 {
                                should_confirm = true;
                                confirm_region = Some((x, y, w, h));
                            }
                        }
                    }

                    // Allow Escape to cancel
                    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                        should_close = true;
                    }
                });
            },
        );

        self.drag_start = new_drag_start;
        self.current_pos = new_current_pos;
        self.pending_rect = new_pending_rect;
        self.resize_handle = new_resize_handle;
        self.initial_region = new_initial_region;

        if should_confirm {
            self.confirmed = true;
            self.confirmed_at = Some(Instant::now());
            self.confirmed_region = confirm_region;
        }

        if should_close && !self.confirmed {
            self.close();
        }
    }
}

/// Draw a dashed rectangle with a drop-shadow effect.
pub fn draw_dashed_rect(
    painter: &egui::Painter,
    rect: egui::Rect,
    color: egui::Color32,
    shadow_color: egui::Color32,
    width: f32,
    dash_len: f32,
    gap_len: f32,
) {
    let edges: [(egui::Pos2, egui::Pos2); 4] = [
        (rect.left_top(), rect.right_top()),
        (rect.right_top(), rect.right_bottom()),
        (rect.right_bottom(), rect.left_bottom()),
        (rect.left_bottom(), rect.left_top()),
    ];

    for (start, end) in edges {
        let dir = end - start;
        let length = dir.length();
        if length < 1.0 {
            continue;
        }
        let unit = dir / length;
        let step = dash_len + gap_len;
        let mut t = 0.0;

        while t < length {
            let seg_end = (t + dash_len).min(length);
            let p0 = start + unit * t;
            let p1 = start + unit * seg_end;

            // Shadow slightly offset
            painter.line_segment(
                [p0 + egui::vec2(1.0, 1.0), p1 + egui::vec2(1.0, 1.0)],
                egui::Stroke::new(width, shadow_color),
            );
            // Main dash
            painter.line_segment([p0, p1], egui::Stroke::new(width, color));

            t += step;
        }
    }
}
