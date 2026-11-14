use super::value::FontSpec;
use crate::capture::Result;
use cairo::{Context, Format, ImageSurface};
use gtk::pango;

impl FontSpec {
    pub fn layout(&self, c: &Context, text: &str) -> pango::Layout {
        let layout = pangocairo::functions::create_layout(c);
        let mut description = pango::FontDescription::new();
        description.set_family(&self.family);
        if self.bold {
            description.set_weight(pango::Weight::Bold);
        }
        if self.italic {
            description.set_style(pango::Style::Italic);
        }
        description.set_absolute_size(self.pixels * f64::from(pango::SCALE));
        layout.set_font_description(Some(&description));
        let attributes = pango::AttrList::new();
        if self.underline {
            attributes.insert(pango::AttrInt::new_underline(pango::Underline::Single));
        }
        if self.strikeout {
            attributes.insert(pango::AttrInt::new_strikethrough(true));
        }
        layout.set_attributes(Some(&attributes));
        layout.set_text(text);
        layout
    }
}

pub fn measure(font: &FontSpec, text: &str) -> Result<(i32, i32)> {
    let c = Context::new(&ImageSurface::create(Format::ARgb32, 1, 1)?)?;
    Ok(font.layout(&c, text).pixel_size())
}
