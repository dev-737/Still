use super::{
    Bitmap, Effect,
    pixels::{Pixels, premultiply, unpremultiply},
    value::Rgba,
};
use crate::capture::Result;
use cairo::{Context, Extend, Format, ImageSurface};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct Pixelate {
    size: i32,
    border_size: i32,
    border_color: Rgba,
}

impl Default for Pixelate {
    fn default() -> Self {
        Self {
            size: 10,
            border_size: 0,
            border_color: Rgba::BLACK,
        }
    }
}

impl Pixelate {
    fn flatten_block(pixels: &mut Pixels, (left, top): (usize, usize), side: usize) {
        let (right, bottom) = (
            (left + side).min(pixels.width),
            (top + side).min(pixels.height),
        );
        let count = ((right - left) * (bottom - top)) as f64;
        let (mut weight, mut colour, mut alpha) = (0., [0.; 3], 0.);
        for y in top..bottom {
            for x in left..right {
                let [a, r, g, b] = unpremultiply(pixels.get(x, y));
                let w = f64::from(a) / 255.;
                weight += w;
                alpha += f64::from(a) * w;
                for (sum, channel) in colour.iter_mut().zip([r, g, b]) {
                    *sum += f64::from(channel) * w;
                }
            }
        }
        let average = |sum: f64| {
            if weight == 0. {
                0
            } else {
                (sum / weight) as u8
            }
        };
        let block = premultiply([
            (alpha / count) as u8,
            average(colour[0]),
            average(colour[1]),
            average(colour[2]),
        ]);
        for y in top..bottom {
            for x in left..right {
                pixels.set(x, y, block);
            }
        }
    }

    fn block_outline(&self, side: i32) -> Result<ImageSurface> {
        let tile = ImageSurface::create(Format::ARgb32, side, side)?;
        let c = Context::new(&tile)?;
        let width = f64::from(self.border_size.min(side));
        self.border_color.set_source(&c);
        c.set_line_width(width);
        c.rectangle(
            width / 2.,
            width / 2.,
            f64::from(side) - width,
            f64::from(side) - width,
        );
        c.stroke()?;
        Ok(tile)
    }
}

impl Effect for Pixelate {
    fn apply(&self, input: Bitmap) -> Result<Bitmap> {
        let side = self.size.max(2);
        let mut pixels = Pixels::from_bitmap(&input)?;
        for top in (0..pixels.height).step_by(side as usize) {
            for left in (0..pixels.width).step_by(side as usize) {
                Self::flatten_block(&mut pixels, (left, top), side as usize);
            }
        }
        let out = Bitmap {
            surface: pixels.into_surface()?,
            origin: input.origin,
        };
        if self.border_size > 0 && self.border_color.is_visible() {
            let c = Context::new(&out.surface)?;
            c.set_source_surface(&self.block_outline(side)?, 0., 0.)?;
            c.source().set_extend(Extend::Repeat);
            c.paint()?;
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::tests::{RED, pixel, solid};

    #[test]
    fn averages_each_block() {
        let image = solid(4, 2, RED);
        let c = Context::new(&image.surface).unwrap();
        c.set_source_rgb(0., 0., 1.);
        c.rectangle(0., 0., 4., 1.);
        c.fill().unwrap();
        let out = serde_json::from_str::<Pixelate>(r#"{"Size":2}"#)
            .unwrap()
            .apply(image)
            .unwrap();
        for (x, y) in [(0, 0), (1, 1), (3, 0)] {
            let value = pixel(&out, x, y);
            assert_eq!(value, pixel(&out, 0, 0));
            assert_eq!(value >> 24, 255);
            assert!((value >> 16 & 0xff).abs_diff(127) <= 1 && (value & 0xff).abs_diff(127) <= 1);
        }
    }

    #[test]
    fn transparent_blocks_stay_clear_and_the_border_is_drawn() {
        let clear = serde_json::from_str::<Pixelate>(r#"{"Size":3}"#)
            .unwrap()
            .apply(solid(6, 6, (0., 0., 0., 0.)))
            .unwrap();
        assert_eq!(pixel(&clear, 4, 4), 0);
        let bordered =
            serde_json::from_str::<Pixelate>(r#"{"Size":4,"BorderSize":1,"BorderColor":"Black"}"#)
                .unwrap()
                .apply(solid(8, 8, RED))
                .unwrap();
        assert_eq!(pixel(&bordered, 0, 0), 0xff000000);
        assert_eq!(pixel(&bordered, 2, 2), 0xffff0000);
        assert_eq!(pixel(&bordered, 7, 5), 0xff000000);
    }
}
