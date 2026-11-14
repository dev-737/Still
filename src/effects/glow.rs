use super::{
    Bitmap, Effect,
    halo::{compose, silhouette},
    value::{Fill, Gradient, GradientKind, Offset, Rgba},
};
use crate::capture::Result;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct Glow {
    size: i32,
    strength: f64,
    color: Rgba,
    use_gradient: bool,
    gradient: Gradient,
    offset: Offset,
}

impl Default for Glow {
    fn default() -> Self {
        Self {
            size: 20,
            strength: 1.,
            color: Rgba::WHITE,
            use_gradient: false,
            gradient: Gradient::teal(GradientKind::ForwardDiagonal),
            offset: Offset::default(),
        }
    }
}

impl Effect for Glow {
    fn apply(&self, input: Bitmap) -> Result<Bitmap> {
        let size = self.size.max(0);
        let mut mask = silhouette(&input, size)?;
        let fill = Fill::new(self.use_gradient, self.color, &self.gradient);
        let fill_alpha = match fill {
            Fill::Solid(color) => color.a,
            Fill::Gradient(_) => 1.,
        };
        mask.scale_alpha(self.strength.max(0.1) * fill_alpha);
        compose(input, mask, &fill, size, self.offset, true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::tests::{RED, pixel, solid};

    #[test]
    fn surrounds_the_image_with_the_glow_colour() {
        let glow: Glow = serde_json::from_str(r#"{"Size":6,"Color":"Blue","Strength":3}"#).unwrap();
        let out = glow.apply(solid(10, 10, RED)).unwrap();
        assert_eq!(out.size(), (22, 22));
        assert_eq!(out.origin, (6., 6.));
        assert_eq!(pixel(&out, 11, 11), 0xffff0000);
        let near = pixel(&out, 4, 11);
        assert!(near >> 24 > 0 && near & 0xff == near >> 24);
        assert_eq!(pixel(&out, 0, 0), 0);
    }

    #[test]
    fn a_gradient_glow_uses_the_gradient_colours() {
        let glow: Glow = serde_json::from_str(
            r#"{"Size":4,"UseGradient":true,"Gradient":{"Type":"Horizontal","Colors":[
                {"Color":"Blue","Location":0},{"Color":"Lime","Location":100}]}}"#,
        )
        .unwrap();
        let out = glow.apply(solid(30, 6, RED)).unwrap();
        let (left, right) = (pixel(&out, 1, 7), pixel(&out, 37, 7));
        assert!(left & 0xff > left >> 8 & 0xff);
        assert!(right >> 8 & 0xff > right & 0xff);
    }
}
