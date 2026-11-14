use super::{
    Bitmap, Effect,
    draw::fits,
    value::{Fill, FontSpec, Gradient, GradientKind, Offset, Placement, Rgba},
};
use crate::capture::Result;
use cairo::{Context, Format, ImageSurface, LineJoin};
use serde::Deserialize;
use std::f64::consts::TAU;

const CIRCLE_GLYPH: &str = "\u{e91f}";
const ICON_FONT: &str = "Segoe MDL2 Assets";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct DrawTextEx {
    text: String,
    placement: Placement,
    offset: Offset,
    angle: f64,
    auto_hide: bool,
    font: FontSpec,
    color: Rgba,
    use_gradient: bool,
    gradient: Gradient,
    outline: bool,
    outline_size: i32,
    outline_color: Rgba,
    outline_use_gradient: bool,
    outline_gradient: Gradient,
    shadow: bool,
    shadow_offset: Offset,
    shadow_color: Rgba,
    shadow_use_gradient: bool,
    shadow_gradient: Gradient,
}

impl Default for DrawTextEx {
    fn default() -> Self {
        let gradient = Gradient::teal(GradientKind::Horizontal);
        Self {
            text: "Text".to_owned(),
            placement: Placement::TopLeft,
            offset: Offset::default(),
            angle: 0.,
            auto_hide: false,
            font: FontSpec::points("Arial", 36.),
            color: Rgba::rgb(235, 235, 235),
            use_gradient: false,
            gradient: gradient.clone(),
            outline: false,
            outline_size: 5,
            outline_color: Rgba::rgb(235, 0, 0),
            outline_use_gradient: false,
            outline_gradient: gradient.clone(),
            shadow: false,
            shadow_offset: Offset(0, 5),
            shadow_color: "125, 0, 0, 0".parse().expect("valid colour"),
            shadow_use_gradient: false,
            shadow_gradient: gradient,
        }
    }
}

impl DrawTextEx {
    fn trace(&self, c: &Context, text: &str) {
        c.save().ok();
        c.rotate(self.angle.to_radians());
        if self.font.family == ICON_FONT && text == CIRCLE_GLYPH {
            let radius = self.font.pixels / 2.;
            c.new_sub_path();
            c.arc(radius, radius, radius, 0., TAU);
        } else {
            c.move_to(0., 0.);
            pangocairo::functions::layout_path(c, &self.font.layout(c, text));
        }
        c.restore().ok();
    }
}

impl Effect for DrawTextEx {
    fn apply(&self, input: Bitmap) -> Result<Bitmap> {
        let canvas = input.size();
        let text = super::macros::expand(&self.text, canvas);
        if text.is_empty() || self.font.pixels < 1. {
            return Ok(input);
        }
        let scratch = Context::new(&ImageSurface::create(Format::ARgb32, 1, 1)?)?;
        self.trace(&scratch, &text);
        let (left, top, right, bottom) = scratch.path_extents()?;
        if right <= left || bottom <= top {
            return Ok(input);
        }
        let size = (
            ((right - left).ceil() as i32) + 1,
            ((bottom - top).ceil() as i32) + 1,
        );
        let (x, y) = self.placement.position(self.offset, canvas, size);
        if self.auto_hide && !fits(canvas, (x, y, size.0, size.1)) {
            return Ok(input);
        }
        let out = input.blank_like()?;
        let c = Context::new(&out.surface)?;
        input.paint(&c, 0., 0.)?;
        c.save()?;
        c.translate(f64::from(x) - left, f64::from(y) - top);
        self.trace(&c, &text);
        c.restore()?;
        let shape = c.copy_path()?;
        c.new_path();
        let area = (
            f64::from(x),
            f64::from(y),
            f64::from(size.0),
            f64::from(size.1),
        );
        let stroke = f64::from(self.outline_size.max(0));
        c.set_line_join(LineJoin::Round);

        let shadow = Fill::new(
            self.shadow_use_gradient,
            self.shadow_color,
            &self.shadow_gradient,
        );
        if self.shadow && shadow.is_visible() {
            c.save()?;
            c.translate(
                f64::from(self.shadow_offset.0),
                f64::from(self.shadow_offset.1),
            );
            c.append_path(&shape);
            shadow.set_source(&c, area)?;
            if self.outline {
                c.set_line_width(stroke);
                c.stroke()?;
            } else {
                c.fill()?;
            }
            c.restore()?;
        }
        let outline = Fill::new(
            self.outline_use_gradient,
            self.outline_color,
            &self.outline_gradient,
        );
        if self.outline && stroke > 0. && outline.is_visible() {
            c.append_path(&shape);
            outline.set_source(&c, area)?;
            c.set_line_width(stroke);
            c.stroke()?;
        }
        let fill = Fill::new(self.use_gradient, self.color, &self.gradient);
        if fill.is_visible() {
            c.append_path(&shape);
            fill.set_source(&c, area)?;
            c.fill()?;
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::tests::{RED, pixel, solid};

    #[test]
    fn the_icon_glyph_becomes_a_filled_circle() {
        let dot: DrawTextEx = serde_json::from_str(
            r#"{"Text":"","Font":"Segoe MDL2 Assets, 15pt","Color":"Blue",
                "Placement":"TopLeft","Offset":"4, 4"}"#,
        )
        .unwrap();
        let out = dot.apply(solid(60, 60, RED)).unwrap();
        assert_eq!(pixel(&out, 14, 14), 0xff0000ff);
        assert_eq!(pixel(&out, 4, 4), 0xffff0000);
        assert_eq!(pixel(&out, 50, 50), 0xffff0000);
    }

    #[test]
    fn an_outline_surrounds_the_fill() {
        let dot: DrawTextEx = serde_json::from_str(
            r#"{"Text":"","Font":"Segoe MDL2 Assets, 15pt","Color":"Blue","Outline":true,
                "OutlineSize":4,"OutlineColor":"Black","Placement":"TopLeft","Offset":"10, 10"}"#,
        )
        .unwrap();
        let out = dot.apply(solid(60, 60, RED)).unwrap();
        assert_eq!(pixel(&out, 20, 20), 0xff0000ff);
        assert_eq!(pixel(&out, 20, 9), 0xff000000);
    }
}
