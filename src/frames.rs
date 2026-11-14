use crate::{
    capture::{self, Result},
    render::{Frame, Gradient, Mode},
    sharex,
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum DefaultFrame {
    #[default]
    Original,
    Shadow,
    Gradient,
    Image,
    Imported(String),
}

fn root() -> PathBuf {
    capture::config_dir()
}
fn library() -> PathBuf {
    root().join("frames")
}
fn default_path() -> PathBuf {
    root().join("default-frame.json")
}
fn style_path() -> PathBuf {
    root().join("default-style.json")
}
fn palettes_path() -> PathBuf {
    root().join("palettes.json")
}

type Rgba = (f64, f64, f64, f64);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FrameStyle {
    padding: [f64; 4],
    adaptive_padding: bool,
    extra_space: f64,
    radius: f64,
    outer_radius: f64,
    shadow: f64,
    shadow_offset: (f64, f64),
    shadow_color: Rgba,
    gradient: u8,
    angle: f64,
    stops: Vec<(f64, Rgba)>,
}

impl From<&Frame> for FrameStyle {
    fn from(frame: &Frame) -> Self {
        Self {
            padding: frame.padding,
            adaptive_padding: frame.adaptive_padding,
            extra_space: frame.extra_space,
            radius: frame.radius,
            outer_radius: frame.outer_radius,
            shadow: frame.shadow,
            shadow_offset: frame.shadow_offset,
            shadow_color: frame.shadow_color,
            gradient: match frame.gradient {
                Gradient::Linear => 0,
                Gradient::Radial => 1,
                Gradient::Conic => 2,
            },
            angle: frame.angle,
            stops: frame.stops.clone(),
        }
    }
}

impl FrameStyle {
    fn apply(&self, frame: &mut Frame) {
        frame.padding = self.padding;
        frame.adaptive_padding = self.adaptive_padding;
        frame.extra_space = self.extra_space;
        frame.radius = self.radius;
        frame.outer_radius = self.outer_radius;
        frame.shadow = self.shadow;
        frame.shadow_offset = self.shadow_offset;
        frame.shadow_color = self.shadow_color;
        frame.gradient = match self.gradient {
            1 => Gradient::Radial,
            2 => Gradient::Conic,
            _ => Gradient::Linear,
        };
        frame.angle = self.angle;
        if self.stops.len() >= 2 {
            frame.stops = self.stops.clone();
        }
    }
}

fn write_json(path: PathBuf, value: &impl Serialize) -> Result<()> {
    let folder = root();
    fs::create_dir_all(&folder)?;
    let mut file = tempfile::NamedTempFile::new_in(&folder)?;
    serde_json::to_writer(&mut file, value)?;
    file.persist(path)?;
    Ok(())
}

pub fn set_default_style(frame: &Frame) -> Result<()> {
    if frame.mode == Mode::Original && frame.preset.is_none() {
        return match fs::remove_file(style_path()) {
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(err.into()),
            _ => Ok(()),
        };
    }
    write_json(style_path(), &FrameStyle::from(frame))
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Palette {
    pub name: String,
    pub stops: Vec<(f64, Rgba)>,
}

pub fn palettes() -> Vec<Palette> {
    fs::read(palettes_path())
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

pub fn set_palettes(all: &[Palette]) -> Result<()> {
    write_json(palettes_path(), &all)
}

pub fn default_frame() -> Result<DefaultFrame> {
    match fs::read(default_path()) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(DefaultFrame::Original),
        Err(err) => Err(err.into()),
    }
}

pub fn save_background(surface: &cairo::ImageSurface) -> Result<()> {
    let folder = root();
    fs::create_dir_all(&folder)?;
    let mut file = tempfile::NamedTempFile::new_in(&folder)?;
    surface.write_to_png(&mut file)?;
    file.persist(folder.join("default-background.png"))?;
    Ok(())
}

pub fn set_default(selection: &DefaultFrame) -> Result<()> {
    write_json(default_path(), selection)
}

fn preset_path(id: &str) -> Result<PathBuf> {
    if !id.starts_with("frame-")
        || !id.ends_with(".sxie")
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.'))
    {
        return Err("Invalid preset ID.".into());
    }
    Ok(library().join(id))
}

pub fn load(selection: &DefaultFrame) -> Result<Frame> {
    Ok(match selection {
        DefaultFrame::Original => Frame {
            mode: Mode::Original,
            ..Frame::default()
        },
        DefaultFrame::Shadow => Frame {
            mode: Mode::Shadow,
            ..Frame::default()
        },
        DefaultFrame::Gradient => Frame {
            mode: Mode::Gradient,
            ..Frame::default()
        },
        DefaultFrame::Image => {
            let path = root().join("default-background.png");
            let mut file = fs::File::open(path)?;
            let background = cairo::ImageSurface::create_from_png(&mut file)?;
            Frame {
                mode: Mode::Image,
                background: Some(background),
                ..Frame::default()
            }
        }
        DefaultFrame::Imported(id) => {
            let preset = sharex::load(&preset_path(id)?)?;
            let mut frame = Frame::from_pipeline(preset.pipeline.clone());
            frame.preset = Some((id.clone(), preset.note()));
            frame
        }
    })
}

pub fn load_default(selection: &DefaultFrame) -> Result<Frame> {
    let mut frame = load(selection)?;
    if *selection != DefaultFrame::Original {
        let saved = fs::read(style_path())
            .ok()
            .and_then(|bytes| serde_json::from_slice::<FrameStyle>(&bytes).ok());
        if let Some(style) = saved {
            style.apply(&mut frame);
        }
    }
    Ok(frame)
}

pub fn render_capture(raw: &[u8]) -> Result<Option<Vec<u8>>> {
    let selection = default_frame()?;
    if selection == DefaultFrame::Original {
        return Ok(None);
    }
    let frame = load_default(&selection)?;
    let image = cairo::ImageSurface::create_from_png(&mut Cursor::new(raw))?;
    let mut edit = crate::render::Edit::new(&image);
    edit.frame = frame;
    Ok(Some(crate::render::png(&image, &edit)?))
}

impl DefaultFrame {
    pub fn imported_id(&self) -> Option<&str> {
        match self {
            Self::Imported(id) => Some(id),
            _ => None,
        }
    }
}

pub fn selection(frame: &Frame) -> DefaultFrame {
    if let Some((id, _)) = &frame.preset {
        return DefaultFrame::Imported(id.clone());
    }
    match frame.mode {
        Mode::Original => DefaultFrame::Original,
        Mode::Shadow => DefaultFrame::Shadow,
        Mode::Gradient => DefaultFrame::Gradient,
        Mode::Image => DefaultFrame::Image,
    }
}

pub fn list() -> Result<Vec<String>> {
    let entries = match fs::read_dir(library()) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(err) => return Err(err.into()),
    };
    let mut ids = Vec::new();
    for entry in entries {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        if let Ok(id) = entry.file_name().into_string() {
            if preset_path(&id).is_ok() {
                ids.push(id);
            }
        }
    }
    ids.sort();
    Ok(ids)
}

pub fn entries() -> Result<Vec<(String, String)>> {
    let mut entries = list()?
        .into_iter()
        .map(|id| {
            let name = preset_path(&id)
                .and_then(|path| sharex::name(&path))
                .unwrap_or_else(|_| id.clone());
            (id, name)
        })
        .collect::<Vec<_>>();
    entries.sort_by_key(|(_, name)| name.to_lowercase());
    Ok(entries)
}

pub fn import(path: &Path) -> Result<Frame> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 16 * 1024 * 1024 {
        return Err("Choose a preset under 16 MiB.".into());
    }
    save_preset(&bytes)
}

