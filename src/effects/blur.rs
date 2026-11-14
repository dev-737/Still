use super::{
    Bitmap, Effect,
    pixels::{Pixels, box_blur},
};
use crate::capture::Result;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct Blur {
    radius: i32,
}

impl Default for Blur {
    fn default() -> Self {
        Self { radius: 15 }
    }
}

impl Effect for Blur {
    fn apply(&self, input: Bitmap) -> Result<Bitmap> {
        let radius = self.radius.max(3) as usize | 1;
        let mut pixels = Pixels::from_bitmap(&input)?;
        box_blur(&mut pixels, radius);
        Ok(Bitmap {
            surface: pixels.into_surface()?,
            origin: input.origin,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::tests::{RED, pixel, solid};
    use cairo::Context;

    #[test]
    fn keeps_flat_images_and_softens_edges() {
        let blur: Blur = serde_json::from_str(r#"{"Radius":5}"#).unwrap();
        assert_eq!(
            pixel(&blur.apply(solid(9, 9, RED)).unwrap(), 4, 4),
            0xffff0000
        );
        let split = solid(20, 4, RED);
        let c = Context::new(&split.surface).unwrap();
        c.set_source_rgb(0., 0., 1.);
        c.rectangle(10., 0., 10., 4.);
        c.fill().unwrap();
        let out = blur.apply(split).unwrap();
        let seam = pixel(&out, 10, 2);
        assert_ne!(seam, 0xffff0000);
        assert_ne!(seam, 0xff0000ff);
        assert_eq!(pixel(&out, 0, 2), 0xffff0000);
    }
}
