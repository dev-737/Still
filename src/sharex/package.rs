use crate::{capture::Result, effects::Assets};
use cairo::ImageSurface;
use serde_json::Value;
use std::{
    collections::HashMap,
    io::{Cursor, Read, Seek},
};
use zip::ZipArchive;

const MAX_ENTRIES: usize = 2048;
const MAX_CONFIG_BYTES: u64 = 1 << 20;
const MAX_IMAGE_BYTES: u64 = 12 << 20;
const MAX_IMAGE_SIDE: u32 = 16_384;
const MAX_IMAGE_PIXELS: u64 = 4 << 20;
const DECODED_PIXEL_BUDGET: u64 = 16 << 20;
const ASSET_ROOT: &str = "%ShareXImageEffects%";
const PNG_HEADER: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";

pub struct Package<R> {
    zip: ZipArchive<R>,
    entries: Vec<(String, usize)>,
    decoded: HashMap<String, ImageSurface>,
    budget: u64,
}

impl<R: Read + Seek> Package<R> {
    pub fn open(input: R) -> Result<Self> {
        let mut zip = ZipArchive::new(input)?;
        if zip.len() > MAX_ENTRIES {
            return Err("Too many entries in this preset.".into());
        }
        let mut entries = Vec::with_capacity(zip.len());
        for index in 0..zip.len() {
            let file = zip.by_index_raw(index)?;
            if !file.is_dir() {
                entries.push((file.name().replace('\\', "/"), index));
            }
        }
        Ok(Self {
            zip,
            entries,
            decoded: HashMap::new(),
            budget: DECODED_PIXEL_BUDGET,
        })
    }

    pub fn config(&mut self) -> Result<Value> {
        let bytes = self.read("Config.json", MAX_CONFIG_BYTES)?;
        let json = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&bytes);
        Ok(serde_json::from_slice(json)?)
    }

    fn read(&mut self, name: &str, limit: u64) -> Result<Vec<u8>> {
        let index = self
            .entries
            .iter()
            .find(|(entry, _)| entry == name)
            .map(|&(_, index)| index)
            .ok_or_else(|| format!("{name} is missing from the preset."))?;
        let mut bytes = vec![];
        self.zip
            .by_index(index)?
            .take(limit + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > limit {
            return Err(format!("{name} is too large.").into());
        }
        Ok(bytes)
    }

    fn decode(&mut self, path: &str) -> Result<ImageSurface> {
        if let Some(image) = self.decoded.get(path) {
            return Ok(image.clone());
        }
        let bytes = self.read(path, MAX_IMAGE_BYTES)?;
        if !bytes.starts_with(PNG_HEADER) {
            return Err(format!("{path} is not a PNG image.").into());
        }
        let side = |at: usize| u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap());
        let (width, height) = (side(16), side(20));
        let pixels = u64::from(width) * u64::from(height);
        if width == 0
            || height == 0
            || width > MAX_IMAGE_SIDE
            || height > MAX_IMAGE_SIDE
            || pixels > MAX_IMAGE_PIXELS
        {
            return Err(format!("{path} has an unusable size.").into());
        }
        if pixels > self.budget {
            return Err("This preset's images are too large together.".into());
        }
        self.budget -= pixels;
        let image = ImageSurface::create_from_png(&mut Cursor::new(bytes))?;
        self.decoded.insert(path.to_owned(), image.clone());
        Ok(image)
    }
}

fn inside_package(location: &str) -> Result<String> {
    let path = location.replace('\\', "/");
    let relative = match path.get(..ASSET_ROOT.len()) {
        Some(root) if root.eq_ignore_ascii_case(ASSET_ROOT) => &path[ASSET_ROOT.len()..],
        _ => return Err(format!("{location} is not inside the preset.").into()),
    };
    let relative = relative.trim_start_matches('/');
    if relative.is_empty()
        || relative
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | "..") || part.contains(':'))
    {
        return Err(format!("{location} is not a valid preset path.").into());
    }
    Ok(relative.to_owned())
}

impl<R: Read + Seek> Assets for Package<R> {
    fn image(&mut self, location: &str) -> Result<ImageSurface> {
        self.decode(&inside_package(location)?)
    }

    fn folder(&mut self, location: &str) -> Result<Vec<ImageSurface>> {
        let prefix = format!("{}/", inside_package(location)?);
        let mut names: Vec<String> = self
            .entries
            .iter()
            .filter_map(|(name, _)| name.strip_prefix(&prefix).map(|rest| (name, rest)))
            .filter(|(_, rest)| !rest.contains('/') && rest.to_ascii_lowercase().ends_with(".png"))
            .map(|(name, _)| name.clone())
            .collect();
        names.sort();
        names.iter().map(|name| self.decode(name)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::{ZipWriter, write::SimpleFileOptions};

    fn png() -> Vec<u8> {
        let image = ImageSurface::create(cairo::Format::ARgb32, 3, 2).unwrap();
        let mut bytes = vec![];
        image.write_to_png(&mut bytes).unwrap();
        bytes
    }

    fn package(files: &[(&str, &[u8])]) -> Package<Cursor<Vec<u8>>> {
        let mut zip = ZipWriter::new(Cursor::new(vec![]));
        for (name, data) in files {
            zip.start_file(*name, SimpleFileOptions::default()).unwrap();
            zip.write_all(data).unwrap();
        }
        Package::open(Cursor::new(zip.finish().unwrap().into_inner())).unwrap()
    }

    #[test]
    fn reads_config_with_a_byte_order_mark() {
        let mut package = package(&[("Config.json", b"\xef\xbb\xbf{\"Name\":\"x\"}")]);
        assert_eq!(package.config().unwrap()["Name"], "x");
    }

    #[test]
    fn finds_images_with_either_slash_and_shares_decodes() {
        let png = png();
        let mut package = package(&[
            ("Logo\\a.png", &png),
            ("Logo/b.png", &png),
            ("Logo/notes.txt", b"hi"),
        ]);
        let a = package.image("%ShareXImageEffects%\\Logo\\a.png").unwrap();
        assert_eq!((a.width(), a.height()), (3, 2));
        assert_eq!(
            package.folder("%ShareXImageEffects%\\Logo").unwrap().len(),
            2
        );
        assert!(
            package
                .image("%ShareXImageEffects%\\Logo\\missing.png")
                .is_err()
        );
    }

    #[test]
    fn refuses_paths_that_leave_the_package() {
        let png = png();
        let mut package = package(&[("a.png", &png)]);
        for location in [
            "C:\\Windows\\a.png",
            "%ShareXImageEffects%\\..\\a.png",
            "%ShareXImageEffects%\\",
            "a.png",
        ] {
            assert!(package.image(location).is_err(), "{location}");
        }
    }

    #[test]
    fn rejects_files_that_are_not_pngs() {
        let mut package = package(&[("fake.png", b"GIF89a....................")]);
        assert!(package.image("%ShareXImageEffects%\\fake.png").is_err());
    }
}
