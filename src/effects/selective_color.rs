use super::{
    Bitmap, Effect,
    pixels::{Pixels, premultiply, unpremultiply},
    value::Rgba,
};
use crate::capture::Result;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct SelectiveColor {
    light_color: Rgba,
    dark_color: Rgba,
    palette_size: i32,
}

impl Default for SelectiveColor {
    fn default() -> Self {
        Self {
            light_color: Rgba::WHITE,
            dark_color: Rgba::BLACK,
            palette_size: 10,
        }
    }
}

fn brightness([r, g, b]: [u8; 3]) -> i32 {
    let (r, g, b) = (f64::from(r), f64::from(g), f64::from(b));
    (0.299 * r * r + 0.587 * g * g + 0.114 * b * b).sqrt() as i32
}

impl SelectiveColor {
    fn palette(&self) -> Vec<(i32, [u8; 3])> {
        let steps = self.palette_size.clamp(2, 100);
        let channel = |from: f64, to: f64, t: f64| (from + (to - from) * t) as u8;
        let mut palette: Vec<(i32, [u8; 3])> = vec![];
        for step in 0..steps {
            let t = f64::from(step) / f64::from(steps - 1);
            let (light, dark) = (self.light_color, self.dark_color);
            let shade = [
                channel(light.r * 255., dark.r * 255., t),
                channel(light.g * 255., dark.g * 255., t),
                channel(light.b * 255., dark.b * 255., t),
            ];
            let key = brightness(shade);
            if palette.iter().all(|&(known, _)| known != key) {
                palette.push((key, shade));
            }
        }
        palette
    }
}

impl Effect for SelectiveColor {
    fn apply(&self, input: Bitmap) -> Result<Bitmap> {
        let palette = self.palette();
        let nearest: Vec<[u8; 3]> = (0..=255)
            .map(|level| {
                palette
                    .iter()
                    .min_by_key(|&&(key, _)| ((key - level).abs(), key))
                    .map_or([0; 3], |&(_, shade)| shade)
            })
            .collect();
        let mut pixels = Pixels::from_bitmap(&input)?;
        pixels.map(|pixel| {
            let [a, r, g, b] = unpremultiply(pixel);
            let [r, g, b] = nearest[brightness([r, g, b]).clamp(0, 255) as usize];
            premultiply([a, r, g, b])
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

    #[test]
    fn maps_brightness_onto_the_palette() {
        let effect: SelectiveColor = serde_json::from_str(
            r#"{"LightColor":"255, 129, 1","DarkColor":"51, 51, 51","PaletteSize":2}"#,
        )
        .unwrap();
        let white = effect.apply(solid(2, 2, (1., 1., 1., 1.))).unwrap();
        assert_eq!(pixel(&white, 0, 0), 0xffff8101);
        let black = effect.apply(solid(2, 2, (0., 0., 0., 1.))).unwrap();
        assert_eq!(pixel(&black, 0, 0), 0xff333333);
    }

    #[test]
    fn keeps_transparency() {
        let effect: SelectiveColor = serde_json::from_str(r#"{"LightColor":"Red"}"#).unwrap();
        assert_eq!(
            pixel(&effect.apply(solid(2, 2, (0., 0., 0., 0.))).unwrap(), 0, 0),
            0
        );
    }
}
