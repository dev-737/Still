use super::{Bitmap, Effect, value::Margin};
use crate::capture::Result;
use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct Crop {
    margin: Margin,
}

impl Effect for Crop {
    fn apply(&self, input: Bitmap) -> Result<Bitmap> {
        let m = self.margin;
        let (w, h) = input.size();
        if m.is_zero() || m.has_negative() || m.horizontal() >= w || m.vertical() >= h {
            return Ok(input);
        }
        input.cropped(m.left, m.top, w - m.horizontal(), h - m.vertical())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::tests::{RED, solid};

    #[test]
    fn trims_margins_and_moves_the_origin() {
        let crop: Crop = serde_json::from_str(r#"{"Margin":"1, 2, 3, 4"}"#).unwrap();
        let out = crop.apply(solid(10, 10, RED)).unwrap();
        assert_eq!(out.size(), (6, 4));
        assert_eq!(out.origin, (-1., -2.));
    }

    #[test]
    fn negative_or_excessive_margins_do_nothing() {
        for json in [r#"{"Margin":"-1, 0, 0, 0"}"#, r#"{"Margin":"6, 0, 6, 0"}"#] {
            let crop: Crop = serde_json::from_str(json).unwrap();
            assert_eq!(crop.apply(solid(10, 10, RED)).unwrap().size(), (10, 10));
        }
    }
}
