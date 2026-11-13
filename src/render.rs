use crate::capture::Result;
use cairo::{Context, Format, ImageSurface, LinearGradient, RadialGradient};
use std::f64::consts::{PI, TAU};

pub type Color = (f64, f64, f64, f64);
pub type Point = (f64, f64);
#[derive(Clone, Copy, PartialEq)]
pub enum Tool {
    Crop,
    Pen,
    Line,
    Arrow,
    Rectangle,
    Ellipse,
    Text,
    Blur,
    Redact,
}
#[derive(Clone, Copy)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}
impl Rect {
    pub fn between(a: Point, b: Point) -> Self {
        Self {
            x: a.0.min(b.0),
            y: a.1.min(b.1),
            w: (a.0 - b.0).abs(),
            h: (a.1 - b.1).abs(),
        }
    }
}
#[derive(Clone)]
pub struct Mark {
    pub tool: Tool,
    pub points: Vec<Point>,
    pub color: Color,
    pub width: f64,
    pub text: String,
}
#[derive(Clone, Copy, PartialEq)]
pub enum Mode {
    Original,
    Shadow,
    Gradient,
    Image,
}
#[derive(Clone, Copy, PartialEq)]
pub enum Gradient {
    Linear,
    Radial,
    Conic,
}
#[derive(Clone)]
pub struct Overlay {
    pub right: bool,
    pub bottom: bool,
    pub offset: Point,
    pub canvas_inset: [f64; 4],
    pub content: OverlayContent,
}
#[derive(Clone)]
pub enum OverlayContent {
    Image {
        surface: ImageSurface,
        size: Point,
        opacity: f64,
        flip: (bool, bool),
        nearest: bool,
    },
    Circle {
        diameter: f64,
        color: Color,
        outline: Option<(f64, Color)>,
    },
    Border {
        width: f64,
        color: Color,
    },
    Text {
        text: String,
        font: String,
        points: f64,
        color: Color,
        padding: [f64; 4],
    },
}
impl Overlay {
    fn position(&self, w: f64, h: f64, width: f64, height: f64) -> Point {
        let (x, y) = self.offset;
        (
            if self.right { w - width - x } else { x },
            if self.bottom { h - height - y } else { y },
        )
    }
    fn paint(&self, c: &Context, w: f64, h: f64) -> Result<()> {
        match &self.content {
            OverlayContent::Image {
                surface,
                size: (width, height),
                opacity,
                flip,
                nearest,
            } => {
                let (x, y) = self.position(w, h, *width, *height);
                c.save()?;
                c.translate(x, y);
                c.rectangle(0., 0., *width, *height);
                c.clip();
                c.scale(
                    width / f64::from(surface.width()),
                    height / f64::from(surface.height()),
                );
                let sw = f64::from(surface.width());
                let sh = f64::from(surface.height());
                c.translate(if flip.0 { sw } else { 0. }, if flip.1 { sh } else { 0. });
                c.scale(if flip.0 { -1. } else { 1. }, if flip.1 { -1. } else { 1. });
                c.set_source_surface(surface, 0., 0.)?;
                c.source().set_filter(if *nearest {
                    cairo::Filter::Nearest
                } else {
                    cairo::Filter::Best
                });
                c.paint_with_alpha(*opacity)?;
                c.restore()?;
            }
            OverlayContent::Circle {
                diameter,
                color,
                outline,
            } => {
                let (x, y) = self.position(w, h, *diameter, *diameter);
                let radius = diameter / 2.;
                c.new_path();
                c.arc(x + radius, y + radius, radius, 0., TAU);
                source(c, *color);
                if let Some((width, stroke)) = outline {
                    c.fill_preserve()?;
                    source(c, *stroke);
                    c.set_line_width(*width);
                    c.stroke()?;
                } else {
                    c.fill()?;
                }
            }
            OverlayContent::Border { width, color } => {
                let width = width.min(w / 2.).min(h / 2.);
                c.new_path();
                c.rectangle(width / 2., width / 2., w - width, h - width);
                c.set_line_width(width);
                source(c, *color);
                c.stroke()?;
            }
            OverlayContent::Text {
                text,
                font,
                points,
                color,
                padding: [l, t, r, b],
            } => {
                let layout = pangocairo::functions::create_layout(c);
                let mut description = gtk::pango::FontDescription::from_string(font);
                description.set_absolute_size(points * 96. / 72. * gtk::pango::SCALE as f64);
                layout.set_font_description(Some(&description));
                layout.set_text(text);
                let (width, height) = layout.pixel_size();
                let (x, y) =
                    self.position(w, h, f64::from(width) + l + r, f64::from(height) + t + b);
                source(c, *color);
                c.move_to(x + l, y + t);
                pangocairo::functions::show_layout(c, &layout);
            }
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct Frame {
    pub mode: Mode,
    pub padding: [f64; 4],
    pub adaptive_padding: bool,
    pub extra_space: f64,
    pub radius: f64,
    pub outer_radius: f64,
    pub preset: Option<(String, String)>,
    pub shadow: f64,
    pub shadow_offset: Point,
    pub shadow_color: Color,
    pub gradient: Gradient,
    pub angle: f64,
    pub stops: Vec<(f64, Color)>,
    pub background: Option<ImageSurface>,
    pub canvas_color: Option<Color>,
    pub canvas_radius: Option<f64>,
    pub canvas_rim: Option<(Color, f64)>,
    pub overlays: Vec<Overlay>,
}
impl Default for Frame {
    fn default() -> Self {
        Self {
            mode: Mode::Original,
            padding: [72.; 4],
            adaptive_padding: true,
            extra_space: 0.,
            radius: 18.,
            outer_radius: 0.,
            preset: None,
            shadow: 24.,
            shadow_offset: (0., 12.),
            shadow_color: (0., 0., 0., 0.48),
            gradient: Gradient::Linear,
            angle: 135.,
            stops: vec![(0., (0.28, 0.87, 0.60, 1.)), (1., (0.25, 0.47, 0.86, 1.))],
            background: None,
            canvas_color: None,
            canvas_radius: None,
            canvas_rim: None,
            overlays: vec![],
        }
    }
}
#[derive(Clone)]
pub struct Edit {
    pub crop: Rect,
    pub marks: Vec<Mark>,
    pub frame: Frame,
}
impl Edit {
    pub fn new(image: &ImageSurface) -> Self {
        Self {
            crop: Rect {
                x: 0.,
                y: 0.,
                w: image.width() as f64,
                h: image.height() as f64,
            },
            marks: vec![],
            frame: Frame::default(),
        }
    }
    fn frame_scale(&self) -> f64 {
        if self.frame.adaptive_padding {
            (self.crop.w.min(self.crop.h) / 600.).clamp(1. / 72., 1.)
        } else {
            1.
        }
    }
    pub fn geometry(&self) -> (f64, f64, i32, i32) {
        let f = &self.frame;
        let scale = self.frame_scale();
        let padding = f.padding.map(|p| (p * scale).round());
        let extra = self.extra_space();
        let [l, t, r, b] = match f.mode {
            Mode::Original => [0.; 4],
            Mode::Shadow if f.shadow > 0. => padding.map(|p| {
                let space = (shadow_space(f) * scale).ceil();
                if f.canvas_color.is_some() {
                    p + space
                } else {
                    p.max(space)
                }
            }),
            _ => padding,
        }
        .map(|p| p + extra);
        (
            l,
            t,
            (l + self.crop.w + r).ceil() as i32,
            (t + self.crop.h + b).ceil() as i32,
        )
    }
    fn extra_space(&self) -> f64 {
        if self.frame.mode == Mode::Original {
            0.
        } else {
            self.frame.extra_space.round().max(0.)
        }
    }
}

fn shadow_space(f: &Frame) -> f64 {
    f.shadow * 2. + f.shadow_offset.0.abs().max(f.shadow_offset.1.abs())
}
fn source(c: &Context, (r, g, b, a): Color) {
    c.set_source_rgba(r, g, b, a);
}
fn round(c: &Context, r: Rect, radius: f64) {
    let d = radius.min(r.w / 2.).min(r.h / 2.).max(0.);
    c.new_path();
    if d == 0. {
        c.rectangle(r.x, r.y, r.w, r.h);
        return;
    }
    c.arc(r.x + r.w - d, r.y + d, d, -PI / 2., 0.);
    c.arc(r.x + r.w - d, r.y + r.h - d, d, 0., PI / 2.);
    c.arc(r.x + d, r.y + r.h - d, d, PI / 2., PI);
    c.arc(r.x + d, r.y + d, d, PI, PI * 1.5);
    c.close_path();
}

fn background(c: &Context, f: &Frame, w: f64, h: f64) -> Result<()> {
    if f.mode == Mode::Image {
        if let Some(image) = &f.background {
            let iw = image.width() as f64;
            let ih = image.height() as f64;
            let scale = (w / iw).max(h / ih);
            c.save()?;
            c.translate((w - iw * scale) / 2., (h - ih * scale) / 2.);
            c.scale(scale, scale);
            c.set_source_surface(image, 0., 0.)?;
            c.paint()?;
            c.restore()?;
        }
        return Ok(());
    }
    if f.mode != Mode::Gradient || f.stops.len() < 2 {
        return Ok(());
    }
    let angle = (f.angle - 90.).to_radians();
    let (y, x) = angle.sin_cos();
    let (cx, cy) = (w / 2., h / 2.);
    let extent = x.abs() * cx + y.abs() * cy;
    let mut stops = f.stops.clone();
    stops.sort_by(|a, b| a.0.total_cmp(&b.0));
    match f.gradient {
        Gradient::Linear => {
            let g = LinearGradient::new(
                cx - x * extent,
                cy - y * extent,
                cx + x * extent,
                cy + y * extent,
            );
            for (p, (r, gc, b, a)) in stops {
                g.add_color_stop_rgba(p, r, gc, b, a);
            }
            c.set_source(&g)?;
            c.paint()?;
        }
        Gradient::Radial => {
            let (ox, oy) = (cx + x * w * 0.22, cy + y * h * 0.22);
            let rad = ox.max(w - ox).hypot(oy.max(h - oy));
            let g = RadialGradient::new(ox, oy, 0., ox, oy, rad);
            for (p, (r, gc, b, a)) in stops {
                g.add_color_stop_rgba(p, r, gc, b, a);
            }
            c.set_source(&g)?;
            c.paint()?;
        }
        Gradient::Conic => {
            for i in 0..720 {
                let p = i as f64 / 719.;
                let upper = stops.partition_point(|s| s.0 <= p);
                let (a, ca) = stops[upper.saturating_sub(1)];
                let (b, cb) = stops[upper.min(stops.len() - 1)];
                let t = if a == b { 0. } else { (p - a) / (b - a) };
                let lerp = |a: f64, b: f64| a + (b - a) * t;
                source(
                    c,
                    (
                        lerp(ca.0, cb.0),
                        lerp(ca.1, cb.1),
                        lerp(ca.2, cb.2),
                        lerp(ca.3, cb.3),
                    ),
                );
                let start = angle + TAU * i as f64 / 720.;
                c.move_to(cx, cy);
                c.arc(cx, cy, w.hypot(h), start, start + TAU / 720. + 0.002);
                c.close_path();
                c.fill()?;
            }
        }
    }
    Ok(())
}

pub fn paint(c: &Context, image: &ImageSurface, edit: &Edit, draft: Option<&Mark>) -> Result<()> {
    let (x, y, w, h) = edit.geometry();
    let f = &edit.frame;
    let bounds = Rect {
        x,
        y,
        w: edit.crop.w,
        h: edit.crop.h,
    };
    c.save()?;
    clip_outer(c, f, w, h);
    let inset = if f.mode == Mode::Shadow && f.shadow > 0. && f.canvas_color.is_some() {
        (shadow_space(f) * edit.frame_scale()).ceil()
    } else {
        0.
    } + edit.extra_space();
    let canvas = Rect {
        x: inset,
        y: inset,
        w: f64::from(w) - 2. * inset,
        h: f64::from(h) - 2. * inset,
    };
    let shadow_bounds = if f.canvas_color.is_some() {
        canvas
    } else {
        bounds
    };
    background(c, f, w as f64, h as f64)?;
    let radius = if f.mode == Mode::Original {
        0.
    } else {
        f.radius
    };
    if f.mode != Mode::Original && f.shadow > 0. {
        let (r, g, b, a) = f.shadow_color;
        for step in (1..=28).rev() {
            let scale = edit.frame_scale();
            let d = f.shadow * scale * 1.65 * step as f64 / 28.;
            round(
                c,
                Rect {
                    x: shadow_bounds.x + f.shadow_offset.0 * scale - d,
                    y: shadow_bounds.y + f.shadow_offset.1 * scale - d,
                    w: shadow_bounds.w + 2. * d,
                    h: shadow_bounds.h + 2. * d,
                },
                f.canvas_radius.unwrap_or(radius) + d,
            );
            source(c, (r, g, b, a * (1. - step as f64 / 29.) / 14.));
            c.fill()?;
        }
    }
    c.save()?;
    if f.mode != Mode::Original {
        if let Some(color) = f.canvas_color {
            round(c, canvas, f.canvas_radius.unwrap_or(0.));
            c.clip();
            source(c, f.canvas_rim.map_or(color, |(rim, _)| rim));
            c.paint()?;
            let rim = f.canvas_rim.map_or(0., |(_, size)| size);
            let inner = Rect {
                x: canvas.x + rim,
                y: canvas.y + rim,
                w: (canvas.w - 2. * rim).max(0.),
                h: (canvas.h - 2. * rim).max(0.),
            };
            round(c, inner, f.canvas_radius.unwrap_or(0.));
            source(c, color);
            c.fill()?;
        }
    }
    c.save()?;
    round(c, bounds, radius);
    c.clip();
    c.translate(x - edit.crop.x, y - edit.crop.y);
    c.set_source_surface(image, 0., 0.)?;
    c.paint()?;
    for mark in edit
        .marks
        .iter()
        .chain(draft)
        .filter(|mark| mark.tool != Tool::Redact)
    {
        draw(c, image, mark)?;
    }
    for mark in edit
        .marks
        .iter()
        .chain(draft)
        .filter(|mark| mark.tool == Tool::Redact)
    {
        draw(c, image, mark)?;
    }
    c.restore()?;
    if f.mode != Mode::Original {
        for overlay in &f.overlays {
            let [l, t, r, b] = overlay.canvas_inset;
            c.save()?;
            c.translate(canvas.x + l, canvas.y + t);
            overlay.paint(c, canvas.w - l - r, canvas.h - t - b)?;
            c.restore()?;
        }
    }
    c.restore()?;
    c.restore()?;
    Ok(())
}

fn clip_outer(c: &Context, frame: &Frame, w: i32, h: i32) {
    if frame.mode != Mode::Original && frame.outer_radius > 0. {
        round(
            c,
            Rect {
                x: 0.,
                y: 0.,
                w: f64::from(w),
                h: f64::from(h),
            },
            frame.outer_radius,
        );
        c.clip();
    }
}

pub fn paint_draft(c: &Context, image: &ImageSurface, edit: &Edit, draft: &Mark) -> Result<()> {
    let (x, y, w, h) = edit.geometry();
    let radius = if edit.frame.mode == Mode::Original {
        0.
    } else {
        edit.frame.radius
    };
    c.save()?;
    clip_outer(c, &edit.frame, w, h);
    round(
        c,
        Rect {
            x,
            y,
            w: edit.crop.w,
            h: edit.crop.h,
        },
        radius,
    );
    c.clip();
    c.translate(x - edit.crop.x, y - edit.crop.y);
    draw(c, image, draft)?;
    if draft.tool != Tool::Redact {
        for mark in edit.marks.iter().filter(|mark| mark.tool == Tool::Redact) {
            draw(c, image, mark)?;
        }
    }
    c.restore()?;
    Ok(())
}

fn draw(c: &Context, image: &ImageSurface, m: &Mark) -> Result<()> {
    let a = m.points[0];
    let b = *m.points.last().unwrap();
    let r = Rect::between(a, b);
    let delta = (b.0 - a.0, b.1 - a.1);
    source(c, m.color);
    c.set_line_width(m.width);
    c.set_line_cap(cairo::LineCap::Round);
    c.set_line_join(cairo::LineJoin::Round);
    match m.tool {
        Tool::Crop | Tool::Rectangle | Tool::Redact => {
            c.rectangle(r.x, r.y, r.w, r.h);
            match m.tool {
                Tool::Crop => {
                    c.set_dash(&[8., 5.], 0.);
                    c.stroke()?;
                    c.set_dash(&[], 0.);
                }
                Tool::Redact => {
                    source(c, (m.color.0, m.color.1, m.color.2, 1.));
                    c.fill()?;
                }
                _ => c.stroke()?,
            }
        }
        Tool::Ellipse => {
            if r.w < 1. || r.h < 1. {
                return Ok(());
            }
            c.save()?;
            c.translate(r.x + r.w / 2., r.y + r.h / 2.);
            c.scale(r.w / 2., r.h / 2.);
            c.arc(0., 0., 1., 0., TAU);
            c.restore()?;
            c.stroke()?;
        }
        Tool::Pen | Tool::Line | Tool::Arrow => {
            c.move_to(a.0, a.1);
            if m.tool == Tool::Pen {
                for p in &m.points[1..] {
                    c.line_to(p.0, p.1);
                }
            } else {
                c.line_to(b.0, b.1);
            }
            if m.tool == Tool::Arrow {
                let theta = delta.1.atan2(delta.0);
                let length = (m.width * 4.).min(delta.0.hypot(delta.1) * 0.45);
                for side in [-0.55, 0.55] {
                    c.move_to(b.0, b.1);
                    c.line_to(
                        b.0 - length * (theta + side).cos(),
                        b.1 - length * (theta + side).sin(),
                    );
                }
            }
            c.stroke()?;
        }
        Tool::Text => {
            let layout = pangocairo::functions::create_layout(c);
            let mut font = gtk::pango::FontDescription::from_string("Sans Semi-Bold");
            font.set_absolute_size(m.width * gtk::pango::SCALE as f64);
            layout.set_font_description(Some(&font));
            layout.set_text(&m.text);
            c.move_to(a.0, a.1);
            pangocairo::functions::show_layout(c, &layout);
        }
        Tool::Blur => {
            if r.w < 1. || r.h < 1. {
                return Ok(());
            }
            c.save()?;
            c.rectangle(r.x, r.y, r.w, r.h);
            c.clip();
            let size = m.width.clamp(2., 36.);
            let weights = [1., 4., 6., 4., 1.];
            c.push_group();
            c.set_operator(cairo::Operator::Add);
            for (j, wy) in weights.iter().enumerate() {
                for (i, wx) in weights.iter().enumerate() {
                    let pattern = cairo::SurfacePattern::create(image);
                    pattern.set_extend(cairo::Extend::Pad);
                    pattern.set_matrix(cairo::Matrix::new(
                        1.,
                        0.,
                        0.,
                        1.,
                        (i as f64 - 2.) * size / 2.,
                        (j as f64 - 2.) * size / 2.,
                    ));
                    c.set_source(&pattern)?;
                    c.paint_with_alpha(wx * wy / 256.)?;
                }
            }
            let blurred = c.pop_group()?;
            c.set_operator(cairo::Operator::Source);
            c.set_source(&blurred)?;
            c.paint()?;
            c.restore()?;
        }
    }
    Ok(())
}

pub fn png(image: &ImageSurface, edit: &Edit) -> Result<Vec<u8>> {
    png_scaled(image, edit, 1)
}

pub fn png_scaled(image: &ImageSurface, edit: &Edit, scale: i32) -> Result<Vec<u8>> {
    let (_, _, w, h) = edit.geometry();
    let width = w.checked_mul(scale).ok_or("Export width is too large.")?;
    let height = h.checked_mul(scale).ok_or("Export height is too large.")?;
    if scale > 1
        && (i64::from(width) * i64::from(height) > 64_000_000 || width > 32_767 || height > 32_767)
    {
        return Err("This export is too large. Choose a smaller export scale.".into());
    }
    let result = ImageSurface::create(Format::ARgb32, width, height)?;
    let c = Context::new(&result)?;
    c.scale(f64::from(scale), f64::from(scale));
    paint(&c, image, edit, None)?;
    let mut bytes = Vec::new();
    result.write_to_png(&mut bytes)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn scaled_png_renders_vector_marks_at_output_resolution() {
        let image = ImageSurface::create(Format::ARgb32, 39, 25).unwrap();
        let mut edit = Edit::new(&image);
        edit.marks.push(Mark {
            tool: Tool::Rectangle,
            points: vec![(4.25, 4.25), (16.25, 16.25)],
            color: (1., 0., 0., 1.),
            width: 0.5,
            text: String::new(),
        });
        let bytes = png_scaled(&image, &edit, 4).unwrap();
        let mut result = ImageSurface::create_from_png(&mut Cursor::new(bytes)).unwrap();
        assert_eq!((result.width(), result.height()), (156, 100));
        let stride = result.stride() as usize;
        let pixels = result.data().unwrap();
        let pixel = |x: usize, y: usize| {
            u32::from_ne_bytes(
                pixels[y * stride + x * 4..y * stride + x * 4 + 4]
                    .try_into()
                    .unwrap(),
            )
        };
        assert_eq!(pixel(30, 16), 0xffff0000);
        assert_eq!(pixel(30, 18), 0);
    }

    #[test]
    fn adaptive_padding_tracks_the_short_edge() {
        let image = ImageSurface::create(Format::ARgb32, 39, 25).unwrap();
        let mut edit = Edit::new(&image);
        edit.frame.mode = Mode::Gradient;
        assert_eq!(edit.geometry(), (3., 3., 45, 31));
        edit.crop.w = 1920.;
        edit.crop.h = 1080.;
        assert_eq!(edit.geometry(), (72., 72., 2064, 1224));
        edit.crop.w = 39.;
        edit.crop.h = 25.;
        edit.frame.adaptive_padding = false;
        assert_eq!(edit.geometry(), (72., 72., 183, 169));
    }

    #[test]
    fn extra_space_keeps_preset_margins() {
        let image = ImageSurface::create(Format::ARgb32, 100, 50).unwrap();
        let mut edit = Edit::new(&image);
        edit.frame.mode = Mode::Shadow;
        edit.frame.shadow = 0.;
        edit.frame.adaptive_padding = false;
        edit.frame.padding = [10., 40., 10., 10.];
        assert_eq!(edit.geometry(), (10., 40., 120, 100));
        edit.frame.extra_space = 8.;
        assert_eq!(edit.geometry(), (18., 48., 136, 116));
    }

    #[test]
    fn redact_uses_its_own_color() {
        let image = ImageSurface::create(Format::ARgb32, 16, 16).unwrap();
        let mut edit = Edit::new(&image);
        edit.marks.push(Mark {
            tool: Tool::Redact,
            points: vec![(0., 0.), (16., 16.)],
            color: (0., 0., 1., 1.),
            width: 1.,
            text: String::new(),
        });
        let mut output =
            ImageSurface::create_from_png(&mut Cursor::new(png(&image, &edit).unwrap())).unwrap();
        let pixel = 8 * output.stride() as usize + 8 * 4;
        let value =
            u32::from_ne_bytes(output.data().unwrap()[pixel..pixel + 4].try_into().unwrap());
        assert_eq!(value & 0xffffff, 0x0000ff);
    }

    #[test]
    fn blur_cannot_restore_redacted_pixels() {
        let image = ImageSurface::create(Format::ARgb32, 64, 64).unwrap();
        let cr = Context::new(&image).unwrap();
        cr.set_source_rgb(1., 0., 0.);
        cr.paint().unwrap();
        let mut edit = Edit::new(&image);
        for tool in [Tool::Redact, Tool::Blur] {
            edit.marks.push(Mark {
                tool,
                points: vec![(8., 8.), (56., 56.)],
                color: (0., 0., 0., 1.),
                width: 10.,
                text: String::new(),
            });
        }
        let mut output =
            ImageSurface::create_from_png(&mut Cursor::new(png(&image, &edit).unwrap())).unwrap();
        let pixel = 32 * output.stride() as usize + 32 * 4;
        assert!(
            u32::from_ne_bytes(output.data().unwrap()[pixel..pixel + 4].try_into().unwrap())
                & 0xff0000
                < 0x200000
        );
    }

    #[test]
    fn original_png_preserves_pixels_and_framing_preserves_source_size() {
        let image = ImageSurface::create(Format::ARgb32, 320, 190).unwrap();
        let cr = Context::new(&image).unwrap();
        cr.set_source_rgb(1., 0., 0.);
        cr.paint().unwrap();
        let mut edit = Edit::new(&image);
        let mut original =
            ImageSurface::create_from_png(&mut Cursor::new(png(&image, &edit).unwrap())).unwrap();
        assert_eq!((original.width(), original.height()), (320, 190));
        assert_eq!(&original.data().unwrap()[..4], &0xffff0000u32.to_ne_bytes());
        for gradient in [Gradient::Linear, Gradient::Radial, Gradient::Conic] {
            edit.frame = Frame {
                mode: Mode::Gradient,
                gradient,
                padding: [20., 20., 20., 10.],
                adaptive_padding: false,
                ..Frame::default()
            };
            let mut output =
                ImageSurface::create_from_png(&mut Cursor::new(png(&image, &edit).unwrap()))
                    .unwrap();
            assert_eq!((output.width(), output.height()), (360, 220));
            let pixel = 100 * output.stride() as usize + 100 * 4;
            assert_eq!(
                &output.data().unwrap()[pixel..pixel + 4],
                &0xffff0000u32.to_ne_bytes()
            );
        }
    }
}
