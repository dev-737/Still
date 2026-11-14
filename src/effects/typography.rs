use super::value::FontSpec;
use crate::capture::{self, Result};
use cairo::{Context, Format, ImageSurface};
use gtk::pango::{
    self,
    prelude::{FontFamilyExt, FontMapExt},
};
use std::ffi::{CString, c_char, c_int, c_void};
use std::fs;
use std::os::unix::ffi::OsStrExt;

const HANDWRITING_FONT: &[u8] = include_bytes!("../../assets/fonts/Caveat.ttf");
const HANDWRITING_FILE: &str = "still-caveat.ttf";
const HANDWRITING_FAMILY: &str = "Caveat";
const HANDWRITING_SCALE: f64 = 1.5;
const HANDWRITING_ALIASES: &[&str] = &[
    "Ink Free",
    "Segoe Print",
    "Segoe Script",
    "Bradley Hand ITC",
];

#[link(name = "fontconfig")]
unsafe extern "C" {
    fn FcConfigAppFontAddFile(config: *mut c_void, file: *const c_char) -> c_int;
}

pub fn install_bundled_fonts() {
    let path = capture::cache_dir().join(HANDWRITING_FILE);
    let current = fs::metadata(&path).is_ok_and(|meta| meta.len() == HANDWRITING_FONT.len() as u64);
    if !current
        && (fs::create_dir_all(capture::cache_dir()).is_err()
            || fs::write(&path, HANDWRITING_FONT).is_err())
    {
        return;
    }
    if let Ok(file) = CString::new(path.as_os_str().as_bytes()) {
        unsafe { FcConfigAppFontAddFile(std::ptr::null_mut(), file.as_ptr()) };
    }
}

fn is_installed(family: &str) -> bool {
    pangocairo::FontMap::default()
        .list_families()
        .iter()
        .any(|installed| installed.name().eq_ignore_ascii_case(family))
}

impl FontSpec {
    fn substitute(&self) -> (&str, f64) {
        let wanted = HANDWRITING_ALIASES
            .iter()
            .any(|alias| alias.eq_ignore_ascii_case(&self.family));
        if wanted && !is_installed(&self.family) && is_installed(HANDWRITING_FAMILY) {
            (HANDWRITING_FAMILY, self.pixels * HANDWRITING_SCALE)
        } else {
            (&self.family, self.pixels)
        }
    }

    pub fn layout(&self, c: &Context, text: &str) -> pango::Layout {
        let layout = pangocairo::functions::create_layout(c);
        let (family, pixels) = self.substitute();
        let mut description = pango::FontDescription::new();
        description.set_family(family);
        if self.bold {
            description.set_weight(pango::Weight::Bold);
        }
        if self.italic {
            description.set_style(pango::Style::Italic);
        }
        description.set_absolute_size(pixels * f64::from(pango::SCALE));
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
