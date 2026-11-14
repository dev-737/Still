use super::{
    Bitmap, Effect,
    value::{Fill, Gradient, Rgba},
};
use crate::capture::Result;
use cairo::Context;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct DrawBackground {
    color: Rgba,
    use_gradient: bool,
    gradient: Gradient,
}

impl Default for DrawBackground {
    fn default() -> Self {
        Self {
            color: Rgba::BLACK,
            use_gradient: false,
            gradient: Gradient::default(),
        }
    }
}

impl Effect for DrawBackground {
    fn apply(&self, input: Bitmap) -> Result<Bitmap> {
        let out = input.blank_like()?;
        let (w, h) = out.size();
        let c = Context::new(&out.surface)?;
        Fill::new(self.use_gradient, self.color, &self.gradient)
            .set_source(&c, (0., 0., f64::from(w), f64::from(h)))?;
        c.paint()?;
        input.paint(&c, 0., 0.)?;
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::tests::{pixel, solid};

    #[test]
    fn fills_behind_transparent_pixels_and_runs_gradients() {
        let flat: DrawBackground = serde_json::from_str(r#"{"Color":"Blue"}"#).unwrap();
        let out = flat.apply(solid(4, 4, (0., 0., 0., 0.))).unwrap();
        assert_eq!(pixel(&out, 1, 1), 0xff0000ff);
        let gradient: DrawBackground = serde_json::from_str(
            r#"{"UseGradient":true,"Gradient":{"Type":"Vertical","Colors":[
                {"Color":"Black","Location":0},{"Color":"White","Location":100}]}}"#,
        )
        .unwrap();
        let out = gradient.apply(solid(2, 100, (0., 0., 0., 0.))).unwrap();
        assert!(pixel(&out, 0, 5) & 0xff < 40);
        assert!(pixel(&out, 0, 95) & 0xff > 200);
    }
}
