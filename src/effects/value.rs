use cairo::{Context, LinearGradient};
use serde::Deserialize;
use std::str::FromStr;

macro_rules! from_string {
    ($($type:ty),*) => {$(
        impl TryFrom<String> for $type {
            type Error = String;
            fn try_from(text: String) -> Result<Self, String> {
                text.parse()
            }
        }
    )*};
}

fn integers<const N: usize>(text: &str) -> Result<[i32; N], String> {
    let parts = text
        .split(',')
        .map(|part| {
            part.trim()
                .parse::<i32>()
                .map_err(|_| format!("“{text}” is not a list of whole numbers"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    parts
        .try_into()
        .map_err(|_| format!("“{text}” should have {N} numbers"))
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize)]
#[serde(try_from = "String")]
pub struct Offset(pub i32, pub i32);

impl FromStr for Offset {
    type Err = String;
    fn from_str(text: &str) -> Result<Self, String> {
        let [x, y] = integers(text)?;
        Ok(Self(x, y))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize)]
#[serde(try_from = "String")]
pub struct Size(pub i32, pub i32);

impl FromStr for Size {
    type Err = String;
    fn from_str(text: &str) -> Result<Self, String> {
        let [w, h] = integers(text)?;
        Ok(Self(w, h))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize)]
#[serde(try_from = "String")]
pub struct Margin {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl FromStr for Margin {
    type Err = String;
    fn from_str(text: &str) -> Result<Self, String> {
        let [left, top, right, bottom] = if text.contains(',') {
            integers(text)?
        } else {
            let [all] = integers(text)?;
            [all; 4]
        };
        Ok(Self {
            left,
            top,
            right,
            bottom,
        })
    }
}

impl Margin {
    pub fn all(value: i32) -> Self {
        Self {
            left: value,
            top: value,
            right: value,
            bottom: value,
        }
    }
    pub fn is_zero(&self) -> bool {
        *self == Self::default()
    }
    pub fn has_negative(&self) -> bool {
        [self.left, self.top, self.right, self.bottom]
            .iter()
            .any(|&side| side < 0)
    }
    pub fn horizontal(&self) -> i32 {
        self.left + self.right
    }
    pub fn vertical(&self) -> i32 {
        self.top + self.bottom
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(try_from = "String")]
pub struct Rgba {
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub a: f64,
}

from_string!(Offset, Size, Margin, Rgba, FontSpec, Sides);

impl Rgba {
    pub const BLACK: Self = Self::rgb(0, 0, 0);
    pub const WHITE: Self = Self::rgb(255, 255, 255);
    pub const TRANSPARENT: Self = Self::new(0, 0, 0, 0);

    const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self {
            r: r as f64 / 255.,
            g: g as f64 / 255.,
            b: b as f64 / 255.,
            a: a as f64 / 255.,
        }
    }
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self::new(r, g, b, 255)
    }
    pub fn set_source(&self, c: &Context) {
        c.set_source_rgba(self.r, self.g, self.b, self.a);
    }
    pub fn is_visible(&self) -> bool {
        self.a > 0.
    }
}

impl FromStr for Rgba {
    type Err = String;
    fn from_str(text: &str) -> Result<Self, String> {
        let named = match text.trim().to_ascii_lowercase().as_str() {
            "black" => Some(Self::BLACK),
            "white" => Some(Self::WHITE),
            "transparent" => Some(Self::TRANSPARENT),
            "red" => Some(Self::rgb(255, 0, 0)),
            "lime" => Some(Self::rgb(0, 255, 0)),
            "blue" => Some(Self::rgb(0, 0, 255)),
            "yellow" => Some(Self::rgb(255, 255, 0)),
            "cyan" | "aqua" => Some(Self::rgb(0, 255, 255)),
            "magenta" | "fuchsia" => Some(Self::rgb(255, 0, 255)),
            "gray" | "grey" => Some(Self::rgb(128, 128, 128)),
            "lightgray" | "lightgrey" => Some(Self::rgb(211, 211, 211)),
            "darkgray" | "darkgrey" => Some(Self::rgb(169, 169, 169)),
            "green" => Some(Self::rgb(0, 128, 0)),
            "orange" => Some(Self::rgb(255, 165, 0)),
            _ => None,
        };
        if let Some(color) = named {
            return Ok(color);
        }
        let channel = |value: i32| u8::try_from(value).map_err(|_| format!("bad colour “{text}”"));
        if text.matches(',').count() == 2 {
            let [r, g, b] = integers(text)?;
            Ok(Self::rgb(channel(r)?, channel(g)?, channel(b)?))
        } else if text.matches(',').count() == 3 {
            let [a, r, g, b] = integers(text)?;
            Ok(Self::new(
                channel(r)?,
                channel(g)?,
                channel(b)?,
                channel(a)?,
            ))
        } else {
            Err(format!("unknown colour “{text}”"))
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize)]
pub enum GradientKind {
    #[default]
    Vertical,
    Horizontal,
    ForwardDiagonal,
    BackwardDiagonal,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct GradientStop {
    pub color: Rgba,
    pub location: f64,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct Gradient {
    #[serde(rename = "Type")]
    pub kind: GradientKind,
    pub colors: Vec<GradientStop>,
}

impl Default for Gradient {
    fn default() -> Self {
        let stop = |r, g, b, location| GradientStop {
            color: Rgba::rgb(r, g, b),
            location,
        };
        Self {
            kind: GradientKind::Vertical,
            colors: vec![
                stop(68, 120, 194, 0.),
                stop(13, 58, 122, 50.),
                stop(6, 36, 78, 50.),
                stop(23, 89, 174, 100.),
            ],
        }
    }
}

impl Gradient {
    pub fn teal(kind: GradientKind) -> Self {
        Self {
            kind,
            colors: vec![
                GradientStop {
                    color: Rgba::rgb(0, 187, 138),
                    location: 0.,
                },
                GradientStop {
                    color: Rgba::rgb(0, 105, 163),
                    location: 100.,
                },
            ],
        }
    }
    pub fn is_visible(&self) -> bool {
        self.colors.iter().any(|stop| stop.color.is_visible())
    }
    fn pattern(&self, (x, y, w, h): Rect) -> LinearGradient {
        let (x2, y2) = (x + w, y + h);
        let ((x0, y0), (x1, y1)) = match self.kind {
            GradientKind::Vertical => ((x, y), (x, y2)),
            GradientKind::Horizontal => ((x, y), (x2, y)),
            GradientKind::ForwardDiagonal => ((x, y), (x2, y2)),
            GradientKind::BackwardDiagonal => ((x2, y), (x, y2)),
        };
        let pattern = LinearGradient::new(x0, y0, x1, y1);
        let mut stops = self.colors.clone();
        stops.sort_by(|a, b| a.location.total_cmp(&b.location));
        for stop in stops {
            let Rgba { r, g, b, a } = stop.color;
            pattern.add_color_stop_rgba((stop.location / 100.).clamp(0., 1.), r, g, b, a);
        }
        pattern
    }
}

pub type Rect = (f64, f64, f64, f64);

#[derive(Clone, Debug)]
pub enum Fill {
    Solid(Rgba),
    Gradient(Gradient),
}

impl Fill {
    pub fn new(use_gradient: bool, color: Rgba, gradient: &Gradient) -> Self {
        if use_gradient && !gradient.colors.is_empty() {
            Self::Gradient(gradient.clone())
        } else {
            Self::Solid(color)
        }
    }
    pub fn is_visible(&self) -> bool {
        match self {
            Self::Solid(color) => color.is_visible(),
            Self::Gradient(gradient) => gradient.is_visible(),
        }
    }
    pub fn set_source(&self, c: &Context, area: Rect) -> crate::capture::Result<()> {
        match self {
            Self::Solid(color) => color.set_source(c),
            Self::Gradient(gradient) => c.set_source(gradient.pattern(area))?,
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize)]
pub enum Placement {
    #[default]
    TopLeft,
    TopCenter,
    TopRight,
    MiddleLeft,
    MiddleCenter,
    MiddleRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

impl Placement {
    pub fn position(
        self,
        Offset(x, y): Offset,
        canvas: (i32, i32),
        object: (i32, i32),
    ) -> (i32, i32) {
        let (right, bottom) = (canvas.0 - object.0, canvas.1 - object.1);
        let centre = |canvas: i32, object: i32| {
            (f64::from(canvas) / 2. - f64::from(object) / 2.).round_ties_even() as i32
        };
        let (mid_x, mid_y) = (centre(canvas.0, object.0), centre(canvas.1, object.1));
        match self {
            Self::TopLeft => (x, y),
            Self::TopCenter => (mid_x, y),
            Self::TopRight => (right - x, y),
            Self::MiddleLeft => (x, mid_y),
            Self::MiddleCenter => (mid_x, mid_y),
            Self::MiddleRight => (right - x, mid_y),
            Self::BottomLeft => (x, bottom - y),
            Self::BottomCenter => (mid_x, bottom - y),
            Self::BottomRight => (right - x, bottom - y),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(try_from = "String")]
pub struct Sides {
    pub top: bool,
    pub bottom: bool,
    pub left: bool,
    pub right: bool,
}

impl Default for Sides {
    fn default() -> Self {
        Self {
            top: true,
            bottom: true,
            left: true,
            right: true,
        }
    }
}

impl Sides {
    pub fn is_empty(&self) -> bool {
        !(self.top || self.bottom || self.left || self.right)
    }
}

impl FromStr for Sides {
    type Err = String;
    fn from_str(text: &str) -> Result<Self, String> {
        let mut sides = Self {
            top: false,
            bottom: false,
            left: false,
            right: false,
        };
        for name in text.split(',').map(str::trim) {
            match name.to_ascii_lowercase().as_str() {
                "top" => sides.top = true,
                "bottom" => sides.bottom = true,
                "left" => sides.left = true,
                "right" => sides.right = true,
                "none" | "" => {}
                _ => return Err(format!("unknown side “{name}”")),
            }
        }
        Ok(sides)
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(try_from = "String")]
pub struct FontSpec {
    pub family: String,
    pub pixels: f64,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikeout: bool,
}

impl FontSpec {
    pub fn points(family: &str, points: f64) -> Self {
        Self {
            family: family.to_owned(),
            pixels: points * 96. / 72.,
            bold: false,
            italic: false,
            underline: false,
            strikeout: false,
        }
    }
}

impl FromStr for FontSpec {
    type Err = String;
    fn from_str(text: &str) -> Result<Self, String> {
        let mut parts = text.split(',').map(str::trim);
        let family = parts
            .next()
            .filter(|name| !name.is_empty() && name.len() <= 128)
            .ok_or("a font needs a name")?;
        let size = parts.next().unwrap_or("8.25pt").to_ascii_lowercase();
        let (number, per_unit) = [
            ("pt", 96. / 72.),
            ("px", 1.),
            ("in", 96.),
            ("mm", 96. / 25.4),
            ("doc", 96. / 300.),
            ("world", 1.),
            ("display", 1.),
        ]
        .iter()
        .find_map(|(unit, factor)| size.strip_suffix(unit).map(|n| (n, *factor)))
        .unwrap_or((size.as_str(), 96. / 72.));
        let pixels = number
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|n| n.is_finite())
            .ok_or_else(|| format!("bad font size “{size}”"))?
            * per_unit;
        let styles = parts.collect::<Vec<_>>().join(",").to_ascii_lowercase();
        let has = |style: &str| styles.contains(style);
        Ok(Self {
            family: family.to_owned(),
            pixels,
            bold: has("bold"),
            italic: has("italic"),
            underline: has("underline"),
            strikeout: has("strikeout"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sharex_notation() {
        assert_eq!("10, -4".parse(), Ok(Offset(10, -4)));
        assert_eq!("7".parse(), Ok(Margin::all(7)));
        assert_eq!(
            "1, 2, 3, 4".parse(),
            Ok(Margin {
                left: 1,
                top: 2,
                right: 3,
                bottom: 4
            })
        );
        assert_eq!("Black".parse(), Ok(Rgba::BLACK));
        assert_eq!("206, 206, 206".parse(), Ok(Rgba::rgb(206, 206, 206)));
        let translucent: Rgba = "150, 0, 0, 0".parse().unwrap();
        assert!((translucent.a - 150. / 255.).abs() < 1e-9 && translucent.r == 0.);
        assert!("300, 0, 0".parse::<Rgba>().is_err());
        assert!("Nope".parse::<Rgba>().is_err());
        let sides: Sides = "Top, Left".parse().unwrap();
        assert!(sides.top && sides.left && !sides.bottom && !sides.right);
    }

    #[test]
    fn parses_fonts() {
        let font: FontSpec = "Segoe UI, 9.75pt, style=Bold".parse().unwrap();
        assert_eq!(font.family, "Segoe UI");
        assert!((font.pixels - 13.).abs() < 1e-9);
        assert!(font.bold && !font.italic);
        let plain: FontSpec = "Arial, 20px".parse().unwrap();
        assert_eq!(plain.pixels, 20.);
        assert!("Arial, big".parse::<FontSpec>().is_err());
    }

    #[test]
    fn placement_pushes_offsets_inwards() {
        let at = |p: Placement| p.position(Offset(5, 7), (100, 50), (20, 10));
        assert_eq!(at(Placement::TopLeft), (5, 7));
        assert_eq!(at(Placement::BottomRight), (75, 33));
        assert_eq!(at(Placement::MiddleCenter), (40, 20));
        assert_eq!(at(Placement::TopCenter), (40, 7));
    }
}
