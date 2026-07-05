//! Annotation types and rasterization onto the final image.
//!
//! Annotations are stored in *image* coordinates (pixels of the edited image),
//! so they stay attached to the picture regardless of window size/zoom, and
//! export at full resolution.

use ab_glyph::{FontArc, PxScale};
use egui::{Color32, Pos2, Vec2, vec2};
use image::{Rgba, RgbaImage};
use imageproc::drawing::{
    draw_filled_circle_mut, draw_line_segment_mut, draw_polygon_mut, draw_text_mut,
};
use imageproc::point::Point;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Crop,
    Arrow,
    Line,
    Draw,
    Rect,
    Ellipse,
    Text,
}

#[derive(Clone)]
pub enum Annotation {
    Arrow {
        from: Pos2,
        to: Pos2,
        color: Color32,
        width: f32,
    },
    Line {
        from: Pos2,
        to: Pos2,
        color: Color32,
        width: f32,
    },
    Draw {
        points: Vec<Pos2>,
        color: Color32,
        width: f32,
    },
    Rect {
        from: Pos2,
        to: Pos2,
        color: Color32,
        width: f32,
    },
    Ellipse {
        from: Pos2, // bounding box corner
        to: Pos2,
        color: Color32,
        width: f32,
    },
    Text {
        pos: Pos2,
        text: String,
        color: Color32,
        size: f32,
    },
}

impl Annotation {
    /// Shift the annotation, used when the image is cropped.
    pub fn translate(&mut self, delta: Vec2) {
        match self {
            Annotation::Arrow { from, to, .. }
            | Annotation::Line { from, to, .. }
            | Annotation::Rect { from, to, .. }
            | Annotation::Ellipse { from, to, .. } => {
                *from += delta;
                *to += delta;
            }
            Annotation::Draw { points, .. } => {
                for p in points {
                    *p += delta;
                }
            }
            Annotation::Text { pos, .. } => *pos += delta,
        }
    }
}

/// Smooth a raw freehand stroke with a Catmull-Rom spline so it looks fluid
/// (Excalidraw-style) instead of a jagged polyline. The curve passes through
/// every captured point. Shared by the egui preview and the rasterizer.
pub fn smooth_stroke(points: &[Pos2]) -> Vec<Pos2> {
    const SUBDIV: usize = 6;
    let n = points.len();
    if n < 3 {
        return points.to_vec();
    }
    let get = |i: isize| points[i.clamp(0, n as isize - 1) as usize];
    let mut out = Vec::with_capacity((n - 1) * SUBDIV + 1);
    for i in 0..n - 1 {
        let p0 = get(i as isize - 1);
        let p1 = get(i as isize);
        let p2 = get(i as isize + 1);
        let p3 = get(i as isize + 2);
        for step in 0..SUBDIV {
            let t = step as f32 / SUBDIV as f32;
            let t2 = t * t;
            let t3 = t2 * t;
            // Uniform Catmull-Rom basis.
            let x = 0.5
                * ((2.0 * p1.x)
                    + (-p0.x + p2.x) * t
                    + (2.0 * p0.x - 5.0 * p1.x + 4.0 * p2.x - p3.x) * t2
                    + (-p0.x + 3.0 * p1.x - 3.0 * p2.x + p3.x) * t3);
            let y = 0.5
                * ((2.0 * p1.y)
                    + (-p0.y + p2.y) * t
                    + (2.0 * p0.y - 5.0 * p1.y + 4.0 * p2.y - p3.y) * t2
                    + (-p0.y + 3.0 * p1.y - 3.0 * p2.y + p3.y) * t3);
            out.push(Pos2::new(x, y));
        }
    }
    out.push(points[n - 1]);
    out
}

/// Sample the outline of the ellipse inscribed in the box `from`..`to`.
/// Shared by the egui preview and the rasterizer.
pub fn ellipse_path(from: Pos2, to: Pos2) -> Vec<Pos2> {
    let cx = (from.x + to.x) * 0.5;
    let cy = (from.y + to.y) * 0.5;
    let rx = (to.x - from.x).abs() * 0.5;
    let ry = (to.y - from.y).abs() * 0.5;
    // Enough segments to look round at this size.
    let n = ((rx.max(ry) * 0.7) as usize).clamp(24, 256);
    (0..n)
        .map(|i| {
            let a = i as f32 / n as f32 * std::f32::consts::TAU;
            Pos2::new(cx + rx * a.cos(), cy + ry * a.sin())
        })
        .collect()
}

/// Geometry of an arrow: shaft end point and head triangle, in the same
/// coordinate space as `from`/`to`. Shared by the egui preview and the
/// rasterizer so what you see is what you export.
pub fn arrow_geometry(from: Pos2, to: Pos2, width: f32) -> (Pos2, [Pos2; 3]) {
    let v = to - from;
    let len = v.length();
    let head_len = (width * 4.0).clamp(8.0, len.max(8.0));
    let dir = if len > 0.01 { v / len } else { vec2(1.0, 0.0) };
    let perp = vec2(-dir.y, dir.x);
    let base = to - dir * head_len;
    let half_w = head_len * 0.5;
    (
        base,
        [to, base + perp * half_w, base - perp * half_w],
    )
}

