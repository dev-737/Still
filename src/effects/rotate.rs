use super::{Bitmap, Effect};
use crate::capture::Result;
use cairo::Context;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct Rotate {
    angle: f64,
    upsize: bool,
    clip: bool,
}

impl Default for Rotate {
    fn default() -> Self {
        Self {
            angle: 0.,
            upsize: true,
            clip: false,
        }
    }
}

impl Effect for Rotate {
    fn apply(&self, input: Bitmap) -> Result<Bitmap> {
        if !self.angle.is_finite() || self.angle % 360. == 0. {
            return Ok(input);
        }
        let (w, h) = input.size();
        let (w, h) = (f64::from(w), f64::from(h));
        let radians = self.angle.to_radians();
        let (sin, cos) = (radians.sin().abs(), radians.cos().abs());
        let (fit_w, fit_h) = ((w * cos + h * sin).round(), (w * sin + h * cos).round());
        let (out_w, out_h, scale) = if self.upsize {
            (fit_w, fit_h, 1.)
        } else if self.clip {
            (w, h, 1.)
        } else {
            (w, h, (w / fit_w).min(h / fit_h))
        };
        let mut out = Bitmap::blank(out_w as i32, out_h as i32, (0., 0.))?;
        let c = Context::new(&out.surface)?;
        c.translate(out_w / 2., out_h / 2.);
        c.scale(scale, scale);
        c.rotate(radians);
        c.translate(-w / 2., -h / 2.);
        out.origin = c.user_to_device(input.origin.0, input.origin.1);
        input.paint(&c, 0., 0.)?;
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::tests::{RED, solid};

    #[test]
    fn quarter_turns_swap_the_sides() {
        let rotate: Rotate = serde_json::from_str(r#"{"Angle":90}"#).unwrap();
        assert_eq!(rotate.apply(solid(30, 10, RED)).unwrap().size(), (10, 30));
    }

    #[test]
    fn without_upsize_the_canvas_keeps_its_size() {
        let rotate: Rotate = serde_json::from_str(r#"{"Angle":45,"Upsize":false}"#).unwrap();
        assert_eq!(rotate.apply(solid(30, 20, RED)).unwrap().size(), (30, 20));
        assert_eq!(
            Rotate::default().apply(solid(7, 7, RED)).unwrap().size(),
            (7, 7)
        );
    }
}
