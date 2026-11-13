use crate::{
    capture::Result,
    render::{Color, Frame, Mode, Overlay, OverlayContent},
};
use serde_json::Value;
use std::{
    io::{Read, Seek},
    path::Path,
};

pub fn load(path: &Path) -> Result<(Frame, String)> {
    let file = std::fs::File::open(path)?;
    if file.metadata()?.len() > 16 * 1024 * 1024 {
        return Err("Choose a preset under 16 MiB.".into());
    }
    read(file)
}

pub fn read(input: impl Read + Seek) -> Result<(Frame, String)> {
    let mut zip = zip::ZipArchive::new(input)?;
    if zip.len() > 256 {
        return Err("Too many entries in this preset.".into());
    }
    let entry = zip.by_name("Config.json")?;
    let mut data = Vec::new();
    entry.take(131073).read_to_end(&mut data)?;
    if data.len() > 131072 {
        return Err("Preset configuration is too large.".into());
    }
    let config: Value = serde_json::from_slice(&data)?;
    let effects = config["Effects"]
        .as_array()
        .ok_or("Missing ShareX effects.")?;
    let mut frame = Frame {
        mode: Mode::Shadow,
        padding: [0.; 4],
        radius: 0.,
        shadow: 0.,
        ..Frame::default()
    };
    let mut missing = std::collections::BTreeSet::new();
    let mut matched = 0;
    let mut rim = None;
    let mut image_budget = 4_194_304;
    let now = gtk::glib::DateTime::now_local().ok();
    for effect in effects
        .iter()
        .filter(|e| e["Enabled"].as_bool() != Some(false))
    {
        let kind = effect["$type"]
            .as_str()
            .unwrap_or("Unknown")
            .split(',')
            .next()
            .unwrap_or("")
            .trim()
            .rsplit('.')
            .next()
            .unwrap_or("");
        match kind {
            "RoundedCorners" if effect["CornerRadius"].is_number() => {
                let radius = effect["CornerRadius"].as_f64().unwrap().clamp(0., 512.);
                if frame.canvas_color.is_some() {
                    frame.canvas_radius = Some(radius);
                } else {
                    frame.radius = radius;
                }
            }
            "Shadow" if effect["Size"].is_number() => {
                frame.shadow = effect["Size"].as_f64().unwrap().clamp(0., 128.);
                frame.shadow_color = color(&effect["Color"]).unwrap_or((0., 0., 0., 1.));
                frame.shadow_color.3 = effect["Opacity"].as_f64().unwrap_or(0.6).clamp(0., 1.);
                if let Some([x, y]) = numbers(&effect["Offset"]) {
                    frame.shadow_offset = (x.clamp(-128., 128.), y.clamp(-128., 128.));
                }
            }
            "Canvas"
                if matches!(effect["MarginMode"].as_str(), None | Some("AbsoluteSize"))
                    && (effect["Color"] == "Transparent" || color(&effect["Color"]).is_some()) =>
            {
                if let Some(p) = numbers::<4>(&effect["Margin"])
                    .filter(|p| p.iter().all(|n| *n >= 0. && *n <= 512.))
                {
                    for (margin, added) in frame.padding.iter_mut().zip(p) {
                        *margin += added;
                    }
                    for overlay in &mut frame.overlays {
                        for (inset, added) in overlay.canvas_inset.iter_mut().zip(p) {
                            *inset += added;
                        }
                    }
                    if let Some(color) = color(&effect["Color"]) {
                        frame.canvas_color = Some(color);
                    }
                    rim = if p.iter().all(|&edge| edge == p[0]) {
                        Some(p[0])
                    } else {
                        None
                    };
                } else {
                    missing.insert(kind);
                    continue;
                }
            }
            "DrawBackground"
                if effect["UseGradient"] == false && color(&effect["Color"]).is_some() =>
            {
                let color = color(&effect["Color"]).unwrap();
                if let Some(size) = rim
                    .take()
                    .filter(|&size| size > 0. && frame.canvas_color.is_some())
                {
                    frame.canvas_rim = Some((color, size));
                } else {
                    frame.canvas_color = Some(color);
                }
            }
            "DrawBorder"
                if effect["Type"] == "Inside"
                    && effect["DashStyle"] == "Solid"
                    && effect["UseGradient"] != true =>
            {
                if let (Some(width), Some(color)) =
                    (effect["Size"].as_f64(), color(&effect["Color"]))
                {
                    frame.overlays.push(Overlay {
                        right: false,
                        bottom: false,
                        offset: (0., 0.),
                        canvas_inset: [0.; 4],
                        content: OverlayContent::Border {
                            width: width.clamp(0., 512.),
                            color,
                        },
                    });
                } else {
                    missing.insert(kind);
                    continue;
                }
            }
            "DrawTextEx" => {
                if let Some(overlay) = circle_glyph(effect) {
                    frame.overlays.push(overlay);
                } else {
                    missing.insert(kind);
                    continue;
                }
            }
            "DrawImage" => {
                if let Some(overlay) = image_overlay(&mut zip, effect, &mut image_budget) {
                    frame.overlays.push(overlay);
                } else {
                    missing.insert(kind);
                    continue;
                }
            }
            "DrawText" => {
                if let Some(overlay) = text_overlay(effect, now.as_ref()) {
                    frame.overlays.push(overlay);
                } else {
                    missing.insert(kind);
                    continue;
                }
            }
            "DrawBackground" if effect["UseGradient"] == true => {
                let g = &effect["Gradient"];
                let angle = match g["Type"].as_str() {
                    Some("Horizontal") => 90.,
                    Some("Vertical") => 180.,
                    Some("ForwardDiagonal") => 135.,
                    Some("BackwardDiagonal") => 225.,
                    _ => {
                        missing.insert(kind);
                        continue;
                    }
                };
                let stops = g["Colors"]
                    .as_array()
                    .filter(|a| (2..=5).contains(&a.len()))
                    .and_then(|a| {
                        a.iter()
                            .map(|s| {
                                Some((
                                    s["Location"].as_f64()?.clamp(0., 100.) / 100.,
                                    color(&s["Color"])?,
                                ))
                            })
                            .collect::<Option<Vec<_>>>()
                    });
                if let Some(stops) = stops {
                    frame.stops = stops;
                    frame.angle = angle;
                    frame.mode = Mode::Gradient;
                } else {
                    missing.insert(kind);
                    continue;
                }
            }
            _ => {
                missing.insert(kind);
                continue;
            }
        }
        matched += 1;
    }
    let unsupported = missing.into_iter().collect::<Vec<_>>().join(", ");
    if matched == 0 {
        return Err(format!("No supported effects. {unsupported}").into());
    }
    let name = config["Name"]
        .as_str()
        .unwrap_or("ShareX preset")
        .chars()
        .take(80)
        .collect::<String>();
    let message = if unsupported.is_empty() {
        format!("{name} · approximate preview")
    } else {
        format!("{name} · unsupported: {unsupported}")
    };
    Ok((frame, message))
}
fn overlay(effect: &Value, content: OverlayContent) -> Option<Overlay> {
    let (right, bottom) = match effect["Placement"].as_str()? {
        "TopLeft" => (false, false),
        "TopRight" => (true, false),
        "BottomLeft" => (false, true),
        "BottomRight" => (true, true),
        _ => return None,
    };
    let [x, y] = numbers(&effect["Offset"])?;
    Some(Overlay {
        right,
        bottom,
        offset: (x.clamp(-4096., 4096.), y.clamp(-4096., 4096.)),
        canvas_inset: [0.; 4],
        content,
    })
}
fn image_overlay<R: Read + Seek>(
    zip: &mut zip::ZipArchive<R>,
    effect: &Value,
    budget: &mut u64,
) -> Option<Overlay> {
    if effect["Tile"] == true
        || !matches!(
            effect["CompositingMode"].as_str(),
            None | Some("SourceOver")
        )
        || !matches!(
            effect["SizeMode"].as_str(),
            Some("AbsoluteSize" | "DontResize")
        )
    {
        return None;
    }
    let path = effect["ImageLocation"].as_str()?.replace('\\', "/");
    let path = path.strip_prefix("%ShareXImageEffects%/")?;
    if path
        .split('/')
        .any(|part| part.is_empty() || matches!(part, "." | "..") || part.contains(':'))
    {
        return None;
    }
    let windows = path.replace('/', "\\");
    let index = zip
        .index_for_name(path)
        .or_else(|| zip.index_for_name(&windows))?;
    let file = zip.by_index(index).ok()?;
    let mut bytes = Vec::new();
    file.take(2_097_153).read_to_end(&mut bytes).ok()?;
    if bytes.len() > 2_097_152 || !bytes.starts_with(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR") {
        return None;
    }
    let width = u32::from_be_bytes(bytes.get(16..20)?.try_into().ok()?);
    let height = u32::from_be_bytes(bytes.get(20..24)?.try_into().ok()?);
    let pixels = u64::from(width) * u64::from(height);
    if width == 0 || height == 0 || width > 4096 || height > 4096 || pixels > *budget {
        return None;
    }
    *budget -= pixels;
    let surface = cairo::ImageSurface::create_from_png(&mut std::io::Cursor::new(bytes)).ok()?;
    let flip = match effect["RotateFlip"].as_str() {
        None | Some("None") => (false, false),
        Some("FlipX") => (true, false),
        Some("FlipY") => (false, true),
        Some("Rotate180") => (true, true),
        _ => return None,
    };
    let [w, h] = if effect["SizeMode"] == "DontResize" {
        [0., 0.]
    } else {
        numbers::<2>(&effect["Size"])?
    };
    if !(0. ..=4096.).contains(&w) || !(0. ..=4096.).contains(&h) {
        return None;
    }
    let size = match (w, h) {
        (0., 0.) => (width as f64, height as f64),
        (0., h) => (h * width as f64 / height as f64, h),
        (w, 0.) => (w, w * height as f64 / width as f64),
        size => size,
    };
    overlay(
        effect,
        OverlayContent::Image {
            surface,
            size,
            flip,
            nearest: effect["InterpolationMode"] == "NearestNeighbor",
            opacity: effect["Opacity"].as_f64().unwrap_or(100.).clamp(0., 100.) / 100.,
        },
    )
}
fn circle_glyph(effect: &Value) -> Option<Overlay> {
    if effect["Text"].as_str()? != "\u{e91f}"
        || effect["Angle"] != 0
        || effect["UseGradient"] == true
        || effect["Shadow"] == true
        || effect["OutlineUseGradient"] == true
    {
        return None;
    }
    let (family, size) = effect["Font"].as_str()?.split_once(',')?;
    if family.trim() != "Segoe MDL2 Assets" {
        return None;
    }
    let points = size.trim().strip_suffix("pt")?.parse::<f64>().ok()?;
    if !(1. ..=128.).contains(&points) {
        return None;
    }
    let outline = if effect["Outline"] == true {
        Some((
            effect["OutlineSize"].as_f64()?.clamp(0., 24.),
            color(&effect["OutlineColor"])?,
        ))
    } else {
        None
    };
    overlay(
        effect,
        OverlayContent::Circle {
            diameter: points * 96. / 72.,
            color: color(&effect["Color"])?,
            outline,
        },
    )
}
fn text_overlay(effect: &Value, now: Option<&gtk::glib::DateTime>) -> Option<Overlay> {
    if [
        "DrawTextShadow",
        "DrawBorder",
        "DrawBackground",
        "UseGradient",
    ]
    .iter()
    .any(|key| effect[key] == true)
    {
        return None;
    }
    let raw = effect["Text"].as_str()?;
    let text = match raw {
        "%h:%mi %pm" => now?.format("%-I:%M %p").ok()?.to_string(),
        "%mo/%d/%y" => now?.format("%-m/%-d/%Y").ok()?.to_string(),
        s if !s.contains('%') && s.len() <= 512 => s.to_owned(),
        _ => return None,
    };
    let (font, pt) = effect["TextFont"].as_str()?.rsplit_once(',')?;
    let points = pt.trim().strip_suffix("pt")?.parse::<f64>().ok()?;
    if !(1. ..=128.).contains(&points) || font.len() > 128 {
        return None;
    }
    let padding = numbers::<4>(&effect["Padding"])?.map(|x| x.clamp(0., 512.));
    overlay(
        effect,
        OverlayContent::Text {
            text,
            font: font.trim().to_owned(),
            points,
            color: color(&effect["TextColor"])?,
            padding,
        },
    )
}

