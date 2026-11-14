use super::{
    Bitmap,
    pixels::{Pixels, box_blur},
    value::{Fill, Offset},
};
use crate::capture::Result;
use cairo::Context;

pub fn silhouette(input: &Bitmap, size: i32) -> Result<Pixels> {
    let size = size.max(0) as usize;
    let mut mask = Pixels::from_bitmap(input)?.padded(size);
    mask.scale_alpha(1.);
    box_blur(&mut mask, size);
    Ok(mask)
}

pub fn compose(
    input: Bitmap,
    mask: Pixels,
    fill: &Fill,
    size: i32,
    offset: Offset,
    grow: bool,
) -> Result<Bitmap> {
    let size = size.max(0);
    let Offset(ox, oy) = offset;
    let (w, h) = input.size();
    let (out, image_at, mask_at) = if grow {
        let image_at = (size + (-ox).max(0), size + (-oy).max(0));
        let out = Bitmap::blank(
            w + 2 * size + ox.abs(),
            h + 2 * size + oy.abs(),
            (
                input.origin.0 + f64::from(image_at.0),
                input.origin.1 + f64::from(image_at.1),
            ),
        )?;
        (out, image_at, (ox.max(0), oy.max(0)))
    } else {
        (input.blank_like()?, (0, 0), (ox - size, oy - size))
    };
    let c = Context::new(&out.surface)?;
    let (mask_w, mask_h) = (mask.width as f64, mask.height as f64);
    let mask = mask.into_surface()?;
    c.save()?;
    c.translate(f64::from(mask_at.0), f64::from(mask_at.1));
    fill.set_source(&c, (0., 0., mask_w, mask_h))?;
    c.mask_surface(&mask, 0., 0.)?;
    c.restore()?;
    input.paint(&c, f64::from(image_at.0), f64::from(image_at.1))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::{
        tests::{RED, pixel, solid},
        value::Rgba,
    };

    #[test]
    fn the_silhouette_spreads_past_the_image_edge() {
        let mask = silhouette(&solid(10, 10, RED), 4).unwrap();
        assert_eq!((mask.width, mask.height), (18, 18));
        let edge = mask.get(3, 9) >> 24;
        assert!(edge > 0 && edge < 255);
        assert_eq!(mask.get(9, 9) >> 24, 255);
    }

    #[test]
    fn the_canvas_grows_around_the_image_and_offsets_the_halo() {
        let mask = silhouette(&solid(4, 4, RED), 2).unwrap();
        let fill = Fill::Solid(Rgba::BLACK);
        let out = compose(solid(4, 4, RED), mask, &fill, 2, Offset(0, 3), true).unwrap();
        assert_eq!(out.size(), (8, 11));
        assert_eq!(out.origin, (2., 2.));
        assert_eq!(pixel(&out, 3, 3), 0xffff0000);
        assert_eq!(pixel(&out, 3, 0), 0);
        assert!(pixel(&out, 3, 9) >> 24 > 0);
    }
}
