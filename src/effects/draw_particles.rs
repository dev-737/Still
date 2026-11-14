use super::{Assets, Bitmap, Effect, rng::Rng};
use crate::capture::Result;
use cairo::{Context, ImageSurface};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct DrawParticles {
    image_folder: String,
    image_count: i32,
    background: bool,
    random_size: bool,
    random_size_min: i32,
    random_size_max: i32,
    random_angle: bool,
    random_angle_min: i32,
    random_angle_max: i32,
    random_opacity: bool,
    random_opacity_min: i32,
    random_opacity_max: i32,
    no_overlap: bool,
    no_overlap_offset: i32,
    edge_overlap: bool,
    #[serde(skip)]
    images: Vec<ImageSurface>,
}

impl Default for DrawParticles {
    fn default() -> Self {
        Self {
            image_folder: String::new(),
            image_count: 1,
            background: false,
            random_size: false,
            random_size_min: 64,
            random_size_max: 128,
            random_angle: false,
            random_angle_min: 0,
            random_angle_max: 360,
            random_opacity: false,
            random_opacity_min: 0,
            random_opacity_max: 100,
            no_overlap: false,
            no_overlap_offset: 0,
            edge_overlap: false,
            images: vec![],
        }
    }
}

const PLACEMENT_ATTEMPTS_PER_PARTICLE: u32 = 1000;

impl DrawParticles {
    pub fn build(value: serde_json::Value, assets: &mut dyn Assets) -> Result<Box<dyn Effect>> {
        let mut effect: Self = serde_json::from_value(value)?;
        effect.images = assets.folder(&effect.image_folder)?;
        if effect.images.is_empty() {
            return Err("the particle folder has no images".into());
        }
        Ok(Box::new(effect))
    }

    fn size(&self, rng: &mut Rng, image: &ImageSurface) -> (i32, i32) {
        let (w, h) = (image.width(), image.height());
        if !self.random_size {
            return (w, h);
        }
        let (low, high) = (
            self.random_size_min.min(self.random_size_max),
            self.random_size_min.max(self.random_size_max),
        );
        let side = rng.range(low, high + 1).max(1);
        let scaled = |short: i32, long: i32| {
            ((f64::from(side) * f64::from(short)) / f64::from(long))
                .round()
                .max(1.) as i32
        };
        match w.cmp(&h) {
            std::cmp::Ordering::Equal => (side, side),
            std::cmp::Ordering::Greater => (side, scaled(h, w)),
            std::cmp::Ordering::Less => (scaled(w, h), side),
        }
    }

    fn spot(
        &self,
        rng: &mut Rng,
        canvas: (i32, i32),
        (w, h): (i32, i32),
        placed: &[(i32, i32, i32, i32)],
    ) -> Option<(i32, i32)> {
        let overlap = i32::from(self.edge_overlap);
        let span = |canvas: i32, size: i32| {
            let low = if self.edge_overlap { -size + 1 } else { 0 };
            let high = canvas - (1 - overlap) * size - 1;
            (low.min(high), low.max(high))
        };
        let ((x_low, x_high), (y_low, y_high)) = (span(canvas.0, w), span(canvas.1, h));
        let gap = self.no_overlap_offset;
        (0..PLACEMENT_ATTEMPTS_PER_PARTICLE).find_map(|_| {
            let (x, y) = (rng.range(x_low, x_high), rng.range(y_low, y_high));
            let clear = !self.no_overlap
                || placed.iter().all(|&(px, py, pw, ph)| {
                    x - gap >= px + pw
                        || x + w + gap <= px
                        || y - gap >= py + ph
                        || y + h + gap <= py
                });
            clear.then_some((x, y))
        })
    }
}

impl Effect for DrawParticles {
    fn apply(&self, input: Bitmap) -> Result<Bitmap> {
        let out = input.blank_like()?;
        let c = Context::new(&out.surface)?;
        if !self.background {
            input.paint(&c, 0., 0.)?;
        }
        let canvas = input.size();
        let mut rng = Rng::seeded(0x5eed ^ self.images.len() as u64);
        let mut placed = vec![];
        for _ in 0..self.image_count.clamp(1, 1000) {
            let image = &self.images[rng.pick(self.images.len())];
            let (w, h) = self.size(&mut rng, image);
            let angle = if self.random_angle {
                rng.range(
                    self.random_angle_min.min(self.random_angle_max),
                    self.random_angle_min.max(self.random_angle_max) + 1,
                )
            } else {
                0
            };
            let opacity = if self.random_opacity {
                rng.range(
                    self.random_opacity_min.min(self.random_opacity_max),
                    self.random_opacity_min.max(self.random_opacity_max) + 1,
                )
            } else {
                100
            };
            let Some((x, y)) = self.spot(&mut rng, canvas, (w, h), &placed) else {
                continue;
            };
            placed.push((x, y, w, h));
            c.save()?;
            c.translate(
                f64::from(x) + f64::from(w) / 2.,
                f64::from(y) + f64::from(h) / 2.,
            );
            c.rotate(f64::from(angle).to_radians());
            c.scale(
                f64::from(w) / f64::from(image.width()),
                f64::from(h) / f64::from(image.height()),
            );
            c.set_source_surface(
                image,
                -f64::from(image.width()) / 2.,
                -f64::from(image.height()) / 2.,
            )?;
            c.paint_with_alpha(f64::from(opacity.clamp(0, 100)) / 100.)?;
            c.restore()?;
        }
        if self.background {
            input.paint(&c, 0., 0.)?;
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::tests::{BLUE, RED, pixel, solid};

    fn particles(json: &str) -> DrawParticles {
        let mut effect: DrawParticles = serde_json::from_str(json).unwrap();
        effect.images = vec![solid(4, 4, BLUE).surface];
        effect
    }

    fn blue_pixels(bitmap: &Bitmap) -> usize {
        let (w, h) = bitmap.size();
        (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .filter(|&(x, y)| pixel(bitmap, x as usize, y as usize) == 0xff0000ff)
            .count()
    }

    #[test]
    fn scatters_the_requested_number_without_overlap_and_repeatably() {
        let effect = particles(r#"{"ImageCount":10,"NoOverlap":true}"#);
        let a = effect.apply(solid(100, 100, RED)).unwrap();
        let b = effect.apply(solid(100, 100, RED)).unwrap();
        assert_eq!(blue_pixels(&a), 10 * 16);
        assert_eq!(pixel(&a, 0, 0), pixel(&b, 0, 0));
        assert_eq!(blue_pixels(&a), blue_pixels(&b));
    }

    #[test]
    fn background_particles_sit_behind_the_image() {
        let effect = particles(r#"{"ImageCount":20,"Background":true}"#);
        assert_eq!(blue_pixels(&effect.apply(solid(100, 100, RED)).unwrap()), 0);
    }
}
