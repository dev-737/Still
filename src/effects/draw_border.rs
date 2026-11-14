use super::{
    Bitmap, Effect,
    value::{Fill, Gradient, Rgba},
};
use crate::capture::Result;
use cairo::{Context, LineJoin};
use serde::Deserialize;

#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize)]
pub enum BorderType {
    Inside,
    #[default]
    Outside,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize)]
pub enum DashStyle {
    #[default]
    Solid,
    Dash,
    Dot,
    DashDot,
    DashDotDot,
}

impl DashStyle {
    fn pattern(self) -> &'static [f64] {
        match self {
            Self::Solid => &[],
            Self::Dash => &[3., 1.],
            Self::Dot => &[1., 1.],
            Self::DashDot => &[3., 1., 1., 1.],
            Self::DashDotDot => &[3., 1., 1., 1., 1., 1.],
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct DrawBorder {
    #[serde(rename = "Type")]
    kind: BorderType,
    size: i32,
    dash_style: DashStyle,
    color: Rgba,
    use_gradient: bool,
    gradient: Gradient,
}

impl Default for DrawBorder {
    fn default() -> Self {
        Self {
            kind: BorderType::Outside,
            size: 1,
            dash_style: DashStyle::Solid,
            color: Rgba::BLACK,
            use_gradient: false,
            gradient: Gradient::default(),
        }
    }
}

impl Effect for DrawBorder {
    fn apply(&self, input: Bitmap) -> Result<Bitmap> {
        let width = self.size.max(1);
        let grow = if self.kind == BorderType::Outside {
            width
        } else {
            0
        };
        let (w, h) = input.size();
        let out = Bitmap::blank(
            w + 2 * grow,
            h + 2 * grow,
            (
                input.origin.0 + f64::from(grow),
                input.origin.1 + f64::from(grow),
            ),
        )?;
        let (out_w, out_h) = (f64::from(out.size().0), f64::from(out.size().1));
        let c = Context::new(&out.surface)?;
        if self.kind == BorderType::Inside {
            input.paint(&c, 0., 0.)?;
        }
        let line = f64::from(width);
        Fill::new(self.use_gradient, self.color, &self.gradient)
            .set_source(&c, (0., 0., out_w, out_h))?;
        c.set_line_width(line);
        c.set_line_join(LineJoin::Round);
        let dashes: Vec<f64> = self.dash_style.pattern().iter().map(|d| d * line).collect();
        c.set_dash(&dashes, 0.);
        c.rectangle(line / 2., line / 2., out_w - line, out_h - line);
        c.stroke()?;
        if self.kind == BorderType::Outside {
            input.paint(&c, f64::from(grow), f64::from(grow))?;
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::tests::{BLUE, RED, pixel, solid};

    fn border(json: &str) -> DrawBorder {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn outside_borders_grow_the_canvas() {
        let out = border(r#"{"Type":"Outside","Size":3,"Color":"Blue"}"#)
            .apply(solid(10, 10, RED))
            .unwrap();
        assert_eq!(out.size(), (16, 16));
        assert_eq!(out.origin, (3., 3.));
        assert_eq!(pixel(&out, 1, 8), 0xff0000ff);
        assert_eq!(pixel(&out, 8, 8), 0xffff0000);
    }

    #[test]
    fn inside_borders_cover_the_edge_pixels() {
        let out = border(r#"{"Type":"Inside","Size":2,"Color":"Red"}"#)
            .apply(solid(10, 10, BLUE))
            .unwrap();
        assert_eq!(out.size(), (10, 10));
        assert_eq!(pixel(&out, 1, 5), 0xffff0000);
        assert_eq!(pixel(&out, 5, 5), 0xff0000ff);
    }
}
