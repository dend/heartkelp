use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use eframe::egui;

use crate::config::Config;
use crate::region::RegionSelector;
use crate::types::{CaptureMode, Command, Event, Frame, RecordingState};

const BG: egui::Color32 = egui::Color32::from_rgb(30, 30, 34);
const BTN: f32 = 32.0;

// --- Design Tokens ---

// Colors
const SURFACE: egui::Color32 = egui::Color32::from_gray(38);
const CONTROL: egui::Color32 = egui::Color32::from_gray(48);
const CONTROL_HV: egui::Color32 = egui::Color32::from_gray(62);
const ACTIVE_BG: egui::Color32 = egui::Color32::from_gray(65);
const DISABLED: egui::Color32 = egui::Color32::from_gray(35);
const BORDER: egui::Color32 = egui::Color32::from_gray(48);

const TEXT_PRIMARY: egui::Color32 = egui::Color32::WHITE;
const TEXT_SECONDARY: egui::Color32 = egui::Color32::from_gray(180);
const TEXT_MUTED: egui::Color32 = egui::Color32::from_gray(120);
const TEXT_DISABLED: egui::Color32 = egui::Color32::from_gray(70);

const RED: egui::Color32 = egui::Color32::from_rgb(200, 40, 40);
const RED_HV: egui::Color32 = egui::Color32::from_rgb(220, 50, 50);
const GREEN: egui::Color32 = egui::Color32::from_rgb(50, 140, 65);
const GREEN_HV: egui::Color32 = egui::Color32::from_rgb(60, 170, 80);
const ORANGE: egui::Color32 = egui::Color32::from_rgb(200, 130, 60);
const ACCENT: egui::Color32 = egui::Color32::from_rgb(100, 140, 230);
const ACCENT_BG: egui::Color32 = egui::Color32::from_rgb(40, 48, 65);
const ERROR_TEXT: egui::Color32 = egui::Color32::from_rgb(255, 120, 120);
const PAUSE_DOT: egui::Color32 = egui::Color32::from_rgb(200, 160, 40);

// Sizing
const BTN_SM: f32 = 28.0;
const BTN_ACTION_W: f32 = 76.0;
const BTN_ACTION_H: f32 = 32.0;
const RADIUS: f32 = 6.0;
const RADIUS_SM: f32 = 4.0;
const RADIUS_PILL: f32 = 12.0;

// Spacing
const GAP: f32 = 6.0;
const GAP_SM: f32 = 4.0;
const GAP_LG: f32 = 10.0;

// Font sizes
const FONT_XS: f32 = 10.0;
const FONT_SM: f32 = 11.0;
const FONT_MD: f32 = 12.0;
const FONT_LG: f32 = 14.0;

// Layout
const WIN_W: f32 = 288.0;       // idle / encoding window width
const WIN_W_REVIEW: f32 = 520.0; // review window width
const TITLE_H: f32 = 28.0;      // title bar height
const MARGIN_H: f32 = 16.0;     // horizontal panel margin
const MARGIN_V: f32 = 12.0;     // vertical panel margin
const CLOSE_SIZE: f32 = 20.0;   // title-bar close button hit area
const PROGRESS_W: f32 = 200.0;  // encoding progress bar width
const PROGRESS_H: f32 = 6.0;    // encoding progress bar height
const REVIEW_CONTROLS_H: f32 = 164.0; // playback row + timeline + buttons + spacing + bottom pad

pub struct App {
    state: RecordingState,
    cmd_tx: mpsc::Sender<Command>,
    event_rx: mpsc::Receiver<Event>,
    fps: u8,
    region_selector: RegionSelector,
    selected_region: Option<(i32, i32, u32, u32)>,
    frame_count: usize,
    recording_start: Option<Instant>,
    last_error: Option<String>,
    screen_size: Option<(u32, u32)>,
    active_mode: Option<CaptureMode>,
    use_region: bool,
    paused: bool,
    pause_start: Option<Instant>,
    paused_duration: Duration,
    first_frame: bool,
    encoding_progress: usize,
    // Review state
    review_frames: Vec<Frame>,
    review_fps: u8,
    review_playhead: usize,
    review_playing: bool,
    review_trim_start: usize,
    review_trim_end: usize,
    review_texture: Option<egui::TextureHandle>,
    review_last_tick: Option<Instant>,
    review_dragging: Option<TrimDrag>,
    encoding_in_progress: bool,
    encoding_total_frames: usize,
    review_saved_path: Option<PathBuf>,
    // Deferred screenshot (wait for highlight viewport to disappear)
    pending_screenshot: Option<Instant>,
    // Settings
    show_settings: bool,
    config: Config,
    settings_fps: u8,
    settings_output_dir: String,
    settings_error: Option<String>,
}

#[derive(Clone, Copy, PartialEq)]
enum TrimDrag {
    Start,
    End,
    Playhead,
}

impl App {
    pub fn new(cmd_tx: mpsc::Sender<Command>, event_rx: mpsc::Receiver<Event>) -> Self {
        let config = Config::load();
        let fps = config.default_fps;
        Self {
            state: RecordingState::Idle,
            cmd_tx,
            event_rx,
            fps,
            region_selector: RegionSelector::new(),
            selected_region: None,
            frame_count: 0,
            recording_start: None,
            last_error: None,
            screen_size: None,
            active_mode: None,
            use_region: false,
            paused: false,
            pause_start: None,
            paused_duration: Duration::ZERO,
            first_frame: true,
            encoding_progress: 0,
            review_frames: Vec::new(),
            review_fps: 30,
            review_playhead: 0,
            review_playing: false,
            review_trim_start: 0,
            review_trim_end: 0,
            review_texture: None,
            review_last_tick: None,
            review_dragging: None,
            encoding_in_progress: false,
            encoding_total_frames: 0,
            review_saved_path: None,
            pending_screenshot: None,
            show_settings: false,
            config,
            settings_fps: fps,
            settings_output_dir: String::new(),
            settings_error: None,
        }
    }

    fn active_duration(&self) -> Duration {
        let Some(start) = self.recording_start else {
            return Duration::ZERO;
        };
        let total = start.elapsed();
        let paused = self.paused_duration
            + self
                .pause_start
                .map_or(Duration::ZERO, |s| s.elapsed());
        total.saturating_sub(paused)
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.first_frame {
            self.first_frame = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }

        // Deferred screenshot: wait for compositor to remove highlight viewport
        if let Some(when) = self.pending_screenshot {
            if when.elapsed() >= Duration::from_millis(150) {
                self.pending_screenshot = None;
                let _ = self.cmd_tx.send(Command::TakeScreenshot);
            } else {
                ctx.request_repaint();
            }
        }

        let state_before = self.state.clone();

        // Process backend events
        while let Ok(event) = self.event_rx.try_recv() {
            match event {
                Event::ScreenshotReady(image) => {
                    self.screen_size =
                        Some((image.size[0] as u32, image.size[1] as u32));
                    self.region_selector.set_screenshot(image);
                    self.state = RecordingState::SelectingRegion;
                }
                Event::RecordingStarted => {
                    self.state = RecordingState::Recording;
                    self.frame_count = 0;
                    self.recording_start = Some(Instant::now());
                    self.paused = false;
                    self.pause_start = None;
                    self.paused_duration = Duration::ZERO;
                }
                Event::FrameCaptured(count) => {
                    self.frame_count = count;
                }
                Event::RecordingReady { frames, fps } => {
                    let len = frames.len();
                    self.review_frames = frames;
                    self.review_fps = fps;
                    self.review_playhead = 0;
                    self.review_playing = false;
                    self.review_trim_start = 0;
                    self.review_trim_end = len;
                    self.review_texture = None;
                    self.review_last_tick = None;
                    self.review_dragging = None;
                    self.encoding_in_progress = false;
                    self.encoding_total_frames = 0;
                    self.review_saved_path = None;
                    self.state = RecordingState::Reviewing;
                }
                Event::EncodingProgress(n) => {
                    self.encoding_progress = n;
                }
                Event::RecordingFinished(path) => {
                    self.encoding_in_progress = false;
                    self.review_saved_path = Some(path);
                }
                Event::Error(msg) => {
                    self.last_error = Some(msg);
                    self.encoding_in_progress = false;
                    self.state = RecordingState::Idle;
                    self.recording_start = None;
                    self.active_mode = None;
                    self.paused = false;
                    self.pause_start = None;
                }
            }
        }

        // Check if region was selected
        if let Some(region) = self.region_selector.take_selected_region() {
            self.selected_region = Some(region);
            self.state = RecordingState::Idle;
        } else if self.state == RecordingState::SelectingRegion
            && !self.region_selector.is_open()
        {
            self.state = RecordingState::Idle;
        }

        // ESC in idle clears the selected region.
        if self.state == RecordingState::Idle
            && !self.region_selector.is_open()
            && self.selected_region.is_some()
            && ctx.input(|i| i.key_pressed(egui::Key::Escape))
        {
            self.selected_region = None;
        }

        // Advance review playback
        if self.state == RecordingState::Reviewing && self.review_playing {
            let now = Instant::now();
            if let Some(last) = self.review_last_tick {
                let elapsed = now.duration_since(last).as_secs_f64();
                let frame_dur = 1.0 / self.review_fps as f64;
                let advance = (elapsed / frame_dur) as usize;
                if advance > 0 {
                    self.review_last_tick = Some(now);
                    let range = self.review_trim_end - self.review_trim_start;
                    if range > 0 {
                        let offset = self.review_playhead - self.review_trim_start;
                        self.review_playhead =
                            self.review_trim_start + (offset + advance) % range;
                    }
                }
            } else {
                self.review_last_tick = Some(now);
            }
            ctx.request_repaint();
        }

        // Handle state transitions
        if self.state != state_before {
            self.apply_state_transition(ctx, &state_before);
        }

        // During recording, main window is passthrough — controls are in a separate viewport
        let recording = self.state == RecordingState::Recording;
        ctx.send_viewport_cmd(egui::ViewportCommand::MousePassthrough(recording));

        self.region_selector.show(ctx);
        self.show_region_highlight(ctx);
        self.show_title_bar(ctx);

        let panel_frame = if self.state == RecordingState::Recording {
            egui::Frame::NONE
        } else {
            egui::Frame::NONE
                .fill(BG)
                .inner_margin(egui::Margin::symmetric(MARGIN_H as i8, MARGIN_V as i8))
        };

        egui::CentralPanel::default()
            .frame(panel_frame)
            .show(ctx, |ui| {
                ui.with_layout(
                    egui::Layout::top_down(egui::Align::Center),
                    |ui| match self.state {
                        RecordingState::Idle => self.show_idle_ui(ui),
                        RecordingState::SelectingRegion => {
                            self.show_selecting_ui(ui)
                        }
                        RecordingState::Recording => {
                            self.show_recording_ui(ui)
                        }
                        RecordingState::Reviewing => {
                            self.show_reviewing_ui(ui)
                        }
                        RecordingState::Encoding => {
                            self.show_encoding_ui(ui)
                        }
                    },
                );
            });

        if self.show_settings {
            self.show_settings_viewport(ctx);
        }
    }
}

