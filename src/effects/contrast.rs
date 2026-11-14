use super::{Bitmap, Effect, pixels::Pixels};
use crate::capture::Result;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct Contrast {
    value: f64,
}

impl Default for Contrast {
    fn default() -> Self {
        Self { value: 1. }
    }
}

impl Effect for Contrast {
    fn apply(&self, input: Bitmap) -> Result<Bitmap> {
        let factor = if self.value.is_finite() {
            self.value.max(0.)
        } else {
            1.
        };
        let mut pixels = Pixels::from_bitmap(&input)?;
        pixels.map(|pixel| {
            let alpha = pixel >> 24;
            let scale = |shift: u32| {
                let channel = ((pixel >> shift & 0xff) as f64 * factor).round() as u32;
                channel.min(alpha) << shift
            };
            alpha << 24 | scale(16) | scale(8) | scale(0)
        });
        Ok(Bitmap {
            surface: pixels.into_surface()?,
            origin: input.origin,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::tests::{pixel, solid};

    fn run(value: &str, colour: (f64, f64, f64, f64)) -> u32 {
        let effect: Contrast = serde_json::from_str(&format!(r#"{{"Value":{value}}}"#)).unwrap();
        pixel(&effect.apply(solid(2, 2, colour)).unwrap(), 0, 0)
    }

    #[test]
    fn scales_colour_and_clamps() {
        let grey = (100. / 255., 100. / 255., 100. / 255., 1.);
        assert_eq!(run("1", grey), 0xff646464);
        assert_eq!(run("1.3", grey), 0xff828282);
        assert_eq!(run("3", grey), 0xffffffff);
        assert_eq!(run("0", grey), 0xff000000);
    }

    #[test]
    fn leaves_transparency_alone() {
        assert_eq!(run("2", (0., 0., 0., 0.)), 0);
    }
}
