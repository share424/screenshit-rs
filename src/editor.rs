//! The egui image editor: crop, arrow, line, text, undo/redo, save, copy.

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::Arc;

use ab_glyph::FontArc;
use egui::{
    Align2, Color32, ColorImage, CornerRadius, CursorIcon, FontId, Key, KeyboardShortcut,
    Modifiers, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, TextureHandle, TextureOptions, pos2,
    vec2,
};
use image::RgbaImage;

use crate::annotate::{self, Annotation, Tool};

const UNDO: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Z);
const REDO: KeyboardShortcut = KeyboardShortcut::new(
    Modifiers::COMMAND.plus(Modifiers::SHIFT),
    Key::Z,
);
const REDO_Y: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Y);
const COPY: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::C);
const SAVE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);

struct Snapshot {
    image: Arc<RgbaImage>,
    annotations: Vec<Annotation>,
}

struct TextEditState {
    pos: Pos2, // image coords
    buffer: String,
    just_created: bool,
}

struct Drag {
    start: Pos2, // image coords
    current: Pos2,
    /// Captured trail for the freehand Draw tool.
    points: Vec<Pos2>,
}

/// Constrain `to` so the box from..to is square (Shift held: circle/square).
fn constrain_square(from: Pos2, to: Pos2) -> Pos2 {
    let d = to - from;
    let m = d.x.abs().max(d.y.abs());
    pos2(from.x + m * d.x.signum(), from.y + m * d.y.signum())
}

pub struct EditorApp {
    image: Arc<RgbaImage>,
    source_path: Option<PathBuf>,
    texture: Option<TextureHandle>,
    texture_dirty: bool,

    annotations: Vec<Annotation>,
    undo_stack: Vec<Snapshot>,
    redo_stack: Vec<Snapshot>,

    tool: Tool,
    color: Color32,
    stroke_width: f32,
    font_size: f32,

    drag: Option<Drag>,
    crop_sel: Option<Rect>, // image coords
    text_edit: Option<TextEditState>,
    /// Whether the current eraser gesture already pushed an undo snapshot.
    eraser_pushed: bool,

    font: FontArc,
    status: Option<(String, f64)>,
    last_zoom: f32,
}

impl EditorApp {
    pub fn new(image: RgbaImage, source_path: Option<PathBuf>) -> Self {
        Self {
            image: Arc::new(image),
            source_path,
            texture: None,
            texture_dirty: true,
            annotations: Vec::new(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            tool: Tool::Arrow,
            color: Color32::from_rgb(230, 40, 40),
            stroke_width: 4.0,
            font_size: 28.0,
            drag: None,
            crop_sel: None,
            text_edit: None,
            eraser_pushed: false,
            font: annotate::default_font(),
            status: None,
            last_zoom: 1.0,
        }
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            image: Arc::clone(&self.image),
            annotations: self.annotations.clone(),
        }
    }

    fn push_undo(&mut self) {
        self.undo_stack.push(self.snapshot());
        self.redo_stack.clear();
    }

    fn undo(&mut self) {
        if let Some(prev) = self.undo_stack.pop() {
            self.redo_stack.push(self.snapshot());
            self.restore(prev);
        }
    }

    fn redo(&mut self) {
        if let Some(next) = self.redo_stack.pop() {
            self.undo_stack.push(self.snapshot());
            self.restore(next);
        }
    }

    fn restore(&mut self, s: Snapshot) {
        if !Arc::ptr_eq(&self.image, &s.image) {
            self.texture_dirty = true;
        }
        self.image = s.image;
        self.annotations = s.annotations;
        self.crop_sel = None;
        self.drag = None;
    }

    fn image_size(&self) -> (u32, u32) {
        (self.image.width(), self.image.height())
    }

    fn set_status(&mut self, ctx: &egui::Context, msg: impl Into<String>) {
        self.status = Some((msg.into(), ctx.input(|i| i.time)));
    }

