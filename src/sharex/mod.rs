mod package;

use crate::{
    capture::Result,
    effects::{self, BuildError, Effect, Pipeline},
};
use package::Package;
use serde_json::Value;
use std::{
    io::{Read, Seek},
    path::Path,
};

const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_EFFECTS: usize = 1024;
const MAX_NAME_CHARS: usize = 80;

#[derive(Clone)]
pub struct Preset {
    pub name: String,
    pub pipeline: Pipeline,
    pub skipped: Vec<String>,
}

impl Preset {
    pub fn note(&self) -> String {
        if self.skipped.is_empty() {
            self.name.clone()
        } else {
            format!("{} · skipped: {}", self.name, self.skipped.join(", "))
        }
    }
}

pub fn load(path: &Path) -> Result<Preset> {
    let file = std::fs::File::open(path)?;
    if file.metadata()?.len() > MAX_FILE_BYTES {
        return Err("Choose a preset under 16 MiB.".into());
    }
    read(file)
}

pub fn read(input: impl Read + Seek) -> Result<Preset> {
    let mut package = Package::open(input)?;
    let config = package.config()?;
    let list = config["Effects"]
        .as_array()
        .ok_or("Missing ShareX effects.")?;
    if list.len() > MAX_EFFECTS {
        return Err("Too many effects in this preset.".into());
    }
    let mut built: Vec<Box<dyn Effect>> = vec![];
    let mut skipped: Vec<String> = vec![];
    for settings in list
        .iter()
        .filter(|e| e["Enabled"].as_bool() != Some(false))
    {
        match effects::build(kind(settings), settings.clone(), &mut package) {
            Ok(effect) => built.push(effect),
            Err(error) => {
                let reason = match error {
                    BuildError::Unsupported(name) => name,
                    invalid => invalid.to_string(),
                };
                if !skipped.contains(&reason) {
                    skipped.push(reason);
                }
            }
        }
    }
    if built.is_empty() {
        return Err(format!("No supported effects. {}", skipped.join(", ")).into());
    }
    Ok(Preset {
        name: preset_name(&config),
        pipeline: Pipeline::new(built),
        skipped,
    })
}

fn preset_name(config: &Value) -> String {
    config["Name"]
        .as_str()
        .unwrap_or("ShareX preset")
        .chars()
        .take(MAX_NAME_CHARS)
        .collect()
}

pub fn name(path: &Path) -> Result<String> {
    let mut package = Package::open(std::fs::File::open(path)?)?;
    Ok(preset_name(&package.config()?))
}

fn kind(effect: &Value) -> &str {
    let full = effect["$type"].as_str().unwrap_or("Unknown");
    let name = full.split(',').next().unwrap_or(full).trim();
    name.rsplit('.').next().unwrap_or(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::Bitmap;
    use std::io::{Cursor, Write};
    use zip::{ZipWriter, write::SimpleFileOptions};

    fn preset(config: &str) -> Result<Preset> {
        let mut zip = ZipWriter::new(Cursor::new(vec![]));
        zip.start_file("Config.json", SimpleFileOptions::default())?;
        zip.write_all(config.as_bytes())?;
        read(Cursor::new(zip.finish()?.into_inner()))
    }

    #[test]
    fn builds_known_effects_and_reports_the_rest() {
        let preset = preset(
            r#"{"Name":"Mixed","Effects":[
                {"$type":"ShareX.ImageEffectsLib.Canvas, ShareX.ImageEffectsLib","Margin":"4, 4, 4, 4"},
                {"$type":"Sepia"},
                {"$type":"Canvas","Margin":"wide"},
                {"$type":"Blur","Enabled":false}]}"#,
        )
        .unwrap();
        assert_eq!(preset.skipped.len(), 2);
        assert_eq!(preset.skipped[0], "Sepia");
        assert!(preset.skipped[1].starts_with("Canvas: "));
        assert!(preset.note().starts_with("Mixed · skipped: Sepia"));
    }

    #[test]
    fn a_preset_with_nothing_usable_is_an_error() {
        let error = preset(r#"{"Effects":[{"$type":"Sepia"}]}"#).err().unwrap();
        assert_eq!(error.to_string(), "No supported effects. Sepia");
        assert!(preset(r#"{"Name":"x"}"#).is_err());
    }

    #[test]
    fn a_fully_supported_preset_has_a_plain_note() {
        let preset = preset(r#"{"Name":"Quiet","Effects":[{"$type":"Blur"}]}"#).unwrap();
        assert_eq!(preset.note(), "Quiet");
    }

    #[test]
    fn imports_the_bundled_gallery_sample() {
        let preset = read(Cursor::new(include_bytes!(
            "../../assets/BackgroundGradient.sxie"
        )))
        .unwrap();
        assert!(preset.skipped.is_empty(), "{:?}", preset.skipped);
        let out = preset
            .pipeline
            .apply(Bitmap::blank(100, 50, (0., 0.)).unwrap())
            .unwrap();
        assert_eq!(out.size(), (200, 150));
        assert_eq!(out.origin, (50., 50.));
    }

    #[test]
    #[ignore]
    fn gallery_renders() {
        let dir = std::env::var("STILL_GALLERY").expect("set STILL_GALLERY");
        let out = std::env::var("STILL_GALLERY_OUT").expect("set STILL_GALLERY_OUT");
        let sample = include_bytes!("../../assets/frame-sample.png");
        let image = cairo::ImageSurface::create_from_png(&mut Cursor::new(&sample[..])).unwrap();
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let preset = load(&path).unwrap();
            let mut edit = crate::render::Edit::new(&image);
            edit.frame = crate::render::Frame::from_pipeline(preset.pipeline);
            let png = crate::render::png(&image, &edit).unwrap();
            let name = path.file_stem().unwrap().to_string_lossy().into_owned();
            std::fs::write(format!("{out}/{name}.png"), png).unwrap();
        }
    }

    #[test]
    #[ignore]
    fn gallery_coverage() {
        let dir = std::env::var("STILL_GALLERY").expect("set STILL_GALLERY");
        let mut paths: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        paths.sort();
        let (mut full, mut partial, mut none) = (0, 0, 0);
        for path in paths {
            let name = path.file_stem().unwrap().to_string_lossy().into_owned();
            match load(&path) {
                Ok(preset) if preset.skipped.is_empty() => {
                    full += 1;
                    println!("FULL    {name}");
                }
                Ok(preset) => {
                    partial += 1;
                    println!("PARTIAL {name}: {}", preset.skipped.join(", "));
                }
                Err(error) => {
                    none += 1;
                    println!("NONE    {name}: {error}");
                }
            }
        }
        println!("full {full}, partial {partial}, none {none}");
    }
}