fn to_rgba(c: Color32) -> Rgba<u8> {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    Rgba([r, g, b, a])
}

fn pt(p: Pos2) -> Point<i32> {
    Point::new(p.x.round() as i32, p.y.round() as i32)
}

fn draw_thick_line(img: &mut RgbaImage, from: Pos2, to: Pos2, width: f32, color: Rgba<u8>) {
    let r = width / 2.0;
    if (to - from).length() < 0.5 {
        draw_filled_circle_mut(img, (from.x as i32, from.y as i32), r.max(1.0) as i32, color);
        return;
    }
    if r < 1.0 {
        draw_line_segment_mut(img, (from.x, from.y), (to.x, to.y), color);
        return;
    }
    let dir = (to - from).normalized();
    let perp = vec2(-dir.y, dir.x) * r;
    let quad = [
        pt(from + perp),
        pt(to + perp),
        pt(to - perp),
        pt(from - perp),
    ];
    // draw_polygon_mut requires first != last; degenerate quads fall back to a 1px line.
    let degenerate = quad[0] == quad[3] || quad[1] == quad[2] || quad[0] == quad[1];
    if degenerate {
        draw_line_segment_mut(img, (from.x, from.y), (to.x, to.y), color);
    } else {
        draw_polygon_mut(img, &quad, color);
    }
    // Round caps.
    let cap_r = r as i32;
    if cap_r >= 1 {
        draw_filled_circle_mut(img, (from.x as i32, from.y as i32), cap_r, color);
        draw_filled_circle_mut(img, (to.x as i32, to.y as i32), cap_r, color);
    }
}

fn draw_annotation(img: &mut RgbaImage, a: &Annotation, font: &FontArc) {
    match a {
        Annotation::Line {
            from,
            to,
            color,
            width,
        } => draw_thick_line(img, *from, *to, *width, to_rgba(*color)),
        Annotation::Arrow {
            from,
            to,
            color,
            width,
        } => {
            let rgba = to_rgba(*color);
            let (shaft_end, head) = arrow_geometry(*from, *to, *width);
            draw_thick_line(img, *from, shaft_end, *width, rgba);
            let tri = [pt(head[0]), pt(head[1]), pt(head[2])];
            if tri[0] != tri[1] && tri[1] != tri[2] && tri[0] != tri[2] {
                draw_polygon_mut(img, &tri, rgba);
            }
        }
        Annotation::Draw {
            points,
            color,
            width,
        } => {
            let rgba = to_rgba(*color);
            match points.len() {
                0 => {}
                1 => draw_filled_circle_mut(
                    img,
                    (points[0].x as i32, points[0].y as i32),
                    (width / 2.0).max(1.0) as i32,
                    rgba,
                ),
                _ => {
                    let smoothed = smooth_stroke(points);
                    for seg in smoothed.windows(2) {
                        draw_thick_line(img, seg[0], seg[1], *width, rgba);
                    }
                }
            }
        }
        Annotation::Rect {
            from,
            to,
            color,
            width,
        } => {
            let rgba = to_rgba(*color);
            let (a, b) = (*from, *to);
            let corners = [
                a,
                Pos2::new(b.x, a.y),
                b,
                Pos2::new(a.x, b.y),
            ];
            for i in 0..4 {
                draw_thick_line(img, corners[i], corners[(i + 1) % 4], *width, rgba);
            }
        }
        Annotation::Ellipse {
            from,
            to,
            color,
            width,
        } => {
            let rgba = to_rgba(*color);
            if (to.x - from.x).abs() < 2.0 || (to.y - from.y).abs() < 2.0 {
                // Degenerate ellipse: draw as a line.
                draw_thick_line(img, *from, *to, *width, rgba);
            } else {
                let path = ellipse_path(*from, *to);
                for i in 0..path.len() {
                    draw_thick_line(img, path[i], path[(i + 1) % path.len()], *width, rgba);
                }
            }
        }
        Annotation::Text {
            pos,
            text,
            color,
            size,
        } => {
            let rgba = to_rgba(*color);
            let scale = PxScale::from(*size);
            let line_height = size * 1.25;
            for (i, line) in text.lines().enumerate() {
                if line.is_empty() {
                    continue;
                }
                draw_text_mut(
                    img,
                    rgba,
                    pos.x.round() as i32,
                    (pos.y + i as f32 * line_height).round() as i32,
                    scale,
                    font,
                    line,
                );
            }
        }
    }
}

/// Render the base image with all annotations baked in (for save / clipboard).
pub fn flatten(base: &RgbaImage, annotations: &[Annotation], font: &FontArc) -> RgbaImage {
    let mut img = base.clone();
    for a in annotations {
        draw_annotation(&mut img, a, font);
    }
    img
}