impl App {
    fn apply_state_transition(
        &self,
        ctx: &egui::Context,
        prev: &RecordingState,
    ) {
        match (&self.state, prev) {
            (RecordingState::Recording, _) => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            }
            (RecordingState::Reviewing, _) => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(
                    egui::vec2(WIN_W_REVIEW, 545.0),
                ));
                ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                    egui::WindowLevel::Normal,
                ));
            }
            (
                RecordingState::Idle,
                RecordingState::Encoding
                | RecordingState::Recording
                | RecordingState::Reviewing,
            ) => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                // Size will be set by show_idle_ui based on minimap visibility
            }
            _ => {}
        }
    }

    fn show_title_bar(&mut self, ctx: &egui::Context) {
        if self.state == RecordingState::Recording {
            return;
        }

        let frame = egui::Frame::NONE
            .fill(BG)
            .inner_margin(egui::Margin::symmetric((MARGIN_H / 2.0) as i8, 0));

        egui::TopBottomPanel::top("title_bar")
            .exact_height(TITLE_H)
            .frame(frame)
            .show(ctx, |ui| {
                let rect = ui.max_rect();

                let response = ui.interact(
                    rect,
                    ui.id().with("drag"),
                    egui::Sense::drag(),
                );
                if response.drag_started() {
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }

                ui.painter().text(
                    egui::pos2(rect.left() + 6.0, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    "Heartkelp",
                    egui::FontId::proportional(FONT_MD),
                    TEXT_MUTED,
                );

                let close_rect = egui::Rect::from_center_size(
                    egui::pos2(
                        rect.right() - CLOSE_SIZE / 2.0 - 2.0,
                        rect.center().y,
                    ),
                    egui::vec2(CLOSE_SIZE, CLOSE_SIZE),
                );
                let close = ui.interact(
                    close_rect,
                    ui.id().with("close"),
                    egui::Sense::click(),
                );
                let color = if close.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    egui::Color32::from_rgb(255, 80, 80)
                } else {
                    TEXT_MUTED
                };
                ui.painter().text(
                    close_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "\u{00d7}",
                    egui::FontId::proportional(FONT_LG),
                    color,
                );
                if close.clicked() {
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::Close);
                }

                ui.painter().line_segment(
                    [
                        egui::pos2(rect.left(), rect.bottom()),
                        egui::pos2(rect.right(), rect.bottom()),
                    ],
                    egui::Stroke::new(1.0, BORDER),
                );
            });
    }

    fn show_idle_ui(&mut self, ui: &mut egui::Ui) {
        // Compute window height dynamically based on visible content
        let has_minimap = self.use_region && self.selected_region.is_some();
        let has_error = self.last_error.is_some();
        let mut target_h = TITLE_H + MARGIN_V * 2.0 + BTN + 2.0;
        if has_minimap {
            let (sw, sh) = self.screen_size.unwrap_or((1920, 1080));
            let map_h = 100.0 * sh as f32 / sw as f32;
            target_h += GAP + map_h + GAP_SM + 16.0;
        }
        if has_error {
            target_h += GAP + 16.0;
        }
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::InnerSize(
            egui::vec2(WIN_W, target_h),
        ));

        // Controls row: [Full|Region] [crop] [record] [gear]
        let content_w = 92.0 + BTN * 3.0 + GAP * 3.0;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            let pad = (ui.available_width() - content_w).max(0.0) / 2.0;
            ui.add_space(pad);

            // Segmented mode control
            {
                let seg_w = 46.0;
                let total_w = seg_w * 2.0;
                let (rect, _) = ui.allocate_exact_size(
                    egui::vec2(total_w, BTN),
                    egui::Sense::hover(),
                );

                // Outer background
                ui.painter().rect_filled(
                    rect,
                    RADIUS,
                    SURFACE,
                );

                // Segment rects
                let left_rect = egui::Rect::from_min_size(
                    rect.min,
                    egui::vec2(seg_w, BTN),
                );
                let right_rect = egui::Rect::from_min_size(
                    egui::pos2(rect.left() + seg_w, rect.top()),
                    egui::vec2(seg_w, BTN),
                );

                // Active pill
                let active_rect = if self.use_region {
                    right_rect
                } else {
                    left_rect
                };
                ui.painter().rect_filled(
                    active_rect.shrink(2.0),
                    RADIUS_SM,
                    ACTIVE_BG,
                );

                // Click handlers
                let left_resp = ui.interact(
                    left_rect,
                    ui.id().with("seg_full"),
                    egui::Sense::click(),
                );
                let right_resp = ui.interact(
                    right_rect,
                    ui.id().with("seg_region"),
                    egui::Sense::click(),
                );
                if left_resp.hovered() || right_resp.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if left_resp.clicked() {
                    self.use_region = false;
                }
                if right_resp.clicked() {
                    self.use_region = true;
                }

                // Labels
                ui.painter().text(
                    left_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "Full",
                    egui::FontId::proportional(FONT_SM),
                    if !self.use_region {
                        TEXT_PRIMARY
                    } else {
                        TEXT_MUTED
                    },
                );
                ui.painter().text(
                    right_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "Region",
                    egui::FontId::proportional(FONT_SM),
                    if self.use_region {
                        TEXT_PRIMARY
                    } else {
                        TEXT_MUTED
                    },
                );
            }

            // Select Region (crop icon)
            {
                let enabled = self.use_region;
                let (rect, resp) = ui.allocate_exact_size(
                    egui::vec2(BTN, BTN),
                    if enabled {
                        egui::Sense::click()
                    } else {
                        egui::Sense::hover()
                    },
                );
                if enabled {
                    let bg = if resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        CONTROL_HV
                    } else {
                        CONTROL
                    };
                    ui.painter().rect_filled(rect, RADIUS, bg);
                    paint_crop_icon(
                        ui.painter(),
                        rect,
                        TEXT_SECONDARY,
                    );
                    if resp.clicked() {
                        self.last_error = None;
                        self.selected_region = None;
                        self.region_selector.set_initial_region(None);
                        // Defer screenshot so the compositor has time to
                        // remove the region highlight viewport first.
                        self.pending_screenshot = Some(Instant::now());
                    }
                    resp.on_hover_text("Select Region");
                } else {
                    ui.painter().rect_filled(
                        rect,
                        RADIUS,
                        DISABLED,
                    );
                    paint_crop_icon(
                        ui.painter(),
                        rect,
                        TEXT_DISABLED,
                    );
                }
            }

            // Record button
            {
                let can_record =
                    !self.use_region || self.selected_region.is_some();
                let (rect, resp) = ui.allocate_exact_size(
                    egui::vec2(BTN, BTN),
                    egui::Sense::click(),
                );
                if can_record {
                    let bg = if resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        RED_HV
                    } else {
                        RED
                    };
                    ui.painter().rect_filled(rect, RADIUS, bg);
                    ui.painter().circle_filled(
                        rect.center(),
                        7.0,
                        TEXT_PRIMARY,
                    );
                } else {
                    ui.painter().rect_filled(
                        rect,
                        RADIUS,
                        CONTROL,
                    );
                    ui.painter().circle_filled(
                        rect.center(),
                        7.0,
                        egui::Color32::from_gray(80),
                    );
                }
                if can_record && resp.clicked() {
                    if self.use_region {
                        let (x, y, w, h) = self.selected_region.unwrap();
                        // Inset by border width so the dashed highlight
                        // overlay doesn't appear in the captured frames.
                        let inset: i32 = 4;
                        let ix = x + inset;
                        let iy = y + inset;
                        let iw = w.saturating_sub(2 * inset as u32);
                        let ih = h.saturating_sub(2 * inset as u32);
                        self.start_recording(CaptureMode::Region {
                            x: ix,
                            y: iy,
                            w: iw,
                            h: ih,
                        });
                    } else {
                        self.start_recording(CaptureMode::FullScreen);
                    }
                }
                resp.on_hover_text(if can_record {
                    "Start Recording"
                } else {
                    "Select a region first"
                });
            }

            // Settings (gear) button
            {
                let (rect, resp) = ui.allocate_exact_size(
                    egui::vec2(BTN, BTN),
                    egui::Sense::click(),
                );
                let bg = if resp.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    CONTROL_HV
                } else {
                    CONTROL
                };
                ui.painter().rect_filled(rect, RADIUS, bg);
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "\u{2699}",
                    egui::FontId::proportional(FONT_LG),
                    if resp.hovered() { TEXT_PRIMARY } else { TEXT_SECONDARY },
                );
                if resp.clicked() {
                    self.show_settings = !self.show_settings;
                    if self.show_settings {
                        self.settings_fps = self.config.default_fps;
                        self.settings_output_dir =
                            self.config.output_dir.to_string_lossy().into_owned();
                        self.settings_error = None;
                    }
                }
                resp.on_hover_text("Settings");
            }
        });

        // Region minimap + dimensions
        if self.use_region {
            if let Some((rx, ry, rw, rh)) = self.selected_region {
                let (sw, sh) = self.screen_size.unwrap_or((1920, 1080));
                let map_w = 100.0_f32;
                let map_h = map_w * sh as f32 / sw as f32;

                ui.add_space(GAP_SM);
                let (map_rect, _) = ui.allocate_exact_size(
                    egui::vec2(map_w, map_h),
                    egui::Sense::hover(),
                );
                let painter = ui.painter();

                // Screen outline
                painter.rect_stroke(
                    map_rect,
                    2.0,
                    egui::Stroke::new(1.0, CONTROL_HV),
                    egui::StrokeKind::Inside,
                );

                // Selected region (scaled to minimap)
                let sx = map_w / sw as f32;
                let sy = map_h / sh as f32;
                let sel_rect = egui::Rect::from_min_size(
                    egui::pos2(
                        map_rect.left() + rx as f32 * sx,
                        map_rect.top() + ry as f32 * sy,
                    ),
                    egui::vec2(rw as f32 * sx, rh as f32 * sy),
                );
                painter.rect_filled(
                    sel_rect,
                    0.0,
                    egui::Color32::from_rgba_unmultiplied(255, 255, 255, 25),
                );
                painter.rect_stroke(
                    sel_rect,
                    0.0,
                    egui::Stroke::new(1.0, egui::Color32::WHITE),
                    egui::StrokeKind::Inside,
                );

                // Dimensions label
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new(format!("{rw}\u{00d7}{rh}"))
                        .weak()
                        .size(FONT_SM),
                );
            }
        }

        // Status messages
        if let Some(err) = &self.last_error {
            ui.add_space(GAP_SM);
            ui.label(
                egui::RichText::new(err)
                    .color(ERROR_TEXT)
                    .size(FONT_SM),
            );
        }
    }

    fn show_selecting_ui(&self, ui: &mut egui::Ui) {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::InnerSize(
            egui::vec2(WIN_W * 0.7, TITLE_H + BTN),
        ));
        ui.label(
            egui::RichText::new("Selecting region...")
                .size(FONT_SM)
                .weak(),
        );
    }

    fn show_recording_ui(&mut self, ui: &mut egui::Ui) {
        // Extract state for the viewport closure (can't capture &mut self)
        let paused = self.paused;
        let frame_count = self.frame_count;
        let elapsed = self.active_duration();
        let secs = elapsed.as_secs();
        let timer_text = format!(
            "{:02}:{:02}.{}",
            secs / 60,
            secs % 60,
            elapsed.subsec_millis() / 100
        );
        let frame_text = format!("{}", frame_count);

        let mut should_stop = false;
        let mut should_toggle_pause = false;

        // Compute viewport position near the recording region
        let win_w = 268.0_f32;
        let win_h = 60.0_f32;
        let viewport_pos = if let Some(CaptureMode::Region {
            x,
            y,
            w: rw,
            h,
        }) = &self.active_mode
        {
            let gap = 12.0_f32;
            let (screen_w, screen_h) = self
                .screen_size
                .map_or((1920.0, 1080.0), |(w, h)| (w as f32, h as f32));
            let region_bottom = *y as f32 + *h as f32;
            let region_right = *x as f32 + *rw as f32;
            let below_y = region_bottom + gap;

            if below_y + win_h <= screen_h {
                Some(egui::pos2(*x as f32, below_y))
            } else if *y as f32 - win_h - gap >= 0.0 {
                Some(egui::pos2(
                    *x as f32,
                    *y as f32 - win_h - gap,
                ))
            } else if region_right + gap + win_w <= screen_w {
                Some(egui::pos2(region_right + gap, *y as f32))
            } else if *x as f32 - win_w - gap >= 0.0 {
                Some(egui::pos2(*x as f32 - win_w - gap, *y as f32))
            } else {
                Some(egui::pos2(screen_w - win_w - 20.0, screen_h - win_h - 20.0))
            }
        } else {
            None
        };

        let viewport_id =
            egui::ViewportId::from_hash_of("recording_controls");

        let mut builder = egui::ViewportBuilder::default()
            .with_title("Recording")
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_resizable(false)
            .with_inner_size(egui::vec2(win_w, win_h));

        if let Some(pos) = viewport_pos {
            builder = builder.with_position(pos);
        }

        ui.ctx().show_viewport_immediate(
            viewport_id,
            builder,
            |ctx, _class| {
                let panel_frame = egui::Frame::NONE
                    .fill(BG)
                    .inner_margin(egui::Margin::symmetric(MARGIN_V as i8, 8));

                egui::CentralPanel::default()
                    .frame(panel_frame)
                    .show(ctx, |ui| {
                        // Full-area drag handle
                        let full_rect = ui.max_rect();
                        let drag_resp = ui.interact(
                            full_rect,
                            ui.id().with("controls_drag"),
                            egui::Sense::drag(),
                        );
                        if drag_resp.drag_started() {
                            ctx.send_viewport_cmd(
                                egui::ViewportCommand::StartDrag,
                            );
                        }

                        // Drag grip dots
                        let grip_x = full_rect.left() + 6.0;
                        let grip_cy = full_rect.center().y;
                        for i in -1..=1 {
                            ui.painter().circle_filled(
                                egui::pos2(grip_x, grip_cy + i as f32 * 5.0),
                                1.5,
                                TEXT_DISABLED,
                            );
                        }

                        let timer_w = 60.0;
                        let content_w = 14.0 + timer_w + 52.0 + BTN
                            + BTN
                            + GAP * 4.0;

                        ui.with_layout(
                            egui::Layout::left_to_right(
                                egui::Align::Center,
                            ),
                            |ui| {
                                ui.spacing_mut().item_spacing.x = GAP;
                                let pad = (ui.available_width()
                                    - content_w)
                                    .max(0.0)
                                    / 2.0;
                                ui.add_space(pad);

                                // Pulsing/static dot
                                let (dot_rect, _) =
                                    ui.allocate_exact_size(
                                        egui::vec2(14.0, BTN),
                                        egui::Sense::hover(),
                                    );
                                let dot_color = if paused {
                                    PAUSE_DOT
                                } else {
                                    let time =
                                        ui.input(|i| i.time);
                                    let alpha = ((time * 3.0).sin()
                                        * 0.4
                                        + 0.6)
                                        as f32;
                                    egui::Color32::from_rgba_unmultiplied(
                                        255,
                                        50,
                                        50,
                                        (alpha * 255.0) as u8,
                                    )
                                };
                                ui.painter().circle_filled(
                                    dot_rect.center(),
                                    5.0,
                                    dot_color,
                                );

                                // Timer
                                let (timer_rect, _) =
                                    ui.allocate_exact_size(
                                        egui::vec2(timer_w, BTN),
                                        egui::Sense::hover(),
                                    );
                                ui.painter().text(
                                    timer_rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    &timer_text,
                                    egui::FontId::monospace(FONT_LG),
                                    TEXT_PRIMARY,
                                );

                                // Frame count pill
                                let pill_w = 52.0;
                                let (pill_rect, _) =
                                    ui.allocate_exact_size(
                                        egui::vec2(pill_w, BTN),
                                        egui::Sense::hover(),
                                    );
                                ui.painter().rect_filled(
                                    pill_rect.shrink2(egui::vec2(
                                        0.0, 4.0,
                                    )),
                                    RADIUS_PILL,
                                    ACCENT_BG,
                                );
                                ui.painter().text(
                                    pill_rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    &frame_text,
                                    egui::FontId::monospace(FONT_MD),
                                    ACCENT,
                                );

                                // Pause/Resume button
                                {
                                    let (rect, resp) =
                                        ui.allocate_exact_size(
                                            egui::vec2(BTN, BTN),
                                            egui::Sense::click(),
                                        );
                                    if resp.hovered() {
                                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                    }
                                    if paused {
                                        let bg = if resp.hovered() {
                                            GREEN_HV
                                        } else {
                                            GREEN
                                        };
                                        ui.painter().rect_filled(
                                            rect, RADIUS, bg,
                                        );
                                        paint_play_icon(
                                            ui.painter(),
                                            rect,
                                            TEXT_PRIMARY,
                                        );
                                    } else {
                                        let bg = if resp.hovered() {
                                            CONTROL_HV
                                        } else {
                                            CONTROL
                                        };
                                        ui.painter().rect_filled(
                                            rect, RADIUS, bg,
                                        );
                                        paint_pause_icon(
                                            ui.painter(),
                                            rect,
                                            TEXT_PRIMARY,
                                        );
                                    }
                                    // Detect press directly — in child viewports on
                                    // Wayland the first focus-click can miss `clicked()`.
                                    let pressed_here = resp.rect.contains(
                                        ui.input(|i| i.pointer.interact_pos().unwrap_or_default())
                                    ) && ui.input(|i| i.pointer.any_pressed());
                                    if resp.clicked() || pressed_here {
                                        should_toggle_pause = true;
                                    }
                                    resp.on_hover_text(if paused {
                                        "Resume"
                                    } else {
                                        "Pause"
                                    });
                                }

                                // Stop button
                                {
                                    let (rect, resp) =
                                        ui.allocate_exact_size(
                                            egui::vec2(BTN, BTN),
                                            egui::Sense::click(),
                                        );
                                    if resp.hovered() {
                                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                    }
                                    let bg = if resp.hovered() {
                                        RED_HV
                                    } else {
                                        RED
                                    };
                                    ui.painter().rect_filled(
                                        rect, RADIUS, bg,
                                    );
                                    let sq =
                                        egui::Rect::from_center_size(
                                            rect.center(),
                                            egui::vec2(10.0, 10.0),
                                        );
                                    ui.painter().rect_filled(
                                        sq,
                                        2.0,
                                        TEXT_PRIMARY,
                                    );
                                    let pressed_here = resp.rect.contains(
                                        ui.input(|i| i.pointer.interact_pos().unwrap_or_default())
                                    ) && ui.input(|i| i.pointer.any_pressed());
                                    if resp.clicked() || pressed_here {
                                        should_stop = true;
                                    }
                                    resp.on_hover_text(
                                        "Stop Recording",
                                    );
                                }
                            },
                        );
                    });

                ctx.request_repaint();
            },
        );

        // Apply captured button presses
        if should_stop {
            let _ = self.cmd_tx.send(Command::StopRecording);
            self.encoding_progress = 0;
            self.state = RecordingState::Encoding;
        }
        if should_toggle_pause {
            if self.paused {
                if let Some(start) = self.pause_start {
                    self.paused_duration += start.elapsed();
                }
                self.paused = false;
                self.pause_start = None;
                let _ = self.cmd_tx.send(Command::ResumeRecording);
            } else {
                self.paused = true;
                self.pause_start = Some(Instant::now());
                let _ = self.cmd_tx.send(Command::PauseRecording);
            }
        }

        ui.ctx().request_repaint();
    }

    fn show_encoding_ui(&self, ui: &mut egui::Ui) {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::InnerSize(
            egui::vec2(WIN_W, 120.0),
        ));

        ui.spinner();
        ui.add_space(GAP_SM);
        ui.label(
            egui::RichText::new(format!(
                "Encoding... {} / {} frames",
                self.encoding_progress, self.frame_count
            ))
            .size(FONT_MD),
        );

        ui.add_space(GAP_SM);

        // Progress bar
        let (bar_rect, _) =
            ui.allocate_exact_size(egui::vec2(PROGRESS_W, PROGRESS_H), egui::Sense::hover());
        ui.painter().rect_filled(bar_rect, RADIUS_SM, SURFACE);
        if self.frame_count > 0 {
            let frac = (self.encoding_progress as f32 / self.frame_count as f32).clamp(0.0, 1.0);
            let fill_rect = egui::Rect::from_min_size(
                bar_rect.min,
                egui::vec2(PROGRESS_W * frac, PROGRESS_H),
            );
            ui.painter().rect_filled(fill_rect, RADIUS_SM, GREEN);
        }

        ui.ctx().request_repaint();
    }

    fn show_reviewing_ui(&mut self, ui: &mut egui::Ui) {
        self.show_review_editing_ui(ui);
    }

    fn show_review_editing_ui(&mut self, ui: &mut egui::Ui) {
        if self.review_frames.is_empty() {
            ui.label("No frames recorded.");
            if ui.button("Back").clicked() {
                self.go_to_idle();
            }
            return;
        }

        let total_frames = self.review_frames.len();

        // Clamp playhead
        self.review_playhead = self
            .review_playhead
            .clamp(self.review_trim_start, self.review_trim_end.saturating_sub(1));

        // --- Preview frame ---
        let frame = &self.review_frames[self.review_playhead];
        let fw = frame.width as usize;
        let fh = frame.height as usize;

        let pixels: Vec<egui::Color32> = frame
            .data
            .chunks_exact(4)
            .map(|c| egui::Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]))
            .collect();
        let image = egui::ColorImage::new([fw, fh], pixels);

        let tex = self.review_texture.get_or_insert_with(|| {
            ui.ctx()
                .load_texture("review_preview", image.clone(), egui::TextureOptions::LINEAR)
        });
        tex.set(image, egui::TextureOptions::LINEAR);

        // Scale preview to fit, then size the window to match
        let controls_h = REVIEW_CONTROLS_H;
        let content_w = WIN_W_REVIEW - MARGIN_H * 2.0; // window width minus horizontal panel margins
        let avail_w = content_w - MARGIN_H / 2.0;
        let aspect = fw as f32 / fh as f32;
        let preview_h = (avail_w / aspect).min(380.0);
        let preview_w = preview_h * aspect;

        // Set window height to fit content snugly: title bar + margins + preview + controls + status
        let status_h = if self.encoding_in_progress {
            GAP + FONT_SM + GAP_SM + PROGRESS_H + GAP_LG
        } else if self.review_saved_path.is_some() {
            GAP_LG + BTN_ACTION_H + GAP_LG
        } else {
            0.0
        };
        let target_h = TITLE_H + MARGIN_V * 2.0 + preview_h + controls_h + status_h;
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::InnerSize(
            egui::vec2(WIN_W_REVIEW, target_h),
        ));

        let (preview_rect, _) =
            ui.allocate_exact_size(egui::vec2(preview_w, preview_h), egui::Sense::hover());
        ui.painter().image(
            tex.id(),
            preview_rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );

        // --- Separator between preview and controls ---
        ui.add_space(GAP);
        let clip = ui.clip_rect();
        let y = ui.cursor().top();
        ui.painter().line_segment(
            [
                egui::pos2(clip.left(), y),
                egui::pos2(clip.right(), y),
            ],
            egui::Stroke::new(1.0, BORDER),
        );
        ui.add_space(GAP_LG);

        // --- Playback row: play/pause + time ---
        let trimmed_frames = self.review_trim_end.saturating_sub(self.review_trim_start);
        let playhead_in_trim = self.review_playhead.saturating_sub(self.review_trim_start);
        let frame_dur = 1.0 / self.review_fps as f64;
        let current_time = playhead_in_trim as f64 * frame_dur;
        let total_time = trimmed_frames as f64 * frame_dur;

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            // Play/Pause button
            let (btn_rect, btn_resp) =
                ui.allocate_exact_size(egui::vec2(BTN_SM, BTN_SM), egui::Sense::click());
            if btn_resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            let bg = if btn_resp.hovered() {
                CONTROL_HV
            } else {
                CONTROL
            };
            ui.painter().rect_filled(btn_rect, RADIUS, bg);
            if self.review_playing {
                paint_pause_icon(ui.painter(), btn_rect, TEXT_PRIMARY);
            } else {
                paint_play_icon(ui.painter(), btn_rect, TEXT_PRIMARY);
            }
            if btn_resp.clicked() {
                self.review_playing = !self.review_playing;
                if self.review_playing {
                    self.review_last_tick = Some(Instant::now());
                } else {
                    self.review_last_tick = None;
                }
            }

            // Time label
            ui.label(
                egui::RichText::new(format!(
                    "{}  /  {}",
                    format_time(current_time),
                    format_time(total_time),
                ))
                .size(FONT_MD)
                .color(TEXT_SECONDARY),
            );
        });

        ui.add_space(GAP_SM);

        // --- Timeline with trim handles ---
        {
            let track_h = 36.0;
            let ruler_h = 20.0;
            let timeline_h = track_h + ruler_h;
            let handle_w = 8.0;
            let handle_color = ORANGE;

            let (timeline_rect, timeline_resp) = ui.allocate_exact_size(
                egui::vec2(ui.available_width(), timeline_h),
                egui::Sense::click_and_drag(),
            );

            let painter = ui.painter();

            // Track area (top portion)
            let track_rect = egui::Rect::from_min_size(
                timeline_rect.min,
                egui::vec2(timeline_rect.width(), track_h),
            );

            // Track background
            painter.rect_filled(track_rect, RADIUS_SM, SURFACE);

            let track_w = track_rect.width();

            // Trim fractions
            let trim_start_frac = self.review_trim_start as f32 / total_frames as f32;
            let trim_end_frac = self.review_trim_end as f32 / total_frames as f32;

            // Trim region highlight
            let trim_left = track_rect.left() + track_w * trim_start_frac;
            let trim_right = track_rect.left() + track_w * trim_end_frac;
            let trim_rect = egui::Rect::from_min_max(
                egui::pos2(trim_left, track_rect.top()),
                egui::pos2(trim_right, track_rect.bottom()),
            );
            painter.rect_filled(trim_rect, 0.0, CONTROL_HV);

            // Dim overlay outside trim region (left side)
            if trim_left > track_rect.left() {
                let dim_left = egui::Rect::from_min_max(
                    track_rect.min,
                    egui::pos2(trim_left, track_rect.bottom()),
                );
                painter.rect_filled(dim_left, RADIUS_SM, egui::Color32::from_black_alpha(120));
            }
            // Dim overlay outside trim region (right side)
            if trim_right < track_rect.right() {
                let dim_right = egui::Rect::from_min_max(
                    egui::pos2(trim_right, track_rect.top()),
                    egui::pos2(track_rect.right(), track_rect.bottom()),
                );
                painter.rect_filled(dim_right, RADIUS_SM, egui::Color32::from_black_alpha(120));
            }

            // Bracket handle: start `[`
            let cap_len = 6.0;
            let handle_stroke = egui::Stroke::new(2.5, handle_color);
            // Vertical bar
            painter.line_segment(
                [
                    egui::pos2(trim_left, track_rect.top()),
                    egui::pos2(trim_left, track_rect.bottom()),
                ],
                handle_stroke,
            );
            // Top cap (inward)
            painter.line_segment(
                [
                    egui::pos2(trim_left, track_rect.top()),
                    egui::pos2(trim_left + cap_len, track_rect.top()),
                ],
                handle_stroke,
            );
            // Bottom cap (inward)
            painter.line_segment(
                [
                    egui::pos2(trim_left, track_rect.bottom()),
                    egui::pos2(trim_left + cap_len, track_rect.bottom()),
                ],
                handle_stroke,
            );

            // Bracket handle: end `]`
            painter.line_segment(
                [
                    egui::pos2(trim_right, track_rect.top()),
                    egui::pos2(trim_right, track_rect.bottom()),
                ],
                handle_stroke,
            );
            // Top cap (inward)
            painter.line_segment(
                [
                    egui::pos2(trim_right, track_rect.top()),
                    egui::pos2(trim_right - cap_len, track_rect.top()),
                ],
                handle_stroke,
            );
            // Bottom cap (inward)
            painter.line_segment(
                [
                    egui::pos2(trim_right, track_rect.bottom()),
                    egui::pos2(trim_right - cap_len, track_rect.bottom()),
                ],
                handle_stroke,
            );

            // Playhead
            let playhead_frac = self.review_playhead as f32 / total_frames as f32;
            let playhead_x = track_rect.left() + track_w * playhead_frac;
            // Triangle marker at top
            let tri_half = 5.0;
            let tri_h = 6.0;
            painter.add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(playhead_x - tri_half, track_rect.top()),
                    egui::pos2(playhead_x + tri_half, track_rect.top()),
                    egui::pos2(playhead_x, track_rect.top() + tri_h),
                ],
                egui::Color32::WHITE,
                egui::Stroke::NONE,
            ));
            // Vertical line
            painter.line_segment(
                [
                    egui::pos2(playhead_x, track_rect.top() + tri_h),
                    egui::pos2(playhead_x, track_rect.bottom()),
                ],
                egui::Stroke::new(1.5, egui::Color32::WHITE),
            );

            // --- Time ruler ---
            let ruler_rect = egui::Rect::from_min_size(
                egui::pos2(timeline_rect.left(), track_rect.bottom()),
                egui::vec2(timeline_rect.width(), ruler_h),
            );
            // Separator line
            painter.line_segment(
                [ruler_rect.left_top(), ruler_rect.right_top()],
                egui::Stroke::new(1.0, BORDER),
            );

            let major_tick_h = 6.0;
            let minor_tick_h = 3.0;
            let tick_color = TEXT_MUTED;

            // Ruler uses full recording time so ticks stay fixed when trimming
            let full_time = total_frames as f64 * frame_dur;
            let approx_interval = full_time * 50.0 / track_w as f64;
            let interval = pick_tick_interval(approx_interval);
            let minor_interval = interval / 4.0;

            // Minor ticks
            {
                let mut t = 0.0;
                while t <= full_time + 1e-6 {
                    let x = track_rect.left()
                        + (t / full_time) as f32 * track_w;
                    if x >= track_rect.left() && x <= track_rect.right() {
                        painter.line_segment(
                            [
                                egui::pos2(x, ruler_rect.top()),
                                egui::pos2(x, ruler_rect.top() + minor_tick_h),
                            ],
                            egui::Stroke::new(1.0, tick_color),
                        );
                    }
                    t += minor_interval;
                }
            }

            // Major ticks with labels
            {
                let mut t = 0.0;
                while t <= full_time + 1e-6 {
                    let x = track_rect.left()
                        + (t / full_time) as f32 * track_w;
                    if x >= track_rect.left() && x <= track_rect.right() {
                        painter.line_segment(
                            [
                                egui::pos2(x, ruler_rect.top()),
                                egui::pos2(x, ruler_rect.top() + major_tick_h),
                            ],
                            egui::Stroke::new(1.0, TEXT_MUTED),
                        );
                        let label = format_time(t);
                        painter.text(
                            egui::pos2(x, ruler_rect.top() + major_tick_h + 1.0),
                            egui::Align2::CENTER_TOP,
                            label,
                            egui::FontId::proportional(FONT_XS),
                            TEXT_MUTED,
                        );
                    }
                    t += interval;
                }
            }

            // --- Interaction: drag trim handles, playhead, or scrub ---
            let playhead_grab_r = tri_half + 4.0; // generous grab radius around triangle
            if timeline_resp.drag_started() {
                if let Some(pos) = timeline_resp.interact_pointer_pos() {
                    let dist_playhead = (pos.x - playhead_x).abs();
                    let in_triangle_zone = pos.y < track_rect.top() + tri_h + 6.0;
                    let dist_start = (pos.x - trim_left).abs();
                    let dist_end = (pos.x - trim_right).abs();

                    // Playhead triangle gets priority when click is in the
                    // upper part of the track and close to the playhead x.
                    if dist_playhead < playhead_grab_r && in_triangle_zone {
                        self.review_dragging = Some(TrimDrag::Playhead);
                    } else if dist_start < handle_w * 2.5 && dist_start <= dist_end {
                        self.review_dragging = Some(TrimDrag::Start);
                    } else if dist_end < handle_w * 2.5 {
                        self.review_dragging = Some(TrimDrag::End);
                    } else {
                        self.review_dragging = None;
                    }
                }
            }

            if timeline_resp.dragged() {
                if let Some(pos) = timeline_resp.interact_pointer_pos() {
                    let frac = ((pos.x - track_rect.left()) / track_w).clamp(0.0, 1.0);
                    let frame_idx = (frac * total_frames as f32) as usize;
                    let frame_idx = frame_idx.min(total_frames);

                    match self.review_dragging {
                        Some(TrimDrag::Start) => {
                            let new_start = frame_idx.min(self.review_trim_end.saturating_sub(1));
                            if new_start != self.review_trim_start {
                                self.review_trim_start = new_start;
                                self.review_saved_path = None;
                            }
                            if self.review_playhead < self.review_trim_start {
                                self.review_playhead = self.review_trim_start;
                            }
                        }
                        Some(TrimDrag::End) => {
                            let new_end = frame_idx.max(self.review_trim_start + 1).min(total_frames);
                            if new_end != self.review_trim_end {
                                self.review_trim_end = new_end;
                                self.review_saved_path = None;
                            }
                            if self.review_playhead >= self.review_trim_end {
                                self.review_playhead = self.review_trim_end.saturating_sub(1);
                            }
                        }
                        Some(TrimDrag::Playhead) | None => {
                            // Scrub playhead
                            self.review_playhead = frame_idx
                                .clamp(self.review_trim_start, self.review_trim_end.saturating_sub(1));
                            self.review_playing = false;
                            self.review_last_tick = None;
                        }
                    }
                }
            }

            if timeline_resp.drag_stopped() {
                self.review_dragging = None;
            }

            // Click to scrub (not drag)
            if timeline_resp.clicked() {
                if let Some(pos) = timeline_resp.interact_pointer_pos() {
                    let frac = ((pos.x - track_rect.left()) / track_w).clamp(0.0, 1.0);
                    let frame_idx = (frac * total_frames as f32) as usize;
                    self.review_playhead = frame_idx
                        .clamp(self.review_trim_start, self.review_trim_end.saturating_sub(1));
                    self.review_playing = false;
                    self.review_last_tick = None;
                }
            }
        }

        ui.add_space(GAP_LG);

        // --- Action row: Close + Save ---
        ui.horizontal(|ui| {
            let avail = ui.available_width();
            let total = BTN_ACTION_W * 2.0 + GAP_LG;
            ui.add_space((avail - total).max(0.0) / 2.0);

            // Close (smart)
            let (close_rect, close_resp) =
                ui.allocate_exact_size(egui::vec2(BTN_ACTION_W, BTN_ACTION_H), egui::Sense::click());
            if close_resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            let bg = if close_resp.hovered() {
                CONTROL_HV
            } else {
                CONTROL
            };
            ui.painter().rect_filled(close_rect, RADIUS, bg);
            ui.painter().text(
                close_rect.center(),
                egui::Align2::CENTER_CENTER,
                "Close",
                egui::FontId::proportional(FONT_MD),
                TEXT_SECONDARY,
            );
            if close_resp.clicked() {
                if self.review_frames.is_empty()
                    || self.encoding_in_progress
                    || self.review_saved_path.is_some()
                {
                    self.go_to_idle();
                } else {
                    let result = rfd::MessageDialog::new()
                        .set_title("Unsaved Recording")
                        .set_description("Would you like to save this recording?")
                        .set_buttons(rfd::MessageButtons::YesNoCancel)
                        .set_level(rfd::MessageLevel::Warning)
                        .show();
                    match result {
                        rfd::MessageDialogResult::Yes => {
                            self.save_recording();
                        }
                        rfd::MessageDialogResult::No => {
                            self.go_to_idle();
                        }
                        _ => {} // Cancel — do nothing
                    }
                }
            }

            ui.add_space(GAP_LG);

            // Save
            let save_enabled = !self.encoding_in_progress;
            let (save_rect, save_resp) = ui.allocate_exact_size(
                egui::vec2(BTN_ACTION_W, BTN_ACTION_H),
                if save_enabled { egui::Sense::click() } else { egui::Sense::hover() },
            );
            if save_enabled {
                if save_resp.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                let bg = if save_resp.hovered() { GREEN_HV } else { GREEN };
                ui.painter().rect_filled(save_rect, RADIUS, bg);
                ui.painter().text(
                    save_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "Save",
                    egui::FontId::proportional(FONT_MD),
                    TEXT_PRIMARY,
                );
                if save_resp.clicked() {
                    self.save_recording();
                }
            } else {
                ui.painter().rect_filled(save_rect, RADIUS, DISABLED);
                ui.painter().text(
                    save_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "Saving...",
                    egui::FontId::proportional(FONT_MD),
                    TEXT_DISABLED,
                );
            }
        });

        // --- Inline status section ---
        if self.encoding_in_progress {
            ui.add_space(GAP);
            ui.label(
                egui::RichText::new(format!(
                    "Encoding {} / {}",
                    self.encoding_progress, self.encoding_total_frames
                ))
                .size(FONT_SM)
                .color(TEXT_SECONDARY),
            );
            ui.add_space(GAP_SM);
            let (bar_rect, _) =
                ui.allocate_exact_size(egui::vec2(PROGRESS_W, PROGRESS_H), egui::Sense::hover());
            ui.painter().rect_filled(bar_rect, RADIUS_SM, SURFACE);
            if self.encoding_total_frames > 0 {
                let frac = (self.encoding_progress as f32
                    / self.encoding_total_frames as f32)
                    .clamp(0.0, 1.0);
                let fill_rect = egui::Rect::from_min_size(
                    bar_rect.min,
                    egui::vec2(PROGRESS_W * frac, PROGRESS_H),
                );
                ui.painter().rect_filled(fill_rect, RADIUS_SM, GREEN);
            }
            ui.ctx().request_repaint();
        } else if let Some(path) = &self.review_saved_path.clone() {
            ui.add_space(GAP_LG);

            // Centered row: [dot] "Saved" [path] [Show in Folder]
            let folder_btn_w = 100.0;
            let dot_r = 3.0;
            let dot_w = dot_r * 2.0 + 2.0; // dot diameter + padding
            let inner_gap = 6.0;

            // Measure text widths for centering
            let saved_font = egui::FontId::proportional(FONT_SM);
            let path_str = path.to_string_lossy();
            let display = truncate_path(&path_str, 30);
            let path_font = egui::FontId::proportional(FONT_XS);

            let saved_galley = ui.painter().layout_no_wrap(
                "Saved".to_string(), saved_font.clone(), GREEN,
            );
            let path_galley = ui.painter().layout_no_wrap(
                display.clone(), path_font.clone(), TEXT_MUTED,
            );
            let saved_w = saved_galley.size().x;
            let path_w = path_galley.size().x;

            let total_w = dot_w + inner_gap + saved_w + inner_gap
                + path_w + inner_gap * 2.0 + folder_btn_w;

            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let avail = ui.available_width();
                ui.add_space((avail - total_w).max(0.0) / 2.0);

                // Green dot
                let (dot_rect, _) = ui.allocate_exact_size(
                    egui::vec2(dot_w, BTN_ACTION_H),
                    egui::Sense::hover(),
                );
                ui.painter().circle_filled(
                    egui::pos2(dot_rect.center().x, dot_rect.center().y),
                    dot_r,
                    GREEN,
                );

                ui.add_space(inner_gap);

                // "Saved" text
                let (saved_rect, _) = ui.allocate_exact_size(
                    egui::vec2(saved_w, BTN_ACTION_H),
                    egui::Sense::hover(),
                );
                ui.painter().text(
                    egui::pos2(saved_rect.left(), saved_rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    "Saved",
                    saved_font,
                    GREEN,
                );

                ui.add_space(inner_gap);

                // Truncated path
                let (path_rect, _) = ui.allocate_exact_size(
                    egui::vec2(path_w, BTN_ACTION_H),
                    egui::Sense::hover(),
                );
                ui.painter().text(
                    egui::pos2(path_rect.left(), path_rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    &display,
                    path_font,
                    TEXT_MUTED,
                );

                ui.add_space(inner_gap * 2.0);

                // "Show in Folder" button
                let (folder_rect, folder_resp) = ui.allocate_exact_size(
                    egui::vec2(folder_btn_w, BTN_ACTION_H),
                    egui::Sense::click(),
                );
                if folder_resp.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                let bg = if folder_resp.hovered() { CONTROL_HV } else { CONTROL };
                ui.painter().rect_filled(folder_rect, RADIUS, bg);
                ui.painter().text(
                    folder_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "Show in Folder",
                    egui::FontId::proportional(FONT_SM),
                    TEXT_SECONDARY,
                );
                if folder_resp.clicked() {
                    if let Some(parent) = path.parent() {
                        let _ = open::that(parent);
                    }
                }
            });
        }

        ui.add_space(GAP_LG);
    }

    fn show_settings_viewport(&mut self, ctx: &egui::Context) {
        let mut s_fps = self.settings_fps;
        let s_dir = self.settings_output_dir.clone();
        let s_error = self.settings_error.clone();
        let mut should_apply = false;
        let mut should_cancel = false;
        let mut browse_result: Option<String> = None;

        let viewport_id = egui::ViewportId::from_hash_of("settings_window");
        let builder = egui::ViewportBuilder::default()
            .with_title("Settings")
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_resizable(false)
            .with_inner_size(egui::vec2(WIN_W, 220.0));

        ctx.show_viewport_immediate(
            viewport_id,
            builder,
            |ctx, _class| {
                let frame = egui::Frame::NONE
                    .fill(BG)
                    .inner_margin(egui::Margin::symmetric(0, 0));

                egui::CentralPanel::default()
                    .frame(frame)
                    .show(ctx, |ui| {
                        // --- Title bar ---
                        let title_rect = egui::Rect::from_min_size(
                            ui.cursor().min,
                            egui::vec2(ui.available_width(), TITLE_H),
                        );
                        let (_, _) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), TITLE_H),
                            egui::Sense::hover(),
                        );

                        let drag_resp = ui.interact(
                            title_rect,
                            ui.id().with("settings_drag"),
                            egui::Sense::drag(),
                        );
                        if drag_resp.drag_started() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                        }

                        ui.painter().text(
                            egui::pos2(title_rect.left() + MARGIN_H / 2.0 + 6.0, title_rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            "Settings",
                            egui::FontId::proportional(FONT_MD),
                            TEXT_MUTED,
                        );

                        // Close button
                        let close_rect = egui::Rect::from_center_size(
                            egui::pos2(
                                title_rect.right() - MARGIN_H / 2.0 - BTN / 2.0,
                                title_rect.center().y,
                            ),
                            egui::vec2(BTN, BTN),
                        );
                        let close = ui.interact(
                            close_rect,
                            ui.id().with("settings_close"),
                            egui::Sense::click(),
                        );
                        let color = if close.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                            egui::Color32::from_rgb(255, 80, 80)
                        } else {
                            TEXT_MUTED
                        };
                        ui.painter().text(
                            close_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "\u{00d7}",
                            egui::FontId::proportional(16.0),
                            color,
                        );
                        // Detect press directly — in child viewports on
                        // Wayland the first focus-click can miss `clicked()`.
                        let pressed_here = close.rect.contains(
                            ui.input(|i| i.pointer.interact_pos().unwrap_or_default())
                        ) && ui.input(|i| i.pointer.any_pressed());
                        if close.clicked() || pressed_here {
                            should_cancel = true;
                        }

                        // Title bar separator
                        ui.painter().line_segment(
                            [
                                egui::pos2(title_rect.left(), title_rect.bottom()),
                                egui::pos2(title_rect.right(), title_rect.bottom()),
                            ],
                            egui::Stroke::new(1.0, BORDER),
                        );

                        // --- Content area with margins ---
                        let content_frame = egui::Frame::NONE
                            .inner_margin(egui::Margin::symmetric(MARGIN_H as i8, MARGIN_V as i8));
                        content_frame.show(ui, |ui| {
                            let content_w = ui.available_width();

                            // "Frames Per Second" label
                            ui.label(
                                egui::RichText::new("Frames Per Second")
                                    .size(FONT_SM)
                                    .color(TEXT_SECONDARY),
                            );

                            ui.add_space(GAP_SM);

                            // FPS stepper: [-] value [+], full width
                            {
                                let step_w = 28.0;
                                let w = content_w;
                                let (rect, _) = ui.allocate_exact_size(
                                    egui::vec2(w, BTN_SM),
                                    egui::Sense::hover(),
                                );
                                ui.painter().rect_filled(rect, RADIUS, CONTROL);

                                let minus_r = egui::Rect::from_min_size(
                                    rect.min,
                                    egui::vec2(step_w, BTN_SM),
                                );
                                let minus = ui.interact(
                                    minus_r,
                                    ui.id().with("sfps-"),
                                    egui::Sense::click(),
                                );
                                if minus.hovered() {
                                    ui.painter().rect_filled(minus_r, RADIUS, CONTROL_HV);
                                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                }
                                let c = if minus.hovered() { TEXT_PRIMARY } else { TEXT_SECONDARY };
                                ui.painter().text(
                                    minus_r.center(),
                                    egui::Align2::CENTER_CENTER,
                                    "\u{2212}",
                                    egui::FontId::monospace(FONT_LG),
                                    c,
                                );
                                if minus.clicked() && s_fps > 1 {
                                    s_fps -= 1;
                                }

                                ui.painter().text(
                                    rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    &format!("{}", s_fps),
                                    egui::FontId::monospace(FONT_XS),
                                    TEXT_PRIMARY,
                                );

                                let plus_r = egui::Rect::from_min_size(
                                    egui::pos2(rect.right() - step_w, rect.top()),
                                    egui::vec2(step_w, BTN_SM),
                                );
                                let plus = ui.interact(
                                    plus_r,
                                    ui.id().with("sfps+"),
                                    egui::Sense::click(),
                                );
                                if plus.hovered() {
                                    ui.painter().rect_filled(plus_r, RADIUS, CONTROL_HV);
                                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                }
                                let c = if plus.hovered() { TEXT_PRIMARY } else { TEXT_SECONDARY };
                                ui.painter().text(
                                    plus_r.center(),
                                    egui::Align2::CENTER_CENTER,
                                    "+",
                                    egui::FontId::monospace(FONT_LG),
                                    c,
                                );
                                if plus.clicked() && s_fps < 30 {
                                    s_fps += 1;
                                }
                            }

                            ui.add_space(GAP_LG);

                            // "Output Directory" label
                            ui.label(
                                egui::RichText::new("Output Directory")
                                    .size(FONT_SM)
                                    .color(TEXT_SECONDARY),
                            );

                            ui.add_space(GAP_SM);

                            // Directory row: path display + Browse button
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = GAP_SM;

                                let browse_w = 56.0;
                                let dir_w = content_w - browse_w - GAP_SM;

                                // Truncated path display
                                let max_chars = (dir_w / 6.0) as usize;
                                let dir_display = truncate_path(&s_dir, max_chars);
                                let (dir_rect, _) = ui.allocate_exact_size(
                                    egui::vec2(dir_w, BTN_SM),
                                    egui::Sense::hover(),
                                );
                                ui.painter().rect_filled(dir_rect, RADIUS_SM, SURFACE);
                                ui.painter().text(
                                    egui::pos2(dir_rect.left() + 6.0, dir_rect.center().y),
                                    egui::Align2::LEFT_CENTER,
                                    &dir_display,
                                    egui::FontId::proportional(FONT_XS),
                                    TEXT_SECONDARY,
                                );

                                // Browse button
                                let (browse_rect, browse_resp) = ui.allocate_exact_size(
                                    egui::vec2(browse_w, BTN_SM),
                                    egui::Sense::click(),
                                );
                                let bg = if browse_resp.hovered() {
                                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                    CONTROL_HV
                                } else {
                                    CONTROL
                                };
                                ui.painter().rect_filled(browse_rect, RADIUS_SM, bg);
                                ui.painter().text(
                                    browse_rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    "Browse",
                                    egui::FontId::proportional(FONT_XS),
                                    TEXT_SECONDARY,
                                );
                                if browse_resp.clicked() {
                                    let dir = s_dir.clone();
                                    let folder = tokio::runtime::Runtime::new().ok().and_then(|rt| {
                                        rt.block_on(
                                            rfd::AsyncFileDialog::new()
                                                .set_directory(&dir)
                                                .pick_folder(),
                                        )
                                    });
                                    if let Some(handle) = folder {
                                        browse_result = Some(handle.path().to_string_lossy().into_owned());
                                    }
                                }
                            });

                            ui.add_space(GAP_LG);

                            // Action row: Cancel + Apply (centered)
                            ui.horizontal(|ui| {
                                let avail = ui.available_width();
                                let total = BTN_ACTION_W * 2.0 + GAP_LG;
                                ui.add_space((avail - total).max(0.0) / 2.0);

                                // Cancel
                                let (cancel_rect, cancel_resp) = ui.allocate_exact_size(
                                    egui::vec2(BTN_ACTION_W, BTN_ACTION_H),
                                    egui::Sense::click(),
                                );
                                if cancel_resp.hovered() {
                                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                }
                                let bg = if cancel_resp.hovered() { CONTROL_HV } else { CONTROL };
                                ui.painter().rect_filled(cancel_rect, RADIUS, bg);
                                ui.painter().text(
                                    cancel_rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    "Cancel",
                                    egui::FontId::proportional(FONT_MD),
                                    TEXT_SECONDARY,
                                );
                                // Detect press directly — in child viewports on
                                // Wayland the first focus-click can miss `clicked()`.
                                let pressed_here = cancel_resp.rect.contains(
                                    ui.input(|i| i.pointer.interact_pos().unwrap_or_default())
                                ) && ui.input(|i| i.pointer.any_pressed());
                                if cancel_resp.clicked() || pressed_here {
                                    should_cancel = true;
                                }

                                ui.add_space(GAP_LG);

                                // Apply
                                let (apply_rect, apply_resp) = ui.allocate_exact_size(
                                    egui::vec2(BTN_ACTION_W, BTN_ACTION_H),
                                    egui::Sense::click(),
                                );
                                if apply_resp.hovered() {
                                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                }
                                let bg = if apply_resp.hovered() { GREEN_HV } else { GREEN };
                                ui.painter().rect_filled(apply_rect, RADIUS, bg);
                                ui.painter().text(
                                    apply_rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    "Apply",
                                    egui::FontId::proportional(FONT_MD),
                                    TEXT_PRIMARY,
                                );
                                if apply_resp.clicked() {
                                    should_apply = true;
                                }
                            });

                            // Error display
                            if let Some(err) = &s_error {
                                ui.add_space(GAP_SM);
                                ui.label(
                                    egui::RichText::new(err)
                                        .color(ERROR_TEXT)
                                        .size(FONT_XS),
                                );
                            }
                        });
                    });
            },
        );

        // Write back captured state
        self.settings_fps = s_fps;
        if let Some(d) = browse_result {
            self.settings_output_dir = d;
        }
        if should_cancel {
            self.show_settings = false;
            self.settings_error = None;
        }
        if should_apply {
            self.config.default_fps = self.settings_fps.clamp(1, 30);
            self.config.output_dir = PathBuf::from(&self.settings_output_dir);
            match self.config.save() {
                Ok(()) => {
                    self.fps = self.config.default_fps;
                    self.settings_error = None;
                    self.show_settings = false;
                }
                Err(e) => {
                    self.settings_error = Some(e);
                }
            }
        }
    }

    fn go_to_idle(&mut self) {
        self.review_frames = Vec::new();
        self.review_texture = None;
        self.review_playing = false;
        self.review_last_tick = None;
        self.review_dragging = None;
        self.encoding_in_progress = false;
        self.encoding_total_frames = 0;
        self.review_saved_path = None;
        self.state = RecordingState::Idle;
        self.recording_start = None;
        self.active_mode = None;
        self.paused = false;
        self.pause_start = None;
        self.show_settings = false;
    }

    fn save_recording(&mut self) {
        if self.review_frames.is_empty() || self.encoding_in_progress {
            return;
        }
        let source_w = self.review_frames[0].width;
        let source_h = self.review_frames[0].height;
        let trimmed: Vec<Frame> = self.review_frames
            [self.review_trim_start..self.review_trim_end]
            .to_vec();
        let trimmed_len = trimmed.len();

        let _ = self.cmd_tx.send(Command::EncodeFrames {
            frames: trimmed,
            fps: self.review_fps,
            start: 0,
            end: trimmed_len,
            width: Some(source_w),
            height: Some(source_h),
            output_path: generate_output_path(&self.config.output_dir),
        });

        self.encoding_in_progress = true;
        self.encoding_total_frames = trimmed_len;
        self.encoding_progress = 0;
        self.review_saved_path = None;
    }

    fn show_region_highlight(&self, ctx: &egui::Context) {
        // Show in idle and recording, when region mode is active, a region is
        // selected, and the overlay is not open.
        if !matches!(self.state, RecordingState::Idle | RecordingState::Recording)
            || !self.use_region
            || self.region_selector.is_open()
        {
            return;
        }
        let Some((rx, ry, rw, rh)) = self.selected_region else {
            return;
        };

        let viewport_id =
            egui::ViewportId::from_hash_of("region_highlight");

        ctx.show_viewport_immediate(
            viewport_id,
            egui::ViewportBuilder::default()
                .with_title("Region")
                .with_decorations(false)
                .with_transparent(true)
                .with_always_on_top()
                .with_mouse_passthrough(true)
                .with_resizable(false)
                .with_position(egui::pos2(rx as f32, ry as f32))
                .with_inner_size(egui::vec2(rw as f32, rh as f32)),
            |ctx, _class| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ctx, |ui| {
                        let rect = ui.max_rect();
                        crate::region::draw_dashed_rect(
                            ui.painter(),
                            rect,
                            egui::Color32::WHITE,
                            egui::Color32::from_black_alpha(100),
                            2.0,
                            6.0,
                            4.0,
                        );
                    });
            },
        );
    }

    fn start_recording(&mut self, mode: CaptureMode) {
        self.last_error = None;
        self.frame_count = 0;
        self.paused = false;
        self.pause_start = None;
        self.paused_duration = Duration::ZERO;
        self.active_mode = Some(mode.clone());
        self.show_settings = false;

        let _ = self.cmd_tx.send(Command::StartRecording {
            mode,
            fps: self.fps,
        });
    }
}

