use super::{
    Bitmap, Effect,
    halo::{compose, silhouette},
    value::{Fill, Offset, Rgba},
};
use crate::capture::Result;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct Shadow {
    opacity: f64,
    size: i32,
    darkness: f64,
    color: Rgba,
    offset: Offset,
    auto_resize: bool,
}

impl Default for Shadow {
    fn default() -> Self {
        Self {
            opacity: 0.6,
            size: 10,
            darkness: 0.,
            color: Rgba::BLACK,
            offset: Offset::default(),
            auto_resize: true,
        }
    }
}

impl Effect for Shadow {
    fn apply(&self, input: Bitmap) -> Result<Bitmap> {
        let size = self.size.max(0);
        let mut mask = silhouette(&input, size)?;
        let visibility = self.opacity.clamp(0.1, 1.) * self.color.a;
        let darkening = self.darkness.max(0.) + 1.;
        mask.scale_alpha(visibility);
        mask.scale_alpha(darkening);
        compose(
            input,
            mask,
            &Fill::Solid(Rgba {
                a: 1.,
                ..self.color
            }),
            size,
            self.offset,
            self.auto_resize,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::tests::{RED, pixel, solid};

    fn shadow(json: &str) -> Shadow {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn grows_the_canvas_and_moves_the_origin() {
        let out = shadow(r#"{"Size":5,"Offset":"0, 10"}"#)
            .apply(solid(20, 20, RED))
            .unwrap();
        assert_eq!(out.size(), (30, 40));
        assert_eq!(out.origin, (5., 5.));
        assert_eq!(pixel(&out, 15, 15), 0xffff0000);
    }

    #[test]
    fn a_negative_offset_moves_the_image_instead() {
        let out = shadow(r#"{"Size":0,"Offset":"-4, -6"}"#)
            .apply(solid(20, 20, RED))
            .unwrap();
        assert_eq!(out.size(), (24, 26));
        assert_eq!(out.origin, (4., 6.));
    }

    #[test]
    fn the_shadow_is_tinted_and_darkness_strengthens_it() {
        let faint = shadow(r#"{"Size":0,"Opacity":0.4,"Color":"Blue","Offset":"0, 5"}"#)
            .apply(solid(10, 10, RED))
            .unwrap();
        let strong =
            shadow(r#"{"Size":0,"Opacity":0.4,"Color":"Blue","Offset":"0, 5","Darkness":1}"#)
                .apply(solid(10, 10, RED))
                .unwrap();
        let (low, high) = (pixel(&faint, 5, 12), pixel(&strong, 5, 12));
        assert_eq!(low & 0xff, low >> 24);
        assert!(high >> 24 > low >> 24);
    }

    #[test]
    fn without_auto_resize_the_canvas_keeps_its_size() {
        let out = shadow(r#"{"Size":3,"AutoResize":false,"Offset":"2, 2"}"#)
            .apply(solid(12, 12, RED))
            .unwrap();
        assert_eq!(out.size(), (12, 12));
        assert_eq!(out.origin, (0., 0.));
    }
}
