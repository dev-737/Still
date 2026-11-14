use crate::{
    capture::Result,
    effects::{Bitmap, Effect, Pipeline, rounded_rect},
};
use cairo::{Context, Format, ImageSurface, LinearGradient, RadialGradient};
use std::{cell::RefCell, f64::consts::TAU, rc::Rc};

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
#[derive(Clone, Copy, PartialEq)]
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
    pub pipeline: Option<Pipeline>,
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
            pipeline: None,
        }
    }
}

impl Frame {
    pub fn from_pipeline(pipeline: Pipeline) -> Self {
        Self {
            mode: Mode::Shadow,
            pipeline: Some(pipeline),
            ..Self::default()
        }
    }
}
struct Memo {
    crop: Rect,
    pipeline: Pipeline,
    bitmap: Bitmap,
}

#[derive(Clone)]
pub struct Edit {
    pub crop: Rect,
    pub marks: Vec<Mark>,
    pub frame: Frame,
    source: ImageSurface,
    memo: Rc<RefCell<Option<Memo>>>,
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
            source: image.clone(),
            memo: Rc::default(),
        }
    }
    pub fn with_frame(&self, frame: Frame) -> Self {
        Self {
            frame,
            memo: Rc::default(),
            ..self.clone()
        }
    }
    fn frame_scale(&self) -> f64 {
        if self.frame.adaptive_padding {
            (self.crop.w.min(self.crop.h) / 600.).clamp(1. / 72., 1.)
        } else {
            1.
        }
    }
    fn rendered(&self) -> Option<Bitmap> {
        let pipeline = self.frame.pipeline.as_ref()?;
        let mut memo = self.memo.borrow_mut();
        if let Some(known) = memo.as_ref() {
            if known.crop == self.crop && known.pipeline.same(pipeline) {
                return Some(known.bitmap.clone());
            }
        }
        let bitmap = self.run(pipeline);
        *memo = Some(Memo {
            crop: self.crop,
            pipeline: pipeline.clone(),
            bitmap: bitmap.clone(),
        });
        Some(bitmap)
    }
    fn run(&self, pipeline: &Pipeline) -> Bitmap {
        let (source_w, source_h) = (self.source.width(), self.source.height());
        let left = (self.crop.x.floor() as i32).clamp(0, source_w - 1);
        let top = (self.crop.y.floor() as i32).clamp(0, source_h - 1);
        let right = ((self.crop.x + self.crop.w).ceil() as i32).clamp(left + 1, source_w);
        let bottom = ((self.crop.y + self.crop.h).ceil() as i32).clamp(top + 1, source_h);
        let whole = Bitmap::new(self.source.clone());
        let input = if (left, top, right, bottom) == (0, 0, source_w, source_h) {
            Ok(whole.clone())
        } else {
            whole.cropped(left, top, right - left, bottom - top)
        };
        input
            .map(|input| {
                pipeline.apply(input.clone()).unwrap_or_else(|error| {
                    eprintln!("The frame preset failed: {error}");
                    input
                })
            })
            .unwrap_or(whole)
    }
    pub fn geometry(&self) -> (f64, f64, i32, i32) {
        let extra = self.extra_space();
        if let Some(bitmap) = self.rendered() {
            let (w, h) = bitmap.size();
            return (
                bitmap.origin.0 + extra,
                bitmap.origin.1 + extra,
                w + 2 * extra as i32,
                h + 2 * extra as i32,
            );
        }
        let f = &self.frame;
        let scale = self.frame_scale();
        let padding = f.padding.map(|p| (p * scale).round());
        let [l, t, r, b] = match f.mode {
            Mode::Original => [0.; 4],
            Mode::Shadow if f.shadow > 0. => {
                let space = (shadow_space(f) * scale).ceil();
                padding.map(|p| p.max(space))
            }
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
    fn corner_radius(&self) -> f64 {
        if self.frame.mode == Mode::Original || self.frame.pipeline.is_some() {
            0.
        } else {
            self.frame.radius
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
    rounded_rect(c, r.x, r.y, r.w, r.h, radius);
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
    let radius = edit.corner_radius();
    c.save()?;
    clip_outer(c, &edit.frame, w, h);
    if let Some(bitmap) = edit.rendered() {
        let space = edit.extra_space();
        bitmap.paint(c, space, space)?;
    } else {
        paint_built_in(c, image, edit, (x, y, w, h), radius)?;
    }
    paint_marks(c, image, edit, draft, (x, y), radius)?;
    c.restore()?;
    Ok(())
}

fn paint_built_in(
    c: &Context,
    image: &ImageSurface,
    edit: &Edit,
    (x, y, w, h): (f64, f64, i32, i32),
    radius: f64,
) -> Result<()> {
    let f = &edit.frame;
    background(c, f, f64::from(w), f64::from(h))?;
    if f.mode != Mode::Original && f.shadow > 0. {
        let (r, g, b, a) = f.shadow_color;
        let scale = edit.frame_scale();
        for step in (1..=28).rev() {
            let d = f.shadow * scale * 1.65 * step as f64 / 28.;
            round(
                c,
                Rect {
                    x: x + f.shadow_offset.0 * scale - d,
                    y: y + f.shadow_offset.1 * scale - d,
                    w: edit.crop.w + 2. * d,
                    h: edit.crop.h + 2. * d,
                },
                radius + d,
            );
            source(c, (r, g, b, a * (1. - step as f64 / 29.) / 14.));
            c.fill()?;
        }
    }
    c.save()?;
    c.translate(x - edit.crop.x, y - edit.crop.y);
    within_image(c, edit.crop, radius, |c| {
        c.set_source_surface(image, 0., 0.)?;
        c.paint()?;
        Ok(())
    })?;
    c.restore()?;
    Ok(())
}

fn paint_marks(
    c: &Context,
    image: &ImageSurface,
    edit: &Edit,
    draft: Option<&Mark>,
    (x, y): Point,
    radius: f64,
) -> Result<()> {
    let pixels = edit.crop;
    c.save()?;
    c.translate(x - pixels.x, y - pixels.y);
    let marks = || edit.marks.iter().chain(draft);
    for mark in marks().filter(|mark| mark.tool != Tool::Redact) {
        if is_free(mark.tool) {
            draw(c, image, mark)?;
        } else {
            within_image(c, pixels, radius, |c| draw(c, image, mark))?;
        }
    }
    for mark in marks().filter(|mark| mark.tool == Tool::Redact) {
        within_image(c, pixels, radius, |c| draw(c, image, mark))?;
    }
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
    let radius = edit.corner_radius();
    c.save()?;
    clip_outer(c, &edit.frame, w, h);
    c.translate(x - edit.crop.x, y - edit.crop.y);
    if is_free(draft.tool) {
        draw(c, image, draft)?;
    } else {
        within_image(c, edit.crop, radius, |c| draw(c, image, draft))?;
    }
    if draft.tool != Tool::Redact {
        for mark in edit.marks.iter().filter(|mark| mark.tool == Tool::Redact) {
            within_image(c, edit.crop, radius, |c| draw(c, image, mark))?;
        }
    }
    c.restore()?;
    Ok(())
}

pub fn is_free(tool: Tool) -> bool {
    !matches!(tool, Tool::Crop | Tool::Blur | Tool::Redact)
}

fn within_image(
    c: &Context,
    pixels: Rect,
    radius: f64,
    f: impl FnOnce(&Context) -> Result<()>,
) -> Result<()> {
    c.save()?;
    round(c, pixels, radius);
    c.clip();
    let result = f(c);
    c.restore()?;
    result
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

pub fn thumbnail(image: &ImageSurface, edit: &Edit, longest_side: i32) -> Result<ImageSurface> {
    let (_, _, width, height) = edit.geometry();
    let scale = f64::from(longest_side) / f64::from(width.max(height));
    let scaled = |side: i32| ((f64::from(side) * scale).round() as i32).max(1);
    let result = ImageSurface::create(Format::ARgb32, scaled(width), scaled(height))?;
    let c = Context::new(&result)?;
    c.scale(scale, scale);
    paint(&c, image, edit, None)?;
    Ok(result)
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
    fn free_marks_draw_over_the_frame_but_blur_stays_inside() {
        let image = ImageSurface::create(Format::ARgb32, 20, 20).unwrap();
        let mut edit = Edit::new(&image);
        edit.frame.mode = Mode::Gradient;
        edit.frame.adaptive_padding = false;
        edit.frame.padding = [10.; 4];
        edit.marks.push(Mark {
            tool: Tool::Rectangle,
            points: vec![(-8., -8.), (28., 28.)],
            color: (1., 0., 0., 1.),
            width: 2.,
            text: String::new(),
        });
        let mut output =
            ImageSurface::create_from_png(&mut Cursor::new(png(&image, &edit).unwrap())).unwrap();
        let stride = output.stride() as usize;
        let data = output.data().unwrap();
        let at = |x: usize, y: usize| {
            u32::from_ne_bytes(
                data[y * stride + x * 4..y * stride + x * 4 + 4]
                    .try_into()
                    .unwrap(),
            )
        };
        assert_eq!(at(2, 20) & 0xffffff, 0xff0000);
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
