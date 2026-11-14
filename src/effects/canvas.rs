use super::{
    Bitmap, Effect,
    value::{Margin, Rgba},
};
use crate::capture::Result;
use cairo::{Context, Operator};
use serde::Deserialize;

#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize)]
pub enum MarginMode {
    #[default]
    AbsoluteSize,
    PercentageOfCanvas,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct Canvas {
    margin: Margin,
    margin_mode: MarginMode,
    color: Rgba,
}

impl Default for Canvas {
    fn default() -> Self {
        Self {
            margin: Margin::default(),
            margin_mode: MarginMode::default(),
            color: Rgba::TRANSPARENT,
        }
    }
}

impl Canvas {
    fn resolved(&self, (width, height): (i32, i32)) -> Margin {
        if self.margin_mode == MarginMode::AbsoluteSize {
            return self.margin;
        }
        let share = |percent: i32, of: i32| {
            (f64::from(percent) / 100. * f64::from(of)).round_ties_even() as i32
        };
        Margin {
            left: share(self.margin.left, width),
            top: share(self.margin.top, height),
            right: share(self.margin.right, width),
            bottom: share(self.margin.bottom, height),
        }
    }
}

impl Effect for Canvas {
    fn apply(&self, input: Bitmap) -> Result<Bitmap> {
        let (w, h) = input.size();
        let m = self.resolved((w, h));
        let (new_w, new_h) = (w + m.horizontal(), h + m.vertical());
        if m.is_zero() || new_w < 1 || new_h < 1 {
            return Ok(input);
        }
        let out = Bitmap::blank(
            new_w,
            new_h,
            (
                input.origin.0 + f64::from(m.left),
                input.origin.1 + f64::from(m.top),
            ),
        )?;
        let c = Context::new(&out.surface)?;
        if self.color.is_visible() {
            c.set_operator(Operator::Source);
            self.color.set_source(&c);
            for (x, y, w, h) in [
                (0, 0, m.left, new_h),
                (0, 0, new_w, m.top),
                (new_w - m.right, 0, m.right, new_h),
                (0, new_h - m.bottom, new_w, m.bottom),
            ] {
                if w > 0 && h > 0 {
                    c.rectangle(f64::from(x), f64::from(y), f64::from(w), f64::from(h));
                    c.fill()?;
                }
            }
            c.set_operator(Operator::Over);
        }
        input.paint(&c, f64::from(m.left), f64::from(m.top))?;
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::tests::{BLUE, RED, pixel, solid};

    fn canvas(json: &str) -> Canvas {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn adds_coloured_margins_around_the_image() {
        let out = canvas(r#"{"Margin":"2, 3, 4, 5","Color":"Blue"}"#)
            .apply(solid(10, 10, RED))
            .unwrap();
        assert_eq!(out.size(), (16, 18));
        assert_eq!(out.origin, (2., 3.));
        assert_eq!(pixel(&out, 0, 0), 0xff0000ff);
        assert_eq!(pixel(&out, 2, 3), 0xffff0000);
        assert_eq!(pixel(&out, 15, 17), 0xff0000ff);
    }

    #[test]
    fn negative_margins_crop_and_percentages_scale() {
        let out = canvas(r#"{"Margin":"-2, 0, 0, 0"}"#)
            .apply(solid(10, 10, BLUE))
            .unwrap();
        assert_eq!(out.size(), (8, 10));
        let out = canvas(r#"{"Margin":"10, 10, 10, 10","MarginMode":"PercentageOfCanvas"}"#)
            .apply(solid(50, 20, BLUE))
            .unwrap();
        assert_eq!(out.size(), (60, 24));
    }

    #[test]
    fn zero_margins_leave_the_image_alone() {
        let out = canvas("{}").apply(solid(5, 5, RED)).unwrap();
        assert_eq!(out.size(), (5, 5));
    }
}
