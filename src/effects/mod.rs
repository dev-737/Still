mod auto_crop;
mod blur;
mod canvas;
mod contrast;
mod crop;
mod draw;
mod draw_background;
mod draw_border;
mod draw_checkerboard;
mod draw_image;
mod draw_particles;
mod draw_text;
mod draw_text_ex;
mod glow;
mod halo;
mod macros;
mod pixelate;
mod pixels;
mod registry;
mod rng;
mod rotate;
mod rounded_corners;
mod selective_color;
mod shadow;
mod torn_edge;
mod typography;
mod value;

pub use draw::rounded_rect;
pub use registry::{BuildError, build};
pub use typography::install_bundled_fonts;

use crate::capture::Result;
use cairo::{Context, Format, ImageSurface};
use std::rc::Rc;

pub type Point = (f64, f64);

const MAX_SIDE: i32 = 16_384;
const MAX_PIXELS: i64 = 100_000_000;

#[derive(Clone)]
pub struct Bitmap {
    pub surface: ImageSurface,
    pub origin: Point,
}

impl Bitmap {
    pub fn new(surface: ImageSurface) -> Self {
        Self {
            surface,
            origin: (0., 0.),
        }
    }
    pub fn blank(width: i32, height: i32, origin: Point) -> Result<Self> {
        if width < 1
            || height < 1
            || width > MAX_SIDE
            || height > MAX_SIDE
            || i64::from(width) * i64::from(height) > MAX_PIXELS
        {
            return Err(format!("An effect would make a {width}×{height} image.").into());
        }
        Ok(Self {
            surface: ImageSurface::create(Format::ARgb32, width, height)?,
            origin,
        })
    }
    pub fn size(&self) -> (i32, i32) {
        (self.surface.width(), self.surface.height())
    }
    pub fn blank_like(&self) -> Result<Self> {
        let (w, h) = self.size();
        Self::blank(w, h, self.origin)
    }
    pub fn paint(&self, c: &Context, x: f64, y: f64) -> Result<()> {
        c.set_source_surface(&self.surface, x, y)?;
        c.paint()?;
        Ok(())
    }
    pub fn cropped(&self, x: i32, y: i32, width: i32, height: i32) -> Result<Self> {
        let out = Self::blank(
            width,
            height,
            (self.origin.0 - f64::from(x), self.origin.1 - f64::from(y)),
        )?;
        self.paint(&Context::new(&out.surface)?, -f64::from(x), -f64::from(y))?;
        Ok(out)
    }
}

pub trait Effect {
    fn apply(&self, input: Bitmap) -> Result<Bitmap>;
}

pub trait Assets {
    fn image(&mut self, location: &str) -> Result<ImageSurface>;
    fn folder(&mut self, location: &str) -> Result<Vec<ImageSurface>>;
}

#[derive(Clone)]
pub struct Pipeline(Rc<[Box<dyn Effect>]>);

impl Pipeline {
    pub fn new(effects: Vec<Box<dyn Effect>>) -> Self {
        Self(effects.into())
    }
    pub fn same(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl Effect for Pipeline {
    fn apply(&self, input: Bitmap) -> Result<Bitmap> {
        self.0
            .iter()
            .try_fold(input, |bitmap, effect| effect.apply(bitmap))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn solid(width: i32, height: i32, (r, g, b, a): (f64, f64, f64, f64)) -> Bitmap {
        let bitmap = Bitmap::blank(width, height, (0., 0.)).unwrap();
        let c = Context::new(&bitmap.surface).unwrap();
        c.set_source_rgba(r, g, b, a);
        c.paint().unwrap();
        bitmap
    }

    pub fn pixel(bitmap: &Bitmap, x: usize, y: usize) -> u32 {
        let mut value = 0;
        bitmap
            .surface
            .with_data(|data| {
                let stride = bitmap.surface.stride() as usize;
                let at = y * stride + x * 4;
                value = u32::from_ne_bytes(data[at..at + 4].try_into().unwrap());
            })
            .unwrap();
        value
    }

    pub const RED: (f64, f64, f64, f64) = (1., 0., 0., 1.);
    pub const BLUE: (f64, f64, f64, f64) = (0., 0., 1., 1.);

    #[test]
    fn pipeline_runs_effects_in_order_and_tracks_the_origin() {
        let effects = registry::build_all(
            r#"[{"$type":"Canvas","Margin":"10, 20, 10, 20","Color":"Blue"},
                {"$type":"Crop","Margin":"5, 5, 5, 5"}]"#,
        );
        let out = effects.apply(solid(40, 30, RED)).unwrap();
        assert_eq!(out.size(), (50, 60));
        assert_eq!(out.origin, (5., 15.));
        assert_eq!(pixel(&out, 0, 0), 0xff0000ff);
        assert_eq!(pixel(&out, 10, 20), 0xffff0000);
    }
}