    fn flatten(&self) -> RgbaImage {
        annotate::flatten(&self.image, &self.annotations, &self.font)
    }

    fn copy_to_clipboard(&mut self, ctx: &egui::Context) {
        let img = self.flatten();
        let (w, h) = (img.width() as usize, img.height() as usize);
        let result = arboard::Clipboard::new()
            .and_then(|mut cb| {
                cb.set_image(arboard::ImageData {
                    width: w,
                    height: h,
                    bytes: Cow::Owned(img.into_raw()),
                })
            });
        match result {
            Ok(()) => self.set_status(ctx, "Copied image to clipboard"),
            Err(e) => self.set_status(ctx, format!("Clipboard error: {e}")),
        }
    }

    fn save_dialog(&mut self, ctx: &egui::Context) {
        let default_name = match &self.source_path {
            Some(p) => {
                let stem = p
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| "image".into());
                format!("{stem}-edited.png")
            }
            None => "screenshot.png".to_string(),
        };
        let mut dialog = rfd::FileDialog::new()
            .set_title("Save image")
            .set_file_name(default_name)
            .add_filter("PNG image", &["png"])
            .add_filter("JPEG image", &["jpg", "jpeg"])
            .add_filter("BMP image", &["bmp"]);
        if let Some(dir) = self.source_path.as_ref().and_then(|p| p.parent()) {
            dialog = dialog.set_directory(dir);
        }
        let Some(mut path) = dialog.save_file() else {
            return;
        };
        if path.extension().is_none() {
            path.set_extension("png");
        }
        let img = self.flatten();
        let is_jpeg = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("jpg") || e.eq_ignore_ascii_case("jpeg"))
            .unwrap_or(false);
        let result = if is_jpeg {
            // JPEG has no alpha channel.
            image::DynamicImage::ImageRgba8(img).to_rgb8().save(&path)
        } else {
            img.save(&path)
        };
        match result {
            Ok(()) => self.set_status(ctx, format!("Saved to {}", path.display())),
            Err(e) => self.set_status(ctx, format!("Save failed: {e}")),
        }
    }

    fn apply_crop(&mut self) {
        let Some(sel) = self.crop_sel else { return };
        let (w, h) = self.image_size();
        let bounds = Rect::from_min_size(pos2(0.0, 0.0), vec2(w as f32, h as f32));
        let sel = sel.intersect(bounds);
        let (cw, ch) = (sel.width().round() as u32, sel.height().round() as u32);
        if cw < 2 || ch < 2 {
            self.crop_sel = None;
            return;
        }
        self.push_undo();
        let (x, y) = (sel.min.x.round() as u32, sel.min.y.round() as u32);
        let cw = cw.min(w - x);
        let ch = ch.min(h - y);
        let cropped = image::imageops::crop_imm(&*self.image, x, y, cw, ch).to_image();
        self.image = Arc::new(cropped);
        let delta = vec2(-(x as f32), -(y as f32));
        for a in &mut self.annotations {
            a.translate(delta);
        }
        self.crop_sel = None;
        self.texture_dirty = true;
    }

    fn ensure_texture(&mut self, ctx: &egui::Context) {
        if self.texture.is_some() && !self.texture_dirty {
            return;
        }
        let (w, h) = self.image_size();
        let color_image =
            ColorImage::from_rgba_unmultiplied([w as usize, h as usize], self.image.as_raw());
        self.texture = Some(ctx.load_texture("canvas", color_image, TextureOptions::LINEAR));
        self.texture_dirty = false;
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        // While the text overlay is open, leave the keyboard to it.
        if self.text_edit.is_some() {
            return;
        }
        let (undo, redo, copy, save) = ctx.input_mut(|i| {
            (
                i.consume_shortcut(&UNDO),
                i.consume_shortcut(&REDO) || i.consume_shortcut(&REDO_Y),
                i.consume_shortcut(&COPY),
                i.consume_shortcut(&SAVE),
            )
        });
        // Note: consume REDO before UNDO would be needed if both matched; egui
        // matches exact modifiers, so Ctrl+Shift+Z never triggers UNDO.
        if redo {
            self.redo();
        } else if undo {
            self.undo();
        }
        if copy {
            self.copy_to_clipboard(ctx);
        }
        if save {
            self.save_dialog(ctx);
        }
        if ctx.input(|i| i.key_pressed(Key::Enter)) && self.crop_sel.is_some() {
            self.apply_crop();
        }
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.crop_sel = None;
            self.drag = None;
        }
    }

    fn toolbar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.selectable_value(&mut self.tool, Tool::Crop, "✂ Crop");
                ui.selectable_value(&mut self.tool, Tool::Arrow, "↗ Arrow");
                ui.selectable_value(&mut self.tool, Tool::Line, "∕ Line");
                ui.selectable_value(&mut self.tool, Tool::Draw, "✏ Draw");
                ui.selectable_value(&mut self.tool, Tool::Rect, "▭ Rect")
                    .on_hover_text("Hold Shift for a square");
                ui.selectable_value(&mut self.tool, Tool::Ellipse, "○ Circle")
                    .on_hover_text("Hold Shift for a perfect circle");
                ui.selectable_value(&mut self.tool, Tool::Text, "T Text");
                ui.selectable_value(&mut self.tool, Tool::Eraser, "⌫ Erase")
                    .on_hover_text("Click or sweep over a drawn item to remove it");
                ui.separator();

                ui.color_edit_button_srgba(&mut self.color);
                match self.tool {
                    Tool::Text => {
                        ui.add(
                            egui::Slider::new(&mut self.font_size, 8.0..=96.0)
                                .text("Size")
                                .fixed_decimals(0),
                        );
                    }
                    Tool::Arrow | Tool::Line | Tool::Draw | Tool::Rect | Tool::Ellipse => {
                        ui.add(
                            egui::Slider::new(&mut self.stroke_width, 1.0..=20.0)
                                .text("Width")
                                .fixed_decimals(0),
                        );
                    }
                    Tool::Crop | Tool::Eraser => {}
                }
                ui.separator();

                let can_undo = !self.undo_stack.is_empty();
                let can_redo = !self.redo_stack.is_empty();
                if ui
                    .add_enabled(can_undo, egui::Button::new("⟲ Undo"))
                    .on_hover_text("Ctrl+Z")
                    .clicked()
                {
                    self.undo();
                }
                if ui
                    .add_enabled(can_redo, egui::Button::new("⟳ Redo"))
                    .on_hover_text("Ctrl+Shift+Z / Ctrl+Y")
                    .clicked()
                {
                    self.redo();
                }
                ui.separator();

                if ui.button("📋 Copy").on_hover_text("Ctrl+C").clicked() {
                    self.copy_to_clipboard(ctx);
                }
                if ui.button("💾 Save…").on_hover_text("Ctrl+S").clicked() {
                    self.save_dialog(ctx);
                }

                if self.crop_sel.is_some() {
                    ui.separator();
                    if ui.button("Apply crop (Enter)").clicked() {
                        self.apply_crop();
                    }
                    if ui.button("Cancel (Esc)").clicked() {
                        self.crop_sel = None;
                    }
                }
            });
        });
    }

    fn status_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                let (w, h) = self.image_size();
                ui.label(format!("{w}×{h} px"));
                ui.separator();
                ui.label(format!("{:.0}%", self.last_zoom * 100.0));
                if let Some((msg, t)) = &self.status {
                    let age = ctx.input(|i| i.time) - t;
                    if age < 5.0 {
                        ui.separator();
                        ui.label(msg.clone());
                        ctx.request_repaint_after(std::time::Duration::from_millis(500));
                    } else {
                        self.status = None;
                    }
                }
            });
        });
    }

    /// Draw one committed or in-progress annotation as egui shapes.
    fn paint_annotation(
        &self,
        painter: &egui::Painter,
        a: &Annotation,
        to_screen: &impl Fn(Pos2) -> Pos2,
        scale: f32,
    ) {
        match a {
            Annotation::Line {
                from,
                to,
                color,
                width,
            } => {
                painter.line_segment(
                    [to_screen(*from), to_screen(*to)],
                    Stroke::new(width * scale, *color),
                );
            }
            Annotation::Arrow {
                from,
                to,
                color,
                width,
            } => {
                let (shaft_end, head) = annotate::arrow_geometry(*from, *to, *width);
                painter.line_segment(
                    [to_screen(*from), to_screen(shaft_end)],
                    Stroke::new(width * scale, *color),
                );
                painter.add(Shape::convex_polygon(
                    head.iter().map(|p| to_screen(*p)).collect(),
                    *color,
                    Stroke::NONE,
                ));
            }
            Annotation::Draw {
                points,
                color,
                width,
            } => {
                if points.len() == 1 {
                    painter.circle_filled(to_screen(points[0]), (width * scale) / 2.0, *color);
                } else {
                    let pts: Vec<Pos2> = annotate::smooth_stroke(points)
                        .into_iter()
                        .map(&to_screen)
                        .collect();
                    painter.add(Shape::line(pts, Stroke::new(width * scale, *color)));
                }
            }
            Annotation::Rect {
                from,
                to,
                color,
                width,
            } => {
                painter.rect_stroke(
                    Rect::from_two_pos(to_screen(*from), to_screen(*to)),
                    CornerRadius::ZERO,
                    Stroke::new(width * scale, *color),
                    StrokeKind::Middle,
                );
            }
            Annotation::Ellipse {
                from,
                to,
                color,
                width,
            } => {
                let pts: Vec<Pos2> = annotate::ellipse_path(*from, *to)
                    .into_iter()
                    .map(&to_screen)
                    .collect();
                painter.add(Shape::closed_line(pts, Stroke::new(width * scale, *color)));
            }
            Annotation::Text {
                pos,
                text,
                color,
                size,
            } => {
                painter.text(
                    to_screen(*pos),
                    Align2::LEFT_TOP,
                    text,
                    FontId::proportional(size * scale),
                    *color,
                );
            }
        }
    }

    fn canvas(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            self.ensure_texture(ctx);
            let (w, h) = self.image_size();
            let img_size = vec2(w as f32, h as f32);

            let avail = ui.available_size();
            let (response, painter) =
                ui.allocate_painter(avail, Sense::click_and_drag());
            let panel = response.rect;

            let scale = (panel.width() / img_size.x)
                .min(panel.height() / img_size.y)
                .min(4.0)
                .max(0.01);
            self.last_zoom = scale;
            let shown = img_size * scale;
            let image_rect = Rect::from_center_size(panel.center(), shown);

            let to_screen = |p: Pos2| image_rect.min + p.to_vec2() * scale;
            let to_image = |p: Pos2| {
                pos2(
                    ((p.x - image_rect.min.x) / scale).clamp(0.0, img_size.x),
                    ((p.y - image_rect.min.y) / scale).clamp(0.0, img_size.y),
                )
            };

            if let Some(texture) = &self.texture {
                painter.image(
                    texture.id(),
                    image_rect,
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            }

            if response.hovered() {
                ctx.set_cursor_icon(match self.tool {
                    Tool::Text => CursorIcon::Text,
                    _ => CursorIcon::Crosshair,
                });
                // Eraser reach indicator.
                if self.tool == Tool::Eraser {
                    if let Some(hp) = response.hover_pos() {
                        painter.circle_stroke(hp, 8.0, Stroke::new(1.0_f32, Color32::GRAY));
                    }
                }
            }

            // --- interaction -------------------------------------------------
            let pointer = response.interact_pointer_pos().map(to_image);
            let text_was_active = self.text_edit.is_some();

            let shift = ctx.input(|i| i.modifiers.shift);
            if response.drag_started() {
                if let Some(p) = pointer {
                    match self.tool {
                        Tool::Text => {}
                        _ => {
                            self.drag = Some(Drag {
                                start: p,
                                current: p,
                                points: vec![p],
                            });
                            if self.tool == Tool::Crop {
                                self.crop_sel = None;
                            }
                        }
                    }
                }
            }
            if response.dragged() {
                if let (Some(drag), Some(p)) = (self.drag.as_mut(), pointer) {
                    drag.current = p;
                    if self.tool == Tool::Draw {
                        // Decimate: only keep points ~2 screen px apart, the
                        // spline interpolates between them.
                        let min_dist = 2.0 / scale;
                        if drag
                            .points
                            .last()
                            .is_none_or(|last| (p - *last).length() >= min_dist)
                        {
                            drag.points.push(p);
                        }
                    }
                }
            }
            if response.drag_stopped() {
                if let Some(mut drag) = self.drag.take() {
                    let moved = (drag.current - drag.start).length() >= 2.0;
                    let boxed_to = if shift {
                        constrain_square(drag.start, drag.current)
                    } else {
                        drag.current
                    };
                    match self.tool {
                        Tool::Arrow if moved => {
                            self.push_undo();
                            self.annotations.push(Annotation::Arrow {
                                from: drag.start,
                                to: drag.current,
                                color: self.color,
                                width: self.stroke_width,
                            });
                        }
                        Tool::Line if moved => {
                            self.push_undo();
                            self.annotations.push(Annotation::Line {
                                from: drag.start,
                                to: drag.current,
                                color: self.color,
                                width: self.stroke_width,
                            });
                        }
                        Tool::Draw => {
                            if drag.points.last() != Some(&drag.current) {
                                drag.points.push(drag.current);
                            }
                            self.push_undo();
                            self.annotations.push(Annotation::Draw {
                                points: drag.points,
                                color: self.color,
                                width: self.stroke_width,
                            });
                        }
                        Tool::Rect if moved => {
                            self.push_undo();
                            self.annotations.push(Annotation::Rect {
                                from: drag.start,
                                to: boxed_to,
                                color: self.color,
                                width: self.stroke_width,
                            });
                        }
                        Tool::Ellipse if moved => {
                            self.push_undo();
                            self.annotations.push(Annotation::Ellipse {
                                from: drag.start,
                                to: boxed_to,
                                color: self.color,
                                width: self.stroke_width,
                            });
                        }
                        Tool::Crop if moved => {
                            self.crop_sel =
                                Some(Rect::from_two_pos(drag.start, drag.current));
                        }
                        _ => {}
                    }
                }
            }
            if self.tool == Tool::Eraser {
                if response.drag_started() || response.clicked() {
                    self.eraser_pushed = false;
                }
                if response.dragged() || response.clicked() {
                    if let Some(p) = pointer {
                        // ~8 screen px of reach, in image coordinates.
                        let threshold = 8.0 / scale;
                        if self.annotations.iter().any(|a| a.hit(p, threshold)) {
                            if !self.eraser_pushed {
                                self.push_undo();
                                self.eraser_pushed = true;
                            }
                            self.annotations.retain(|a| !a.hit(p, threshold));
                        }
                    }
                }
            }
            if response.clicked() && self.tool == Tool::Text && !text_was_active {
                if let Some(p) = pointer {
                    self.text_edit = Some(TextEditState {
                        pos: p,
                        buffer: String::new(),
                        just_created: true,
                    });
                }
            }

            // --- painting overlays -------------------------------------------
            for a in &self.annotations {
                self.paint_annotation(&painter, a, &to_screen, scale);
            }

            // In-progress shape preview.
            if let Some(drag) = &self.drag {
                let boxed_to = if shift {
                    constrain_square(drag.start, drag.current)
                } else {
                    drag.current
                };
                let preview = match self.tool {
                    Tool::Arrow => Some(Annotation::Arrow {
                        from: drag.start,
                        to: drag.current,
                        color: self.color,
                        width: self.stroke_width,
                    }),
                    Tool::Line => Some(Annotation::Line {
                        from: drag.start,
                        to: drag.current,
                        color: self.color,
                        width: self.stroke_width,
                    }),
                    Tool::Draw => Some(Annotation::Draw {
                        points: drag.points.clone(),
                        color: self.color,
                        width: self.stroke_width,
                    }),
                    Tool::Rect => Some(Annotation::Rect {
                        from: drag.start,
                        to: boxed_to,
                        color: self.color,
                        width: self.stroke_width,
                    }),
                    Tool::Ellipse => Some(Annotation::Ellipse {
                        from: drag.start,
                        to: boxed_to,
                        color: self.color,
                        width: self.stroke_width,
                    }),
                    _ => None,
                };
                if let Some(p) = preview {
                    self.paint_annotation(&painter, &p, &to_screen, scale);
                }
            }

            // Crop selection overlay (live while dragging, or committed).
            let crop_rect = match (&self.drag, self.tool) {
                (Some(d), Tool::Crop) => Some(Rect::from_two_pos(d.start, d.current)),
                _ => self.crop_sel,
            };
            if let Some(sel) = crop_rect {
                let r = Rect::from_min_max(to_screen(sel.min), to_screen(sel.max));
                let dim = Color32::from_black_alpha(120);
                // Darken everything outside the selection.
                let t = Rect::from_min_max(image_rect.min, pos2(image_rect.max.x, r.min.y));
                let b = Rect::from_min_max(pos2(image_rect.min.x, r.max.y), image_rect.max);
                let l = Rect::from_min_max(pos2(image_rect.min.x, r.min.y), pos2(r.min.x, r.max.y));
                let rr = Rect::from_min_max(pos2(r.max.x, r.min.y), pos2(image_rect.max.x, r.max.y));
                for side in [t, b, l, rr] {
                    if side.is_positive() {
                        painter.rect_filled(side, CornerRadius::ZERO, dim);
                    }
                }
                painter.rect_stroke(
                    r,
                    CornerRadius::ZERO,
                    Stroke::new(1.5_f32, Color32::WHITE),
                    StrokeKind::Outside,
                );
            }

            // Text entry overlay.
            self.text_overlay(ctx, &to_screen, scale);
        });
    }

    fn text_overlay(&mut self, ctx: &egui::Context, to_screen: &impl Fn(Pos2) -> Pos2, scale: f32) {
        let color = self.color;
        let font_size = self.font_size;
        let Some(state) = &mut self.text_edit else {
            return;
        };
        let screen_pos = to_screen(state.pos);
        let font = FontId::proportional((font_size * scale).max(8.0));
        let mut commit = false;
        let mut cancel = false;

        egui::Area::new(egui::Id::new("text-entry"))
            .fixed_pos(screen_pos)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                let edit = egui::TextEdit::singleline(&mut state.buffer)
                    .font(font)
                    .text_color(color)
                    .hint_text("text…")
                    .desired_width(300.0);
                let resp = ui.add(edit);
                if state.just_created {
                    resp.request_focus();
                    state.just_created = false;
                }
                if ctx.input(|i| i.key_pressed(Key::Escape)) {
                    cancel = true;
                } else if resp.lost_focus() {
                    commit = true;
                }
            });

        if cancel {
            self.text_edit = None;
        } else if commit {
            let state = self.text_edit.take().unwrap();
            if !state.buffer.trim().is_empty() {
                self.push_undo();
                self.annotations.push(Annotation::Text {
                    pos: state.pos,
                    text: state.buffer,
                    color: self.color,
                    size: self.font_size,
                });
            }
        }
    }
}

impl eframe::App for EditorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_shortcuts(ctx);
        self.toolbar(ctx);
        self.status_bar(ctx);
        self.canvas(ctx);
    }
}