fn generate_output_path(output_dir: &Path) -> PathBuf {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let time = secs as libc::time_t;
    unsafe { libc::localtime_r(&time, &mut tm) };

    let y = tm.tm_year + 1900;
    let mo = tm.tm_mon + 1;
    let d = tm.tm_mday;
    let hh = tm.tm_hour;
    let mm = tm.tm_min;
    let ss = tm.tm_sec;

    output_dir.join(format!(
        "heartkelp_{y:04}-{mo:02}-{d:02}_{hh:02}-{mm:02}-{ss:02}.gif"
    ))
}

fn truncate_path(path: &str, max_len: usize) -> String {
    if path.len() <= max_len {
        path.to_string()
    } else {
        format!("...{}", &path[path.len() - max_len + 3..])
    }
}

fn format_time(secs: f64) -> String {
    let mins = secs as u64 / 60;
    let s = secs as u64 % 60;
    let tenths = ((secs.fract()) * 10.0) as u64;
    format!("{mins}:{s:02}.{tenths}")
}

fn pick_tick_interval(approx: f64) -> f64 {
    const NICE: &[f64] = &[0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0];
    for &v in NICE {
        if v >= approx {
            return v;
        }
    }
    *NICE.last().unwrap()
}

/// Crop-corner brackets icon.
fn paint_crop_icon(
    painter: &egui::Painter,
    rect: egui::Rect,
    color: egui::Color32,
) {
    let stroke = egui::Stroke::new(1.5, color);
    let s = rect.width().min(rect.height());
    let m = s * 0.24;
    let c = s * 0.18;

    let l = rect.left() + m;
    let r = rect.right() - m;
    let t = rect.top() + m;
    let b = rect.bottom() - m;

    painter.line_segment([egui::pos2(l, t), egui::pos2(l + c, t)], stroke);
    painter.line_segment([egui::pos2(l, t), egui::pos2(l, t + c)], stroke);
    painter.line_segment([egui::pos2(r, t), egui::pos2(r - c, t)], stroke);
    painter.line_segment([egui::pos2(r, t), egui::pos2(r, t + c)], stroke);
    painter.line_segment([egui::pos2(l, b), egui::pos2(l + c, b)], stroke);
    painter.line_segment([egui::pos2(l, b), egui::pos2(l, b - c)], stroke);
    painter.line_segment([egui::pos2(r, b), egui::pos2(r - c, b)], stroke);
    painter.line_segment([egui::pos2(r, b), egui::pos2(r, b - c)], stroke);
}