fn numbers<const N: usize>(v: &Value) -> Option<[f64; N]> {
    v.as_str()?
        .split(',')
        .map(|p| p.trim().parse::<f64>().ok().filter(|x| x.is_finite()))
        .collect::<Option<Vec<_>>>()?
        .try_into()
        .ok()
}
fn color(v: &Value) -> Option<Color> {
    match v.as_str()?.to_ascii_lowercase().as_str() {
        "black" => Some((0., 0., 0., 1.)),
        "white" => Some((1., 1., 1., 1.)),
        _ => {
            let [a, r, g, b] = if let Some([r, g, b]) = numbers::<3>(v) {
                [255., r, g, b]
            } else {
                numbers::<4>(v)?
            };
            if [a, r, g, b].iter().any(|n| !(0. ..=255.).contains(n)) {
                return None;
            }
            Some((r / 255., g / 255., b / 255., a / 255.))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn imports_the_gallery_sample() {
        let (frame, note) = read(std::io::Cursor::new(include_bytes!(
            "../assets/BackgroundGradient.sxie"
        )))
        .unwrap();
        assert!(matches!(frame.mode, Mode::Gradient));
        assert_eq!(frame.padding, [20., 20., 20., 10.]);
        assert_eq!(frame.radius, 20.);
        assert_eq!(frame.stops.len(), 2);
        assert!(note.contains("approximate preview"));
    }
}