/// Extract the default proportional font that egui ships, so the rasterized
/// text matches the on-screen preview without bundling a font file.
#[allow(clippy::expect_used)]
pub fn default_font() -> FontArc {
    let defs = egui::FontDefinitions::default();
    let name = defs
        .families
        .get(&egui::FontFamily::Proportional)
        .and_then(|list| list.first())
        .expect("egui has a default proportional font");
    let data = defs
        .font_data
        .get(name)
        .expect("font data exists for default font");
    FontArc::try_from_vec(data.font.to_vec()).expect("egui default font parses")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> RgbaImage {
        RgbaImage::from_pixel(200, 100, Rgba([255, 255, 255, 255]))
    }

    fn count_red(img: &RgbaImage) -> usize {
        img.pixels().filter(|p| p.0 == [255, 0, 0, 255]).count()
    }

    #[test]
    fn flatten_draws_all_annotation_kinds() {
        let font = default_font();
        let red = Color32::from_rgb(255, 0, 0);
        let annotations = vec![
            Annotation::Line {
                from: Pos2::new(10.0, 10.0),
                to: Pos2::new(100.0, 10.0),
                color: red,
                width: 4.0,
            },
            Annotation::Arrow {
                from: Pos2::new(10.0, 50.0),
                to: Pos2::new(150.0, 80.0),
                color: red,
                width: 4.0,
            },
            Annotation::Text {
                pos: Pos2::new(20.0, 25.0),
                text: "Hi".into(),
                color: red,
                size: 20.0,
            },
            Annotation::Draw {
                points: vec![
                    Pos2::new(10.0, 90.0),
                    Pos2::new(50.0, 70.0),
                    Pos2::new(90.0, 90.0),
                    Pos2::new(130.0, 70.0),
                ],
                color: red,
                width: 3.0,
            },
            Annotation::Rect {
                from: Pos2::new(150.0, 10.0),
                to: Pos2::new(190.0, 40.0),
                color: red,
                width: 2.0,
            },
            Annotation::Ellipse {
                from: Pos2::new(150.0, 50.0),
                to: Pos2::new(190.0, 90.0),
                color: red,
                width: 2.0,
            },
        ];
        let out = flatten(&base(), &annotations, &font);
        assert_eq!(out.dimensions(), (200, 100));
        // Line + arrow + text should all have left red marks.
        assert!(count_red(&out) > 300, "annotations were not rasterized");
        // The base image is untouched.
        assert_eq!(count_red(&base()), 0);
    }

    #[test]
    fn degenerate_shapes_do_not_panic() {
        let font = default_font();
        let red = Color32::from_rgb(255, 0, 0);
        let annotations = vec![
            Annotation::Line {
                from: Pos2::new(10.0, 10.0),
                to: Pos2::new(10.0, 10.0), // zero length
                color: red,
                width: 1.0,
            },
            Annotation::Arrow {
                from: Pos2::new(5.0, 5.0),
                to: Pos2::new(5.5, 5.0), // tiny arrow
                color: red,
                width: 1.0,
            },
            Annotation::Text {
                pos: Pos2::new(190.0, 95.0), // partially off-canvas
                text: "clipped\n\nlines".into(),
                color: red,
                size: 30.0,
            },
            Annotation::Draw {
                points: vec![Pos2::new(30.0, 30.0)], // single dot
                color: red,
                width: 4.0,
            },
            Annotation::Draw {
                points: vec![], // empty stroke
                color: red,
                width: 4.0,
            },
            Annotation::Rect {
                from: Pos2::new(40.0, 40.0),
                to: Pos2::new(40.0, 40.0), // zero size
                color: red,
                width: 3.0,
            },
            Annotation::Ellipse {
                from: Pos2::new(60.0, 60.0),
                to: Pos2::new(60.5, 90.0), // ~zero width
                color: red,
                width: 3.0,
            },
        ];
        let out = flatten(&base(), &annotations, &font);
        assert_eq!(out.dimensions(), (200, 100));
    }

    #[test]
    fn smooth_stroke_interpolates_through_endpoints() {
        let raw = vec![
            Pos2::new(0.0, 0.0),
            Pos2::new(10.0, 20.0),
            Pos2::new(30.0, 5.0),
            Pos2::new(50.0, 25.0),
        ];
        let smooth = smooth_stroke(&raw);
        assert!(smooth.len() > raw.len(), "spline should add points");
        assert_eq!(*smooth.first().unwrap(), raw[0]);
        assert_eq!(*smooth.last().unwrap(), raw[3]);
        // Short strokes pass through unchanged.
        assert_eq!(smooth_stroke(&raw[..2]).len(), 2);
    }

    #[test]
    fn translate_shifts_annotations() {
        let mut a = Annotation::Arrow {
            from: Pos2::new(10.0, 10.0),
            to: Pos2::new(20.0, 20.0),
            color: Color32::RED,
            width: 2.0,
        };
        a.translate(egui::vec2(-5.0, -5.0));
        match a {
            Annotation::Arrow { from, to, .. } => {
                assert_eq!(from, Pos2::new(5.0, 5.0));
                assert_eq!(to, Pos2::new(15.0, 15.0));
            }
            _ => unreachable!(),
        }
    }
}
