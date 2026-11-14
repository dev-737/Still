use super::{Bitmap, Effect, pixels::Pixels, value::Sides};
use crate::capture::Result;
use cairo::Context;
use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct AutoCrop {
    sides: Sides,
    padding: i32,
}

impl AutoCrop {
    fn content(&self, pixels: &Pixels) -> Option<(usize, usize, usize, usize)> {
        let (w, h) = (pixels.width, pixels.height);
        let reference = pixels.get(0, 0);
        let mask = if reference >> 24 == 0 {
            0xff00_0000
        } else {
            u32::MAX
        };
        let differs = |x: usize, y: usize| pixels.get(x, y) & mask != reference & mask;
        let column = |x: usize| (0..h).any(|y| differs(x, y));
        let (mut x, mut y, mut width, mut height) = (0, 0, w, h);
        if self.sides.left {
            x = (0..w).find(|&x| column(x))?;
            width = w - x;
        }
        if self.sides.top {
            y = (0..h).find(|&y| (0..w).any(|x| differs(x, y)))?;
            height = h - y;
        }
        if self.sides.right {
            width = (0..w).rev().find(|&x| column(x))? + 1 - x;
        }
        if self.sides.bottom {
            height = (0..h).rev().find(|&y| (0..w).any(|x| differs(x, y)))? + 1 - y;
        }
        Some((x, y, width, height))
    }
}

impl Effect for AutoCrop {
    fn apply(&self, input: Bitmap) -> Result<Bitmap> {
        let pixels = Pixels::from_bitmap(&input)?;
        let Some((x, y, w, h)) = self.content(&pixels) else {
            return Ok(input);
        };
        if (x, y, w, h) == (0, 0, pixels.width, pixels.height) {
            return Ok(input);
        }
        let cropped = input.cropped(x as i32, y as i32, w as i32, h as i32)?;
        let pad = self.padding.max(0);
        if pad == 0 {
            return Ok(cropped);
        }
        let out = Bitmap::blank(
            w as i32 + 2 * pad,
            h as i32 + 2 * pad,
            (
                cropped.origin.0 + f64::from(pad),
                cropped.origin.1 + f64::from(pad),
            ),
        )?;
        let c = Context::new(&out.surface)?;
        let [a, r, g, b] = super::pixels::unpremultiply(pixels.get(0, 0));
        c.set_source_rgba(
            f64::from(r) / 255.,
            f64::from(g) / 255.,
            f64::from(b) / 255.,
            f64::from(a) / 255.,
        );
        c.paint()?;
        cropped.paint(&c, f64::from(pad), f64::from(pad))?;
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::tests::{RED, solid};

    #[test]
    fn crops_to_the_content_and_pads() {
        let image = Bitmap::blank(30, 20, (0., 0.)).unwrap();
        let c = Context::new(&image.surface).unwrap();
        c.set_source_rgb(1., 0., 0.);
        c.rectangle(8., 5., 10., 6.);
        c.fill().unwrap();
        let out = AutoCrop::default().apply(image.clone()).unwrap();
        assert_eq!(out.size(), (10, 6));
        let padded: AutoCrop = serde_json::from_str(r#"{"Padding":3}"#).unwrap();
        assert_eq!(padded.apply(image).unwrap().size(), (16, 12));
    }

    #[test]
    fn leaves_uniform_images_alone() {
        let out = AutoCrop::default().apply(solid(8, 8, RED)).unwrap();
        assert_eq!(out.size(), (8, 8));
    }
}
