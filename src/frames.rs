use crate::{
    capture::{self, Result},
    render::{Frame, Mode},
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
    let folder = root();
    fs::create_dir_all(&folder)?;
    let mut file = tempfile::NamedTempFile::new_in(&folder)?;
    serde_json::to_writer(&mut file, selection)?;
    file.persist(default_path())?;
    Ok(())
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
    let mode = match selection {
        DefaultFrame::Original => Mode::Original,
        DefaultFrame::Shadow => Mode::Shadow,
        DefaultFrame::Gradient => Mode::Gradient,
        DefaultFrame::Image => {
            let path = root().join("default-background.png");
            let mut file = fs::File::open(path)?;
            let background = cairo::ImageSurface::create_from_png(&mut file)?;
            return Ok(Frame {
                mode: Mode::Image,
                background: Some(background),
                ..Frame::default()
            });
        }
        DefaultFrame::Imported(id) => {
            let (mut frame, note) = sharex::load(&preset_path(id)?)?;
            frame.preset = Some((id.clone(), note));
            return Ok(frame);
        }
    };
    Ok(Frame {
        mode,
        ..Frame::default()
    })
}

pub fn render_capture(raw: &[u8]) -> Result<Option<Vec<u8>>> {
    let selection = default_frame()?;
    if selection == DefaultFrame::Original {
        return Ok(None);
    }
    let frame = load(&selection)?;
    let image = cairo::ImageSurface::create_from_png(&mut Cursor::new(raw))?;
    let mut edit = crate::render::Edit::new(&image);
    edit.frame = frame;
    Ok(Some(crate::render::png(&image, &edit)?))
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
    let (mut frame, note) = sharex::read(Cursor::new(bytes))?;
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
