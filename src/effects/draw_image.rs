use super::{
    Assets, Bitmap, Effect,
    draw::fits,
    value::{Offset, Placement, Size},
};
use crate::capture::Result;
use cairo::{Context, Extend, Filter, Format, ImageSurface, Operator};
use serde::Deserialize;
use std::f64::consts::{FRAC_PI_2, PI};

#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize)]
pub enum SizeMode {
    #[default]
    DontResize,
    AbsoluteSize,
    PercentageOfWatermark,
    PercentageOfCanvas,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize)]
pub enum RotateFlip {
    #[default]
    None,
    Rotate90,
    Rotate180,
    Rotate270,
    FlipX,
    Rotate90FlipX,
    FlipY,
    Rotate90FlipY,
}

impl RotateFlip {
    fn parts(self) -> (u8, bool) {
        match self {
            Self::None => (0, false),
            Self::Rotate90 => (1, false),
            Self::Rotate180 => (2, false),
            Self::Rotate270 => (3, false),
            Self::FlipX => (0, true),
            Self::Rotate90FlipX => (1, true),
            Self::FlipY => (2, true),
            Self::Rotate90FlipY => (3, true),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize)]
pub enum Interpolation {
    NearestNeighbor,
    Bilinear,
    HighQualityBilinear,
    Bicubic,
    #[default]
    HighQualityBicubic,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize)]
pub enum Compositing {
    #[default]
    SourceOver,
    SourceCopy,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct DrawImage {
    image_location: String,
    placement: Placement,
    offset: Offset,
    size_mode: SizeMode,
    size: Size,
    rotate_flip: RotateFlip,
    tile: bool,
    auto_hide: bool,
    interpolation_mode: Interpolation,
    compositing_mode: Compositing,
    opacity: i32,
    #[serde(skip)]
    image: Option<ImageSurface>,
}

impl Default for DrawImage {
    fn default() -> Self {
        Self {
            image_location: String::new(),
            placement: Placement::TopLeft,
            offset: Offset::default(),
            size_mode: SizeMode::DontResize,
            size: Size::default(),
            rotate_flip: RotateFlip::None,
            tile: false,
            auto_hide: false,
            interpolation_mode: Interpolation::HighQualityBicubic,
            compositing_mode: Compositing::SourceOver,
            opacity: 100,
            image: None,
        }
    }
}

impl DrawImage {
    pub fn build(value: serde_json::Value, assets: &mut dyn Assets) -> Result<Box<dyn Effect>> {
        let mut effect: Self = serde_json::from_value(value)?;
        effect.image = Some(assets.image(&effect.image_location)?);
        Ok(Box::new(effect))
    }

    fn oriented(&self, source: &ImageSurface) -> Result<ImageSurface> {
        let (turns, mirror) = self.rotate_flip.parts();
        if turns == 0 && !mirror {
            return Ok(source.clone());
        }
        let (w, h) = (f64::from(source.width()), f64::from(source.height()));
        let (out_w, out_h) = if turns % 2 == 1 { (h, w) } else { (w, h) };
        let out = ImageSurface::create(Format::ARgb32, out_w as i32, out_h as i32)?;
        let c = Context::new(&out)?;
        match turns {
            1 => {
                c.translate(h, 0.);
                c.rotate(FRAC_PI_2);
            }
            2 => {
                c.translate(w, h);
                c.rotate(PI);
            }
            3 => {
                c.translate(0., w);
                c.rotate(PI + FRAC_PI_2);
            }
            _ => {}
        }
        if mirror {
            c.translate(w, 0.);
            c.scale(-1., 1.);
        }
        c.set_source_surface(source, 0., 0.)?;
        c.paint()?;
        Ok(out)
    }