pub fn save_preset(bytes: &[u8]) -> Result<Frame> {
    let preset = sharex::read(Cursor::new(bytes))?;
    let note = preset.note();
    let mut frame = Frame::from_pipeline(preset.pipeline);
    for id in list()? {
        if fs::metadata(preset_path(&id)?)
            .is_ok_and(|metadata| metadata.len() == bytes.len() as u64)
            && fs::read(preset_path(&id)?).is_ok_and(|existing| existing == bytes)
        {
            frame.preset = Some((id, note));
            return Ok(frame);
        }
    }
    let folder = library();
    fs::create_dir_all(&folder)?;
    let mut file = tempfile::Builder::new()
        .prefix("frame-")
        .suffix(".sxie")
        .tempfile_in(&folder)?;
    file.write_all(bytes)?;
    let (_, path) = file.keep()?;
    frame.preset = Some((
        path.file_name().unwrap().to_string_lossy().into_owned(),
        note,
    ));
    Ok(frame)
}

pub fn delete(id: &str) -> Result<()> {
    let path = preset_path(id)?;
    if default_frame()? == DefaultFrame::Imported(id.to_owned()) {
        set_default(&DefaultFrame::Original)?;
    }
    fs::remove_file(path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_style_restores_a_customized_frame() {
        let mut custom = Frame {
            mode: Mode::Gradient,
            ..Frame::default()
        };
        custom.padding = [48.; 4];
        custom.radius = 20.;
        custom.gradient = Gradient::Radial;
        custom.angle = 215.;
        custom.stops = vec![
            (0., (0.1, 0.8, 0.3, 1.)),
            (0.4, (0.2, 0.2, 0.9, 1.)),
            (1., (0.9, 0.1, 0.5, 1.)),
        ];
        let json = serde_json::to_vec(&FrameStyle::from(&custom)).unwrap();
        let style: FrameStyle = serde_json::from_slice(&json).unwrap();
        let mut fresh = Frame {
            mode: Mode::Gradient,
            ..Frame::default()
        };
        assert_ne!(FrameStyle::from(&fresh), style);
        style.apply(&mut fresh);
        assert_eq!(FrameStyle::from(&fresh), FrameStyle::from(&custom));
    }
}
