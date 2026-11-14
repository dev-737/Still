use super::{Bitmap, Effect, value::Rgba};
use crate::capture::Result;
use cairo::Context;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct DrawCheckerboard {
    size: i32,
    color: Rgba,
    color2: Rgba,
}

impl Default for DrawCheckerboard {
    fn default() -> Self {
        Self {
            size: 10,
            color: "LightGray".parse().expect("known colour"),
            color2: Rgba::WHITE,
        }
    }
}

impl Effect for DrawCheckerboard {
    fn apply(&self, input: Bitmap) -> Result<Bitmap> {
        let out = input.blank_like()?;
        let (w, h) = out.size();
        let square = self.size.max(1);
        let c = Context::new(&out.surface)?;
        for (index, y) in (0..h).step_by(square as usize).enumerate() {
            for (column, x) in (0..w).step_by(square as usize).enumerate() {
                if (index + column) % 2 == 0 {
                    self.color.set_source(&c);
                } else {
                    self.color2.set_source(&c);
                }
                c.rectangle(
                    f64::from(x),
                    f64::from(y),
                    f64::from(square),
                    f64::from(square),
                );
                c.fill()?;
            }
        }
        input.paint(&c, 0., 0.)?;
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::tests::{pixel, solid};

    #[test]
    fn alternates_the_two_colours() {
        let board: DrawCheckerboard =
            serde_json::from_str(r#"{"Size":4,"Color":"Black","Color2":"White"}"#).unwrap();
        let out = board.apply(solid(16, 16, (0., 0., 0., 0.))).unwrap();
        assert_eq!(pixel(&out, 1, 1), 0xff000000);
        assert_eq!(pixel(&out, 5, 1), 0xffffffff);
        assert_eq!(pixel(&out, 1, 5), 0xffffffff);
        assert_eq!(pixel(&out, 5, 5), 0xff000000);
    }
}
