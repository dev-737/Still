use super::{Bitmap, Effect, draw::rounded_rect};
use crate::capture::Result;
use cairo::Context;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct RoundedCorners {
    corner_radius: f64,
}

impl Default for RoundedCorners {
    fn default() -> Self {
        Self { corner_radius: 20. }
    }
}

impl Effect for RoundedCorners {
    fn apply(&self, input: Bitmap) -> Result<Bitmap> {
        let out = input.blank_like()?;
        let (w, h) = out.size();
        let c = Context::new(&out.surface)?;
        rounded_rect(
            &c,
            0.,
            0.,
            f64::from(w),
            f64::from(h),
            self.corner_radius.max(0.),
        );
        c.clip();
        input.paint(&c, 0., 0.)?;
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::tests::{RED, pixel, solid};

    #[test]
    fn clears_the_corners_only() {
        let rounded = RoundedCorners { corner_radius: 8. };
        let out = rounded.apply(solid(40, 40, RED)).unwrap();
        assert_eq!(pixel(&out, 0, 0), 0);
        assert_eq!(pixel(&out, 20, 20), 0xffff0000);
        assert_eq!(pixel(&out, 20, 0), 0xffff0000);
    }
}