    fn target(&self, canvas: (i32, i32), (mark_w, mark_h): (i32, i32)) -> (i32, i32) {
        let percent =
            |percent: i32, of: i32| (f64::from(percent) / 100. * f64::from(of)).round() as i32;
        let keep_ratio = |w: i32, h: i32| match (w, h) {
            (0, h) => (
                (f64::from(h) / f64::from(mark_h) * f64::from(mark_w)).round() as i32,
                h,
            ),
            (w, 0) => (
                w,
                (f64::from(w) / f64::from(mark_w) * f64::from(mark_h)).round() as i32,
            ),
            size => size,
        };
        let Size(w, h) = self.size;
        match self.size_mode {
            SizeMode::DontResize => (mark_w, mark_h),
            SizeMode::AbsoluteSize => keep_ratio(
                if w == -1 { canvas.0 } else { w },
                if h == -1 { canvas.1 } else { h },
            ),
            SizeMode::PercentageOfWatermark => keep_ratio(percent(w, mark_w), percent(h, mark_h)),
            SizeMode::PercentageOfCanvas => keep_ratio(percent(w, canvas.0), percent(h, canvas.1)),
        }
    }
}

impl Effect for DrawImage {
    fn apply(&self, input: Bitmap) -> Result<Bitmap> {
        let Some(source) = &self.image else {
            return Ok(input);
        };
        if self.opacity < 1
            || (self.size_mode != SizeMode::DontResize && self.size.0 <= 0 && self.size.1 <= 0)
        {
            return Ok(input);
        }
        let mark = self.oriented(source)?;
        let canvas = input.size();
        let (w, h) = self.target(canvas, (mark.width(), mark.height()));
        if w < 1 || h < 1 || w > 32_768 || h > 32_768 {
            return Ok(input);
        }
        let (x, y) = self.placement.position(self.offset, canvas, (w, h));
        if self.auto_hide && !fits(canvas, (x, y, w, h)) {
            return Ok(input);
        }
        let out = input.blank_like()?;
        let c = Context::new(&out.surface)?;
        input.paint(&c, 0., 0.)?;
        c.rectangle(f64::from(x), f64::from(y), f64::from(w), f64::from(h));
        c.clip();
        c.translate(f64::from(x), f64::from(y));
        if self.compositing_mode == Compositing::SourceCopy {
            c.set_operator(Operator::Source);
        }
        if self.tile {
            c.set_source_surface(&mark, 0., 0.)?;
            c.source().set_extend(Extend::Repeat);
            c.paint()?;
        } else {
            c.scale(
                f64::from(w) / f64::from(mark.width()),
                f64::from(h) / f64::from(mark.height()),
            );
            c.set_source_surface(&mark, 0., 0.)?;
            let pattern = c.source();
            pattern.set_extend(Extend::Pad);
            pattern.set_filter(match self.interpolation_mode {
                Interpolation::NearestNeighbor => Filter::Nearest,
                Interpolation::Bilinear | Interpolation::HighQualityBilinear => Filter::Bilinear,
                _ => Filter::Best,
            });
            c.paint_with_alpha(f64::from(self.opacity.min(100)) / 100.)?;
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::tests::{BLUE, RED, pixel, solid};

    fn draw(json: &str, mark: Bitmap) -> DrawImage {
        let mut effect: DrawImage = serde_json::from_str(json).unwrap();
        effect.image = Some(mark.surface);
        effect
    }

    #[test]
    fn places_the_image_in_a_corner_with_an_inward_offset() {
        let effect = draw(
            r#"{"Placement":"BottomRight","Offset":"2, 3"}"#,
            solid(4, 4, BLUE),
        );
        let out = effect.apply(solid(20, 20, RED)).unwrap();
        assert_eq!(pixel(&out, 14, 13), 0xff0000ff);
        assert_eq!(pixel(&out, 13, 13), 0xffff0000);
        assert_eq!(pixel(&out, 17, 16), 0xff0000ff);
        assert_eq!(pixel(&out, 18, 16), 0xffff0000);
    }

    #[test]
    fn stretches_to_the_canvas_and_hides_when_it_does_not_fit() {
        let fill = draw(
            r#"{"SizeMode":"AbsoluteSize","Size":"-1, -1"}"#,
            solid(2, 2, BLUE),
        );
        assert_eq!(
            pixel(&fill.apply(solid(8, 8, RED)).unwrap(), 4, 4),
            0xffff0000
        );
        let stretch = draw(
            r#"{"SizeMode":"AbsoluteSize","Size":"-1, 6"}"#,
            solid(2, 2, BLUE),
        );
        let out = stretch.apply(solid(8, 8, RED)).unwrap();
        assert_eq!(pixel(&out, 7, 5), 0xff0000ff);
        assert_eq!(pixel(&out, 7, 6), 0xffff0000);
        let hidden = draw(r#"{"AutoHide":true,"Offset":"-1, 0"}"#, solid(2, 2, BLUE));
        assert_eq!(
            pixel(&hidden.apply(solid(8, 8, RED)).unwrap(), 0, 0),
            0xffff0000
        );
    }

    #[test]
    fn rotation_turns_the_image() {
        let mark = Bitmap::blank(4, 2, (0., 0.)).unwrap();
        let c = Context::new(&mark.surface).unwrap();
        c.set_source_rgb(0., 0., 1.);
        c.rectangle(0., 0., 1., 1.);
        c.fill().unwrap();
        let effect = draw(r#"{"RotateFlip":"Rotate90"}"#, mark);
        let out = effect.apply(solid(8, 8, RED)).unwrap();
        assert_eq!(pixel(&out, 1, 0), 0xff0000ff);
        assert_eq!(pixel(&out, 0, 0), 0xffff0000);
    }
}
