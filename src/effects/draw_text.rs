use super::{
    Bitmap, Effect,
    draw::{fits, rounded_rect},
    macros, typography,
    value::{Fill, FontSpec, Gradient, Margin, Offset, Placement, Rgba},
};
use crate::capture::Result;
use cairo::Context;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct DrawText {
    text: String,
    placement: Placement,
    offset: Offset,
    auto_hide: bool,
    text_font: FontSpec,
    text_color: Rgba,
    draw_text_shadow: bool,
    text_shadow_color: Rgba,
    text_shadow_offset: Offset,
    corner_radius: i32,
    padding: Margin,
    draw_border: bool,
    border_color: Rgba,
    border_size: i32,
    draw_background: bool,
    background_color: Rgba,
    use_gradient: bool,
    gradient: Gradient,
}

impl Default for DrawText {
    fn default() -> Self {
        Self {
            text: "Text watermark".to_owned(),
            placement: Placement::BottomRight,
            offset: Offset(5, 5),
            auto_hide: false,
            text_font: FontSpec::points("Arial", 11.25),
            text_color: Rgba::rgb(235, 235, 235),
            draw_text_shadow: true,
            text_shadow_color: Rgba::BLACK,
            text_shadow_offset: Offset(-1, -1),
            corner_radius: 4,
            padding: Margin::all(5),
            draw_border: true,
            border_color: Rgba::BLACK,
            border_size: 1,
            draw_background: true,
            background_color: Rgba::rgb(42, 47, 56),
            use_gradient: false,
            gradient: Gradient::default(),
        }
    }
}

impl Effect for DrawText {
    fn apply(&self, input: Bitmap) -> Result<Bitmap> {
        let canvas = input.size();
        let text = macros::expand(&self.text, canvas);
        if text.is_empty() || self.text_font.pixels < 1. {
            return Ok(input);
        }
        let (text_w, text_h) = typography::measure(&self.text_font, &text)?;
        let pad = self.padding;
        let (w, h) = (text_w + pad.horizontal(), text_h + pad.vertical());
        let (x, y) = self.placement.position(self.offset, canvas, (w, h));
        if self.auto_hide && !fits(canvas, (x, y, w, h)) {
            return Ok(input);
        }
        let out = input.blank_like()?;
        let c = Context::new(&out.surface)?;
        input.paint(&c, 0., 0.)?;
        let (fx, fy, fw, fh) = (f64::from(x), f64::from(y), f64::from(w), f64::from(h));
        let radius = f64::from(self.corner_radius.max(0));
        if self.draw_background {
            rounded_rect(&c, fx + 0.5, fy + 0.5, fw - 1., fh - 1., radius);
            Fill::new(self.use_gradient, self.background_color, &self.gradient)
                .set_source(&c, (fx, fy, fw, fh))?;
            c.fill()?;
        }
        if self.draw_border {
            rounded_rect(&c, fx + 0.5, fy + 0.5, fw - 1., fh - 1., radius);
            self.border_color.set_source(&c);
            c.set_line_width(f64::from(self.border_size.max(1)));
            c.stroke()?;
        }
        let layout = self.text_font.layout(&c, &text);
        let (text_x, text_y) = (fx + f64::from(pad.left), fy + f64::from(pad.top));
        if self.draw_text_shadow {
            self.text_shadow_color.set_source(&c);
            c.move_to(
                text_x + f64::from(self.text_shadow_offset.0),
                text_y + f64::from(self.text_shadow_offset.1),
            );
            pangocairo::functions::show_layout(&c, &layout);
        }
        self.text_color.set_source(&c);
        c.move_to(text_x, text_y);
        pangocairo::functions::show_layout(&c, &layout);
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::tests::{RED, pixel, solid};

    #[test]
    fn draws_a_filled_box_in_the_corner_and_can_hide() {
        let label: DrawText = serde_json::from_str(
            r##"{"Text":"Hi","Placement":"TopLeft","Offset":"2, 2","DrawBorder":false,
                "DrawTextShadow":false,"BackgroundColor":"Blue","Padding":"6, 6, 6, 6"}"##,
        )
        .unwrap();
        let out = label.apply(solid(120, 60, RED)).unwrap();
        assert_eq!(pixel(&out, 4, 10), 0xff0000ff);
        assert_eq!(pixel(&out, 100, 50), 0xffff0000);
        let hidden: DrawText = serde_json::from_str(
            r#"{"Text":"Hi","AutoHide":true,"Offset":"-5, 0","Placement":"TopLeft"}"#,
        )
        .unwrap();
        assert_eq!(
            pixel(&hidden.apply(solid(120, 60, RED)).unwrap(), 0, 0),
            0xffff0000
        );
    }

    #[test]
    fn empty_text_does_nothing() {
        let label: DrawText = serde_json::from_str(r#"{"Text":""}"#).unwrap();
        assert_eq!(label.apply(solid(10, 10, RED)).unwrap().size(), (10, 10));
    }
}