/// Two vertical bars (pause icon).
fn paint_pause_icon(
    painter: &egui::Painter,
    rect: egui::Rect,
    color: egui::Color32,
) {
    let bar_w = 3.0;
    let bar_h = 12.0;
    let gap = 3.0;
    let cx = rect.center().x;
    let cy = rect.center().y;

    let left = egui::Rect::from_center_size(
        egui::pos2(cx - gap / 2.0 - bar_w / 2.0, cy),
        egui::vec2(bar_w, bar_h),
    );
    let right = egui::Rect::from_center_size(
        egui::pos2(cx + gap / 2.0 + bar_w / 2.0, cy),
        egui::vec2(bar_w, bar_h),
    );
    painter.rect_filled(left, 1.0, color);
    painter.rect_filled(right, 1.0, color);
}

/// Right-pointing triangle (play/resume icon).
fn paint_play_icon(
    painter: &egui::Painter,
    rect: egui::Rect,
    color: egui::Color32,
) {
    let s = 10.0;
    let cx = rect.center().x + 1.0; // optical centering
    let cy = rect.center().y;
    let points = vec![
        egui::pos2(cx - s * 0.4, cy - s * 0.5),
        egui::pos2(cx + s * 0.5, cy),
        egui::pos2(cx - s * 0.4, cy + s * 0.5),
    ];
    painter.add(egui::Shape::convex_polygon(
        points,
        color,
        egui::Stroke::NONE,
    ));
}
