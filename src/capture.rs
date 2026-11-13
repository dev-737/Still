use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub copy: bool,
    pub save: bool,
    pub editor: bool,
    pub preview: bool,
    pub folder: PathBuf,
    pub filename: String,
}

fn home() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            copy: true,
            save: false,
            editor: false,
            preview: true,
            folder: home().join("Pictures/Screenshots"),
            filename: "Still-%Y-%m-%d_%H-%M-%S".into(),
        }
    }
}

pub fn config_dir() -> PathBuf {
    env::var_os("XDG_CONFIG_HOME")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".config"))
        .join("still")
}

impl Settings {
    pub fn load() -> Result<Self> {
        match fs::read(config_dir().join("settings.json")) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.into()),
        }
    }
    pub fn store(&self) -> Result<()> {
        if self.save {
            if !self.folder.is_absolute() {
                return Err("Use an absolute save folder.".into());
            }
            self.output_name()?;
        }
        let path = config_dir().join("settings.json");
        let directory = path.parent().unwrap();
        fs::create_dir_all(directory)?;
        let mut temp = tempfile::NamedTempFile::new_in(directory)?;
        serde_json::to_writer_pretty(&mut temp, self)?;
        temp.persist(path)?;
        Ok(())
    }
    fn output_name(&self) -> Result<String> {
        if self.filename.trim().is_empty() || self.filename.contains(['/', '\0', '\n']) {
            return Err("Enter a filename pattern without slashes or line breaks.".into());
        }
        let output = Command::new("date")
            .arg(format!("+{}", self.filename))
            .output()?;
        if !output.status.success() {
            return Err("Could not format the filename.".into());
        }
        let value = std::str::from_utf8(&output.stdout)?.trim_end_matches('\n');
        if value.trim().is_empty() || value.contains(['/', '\0', '\n', '\r']) {
            return Err("The filename pattern must produce a single filename.".into());
        }
        Ok(value.strip_suffix(".png").unwrap_or(value).to_owned())
    }
    pub fn save_capture(&self, png: &[u8]) -> Result<PathBuf> {
        fs::create_dir_all(&self.folder)?;
        let stem = self.output_name()?;
        let mut file = tempfile::NamedTempFile::new_in(&self.folder)?;
        file.write_all(png)?;
        for i in 0..1000 {
            let name = if i == 0 {
                format!("{stem}.png")
            } else {
                format!("{stem}-{i}.png")
            };
            let path = self.folder.join(name);
            match file.persist_noclobber(&path) {
                Ok(_) => return Ok(path),
                Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => file = e.file,
                Err(e) => return Err(e.error.into()),
            }
        }
        Err("Too many captures with the same filename.".into())
    }
}

pub fn cache_dir() -> PathBuf {
    env::var_os("XDG_CACHE_HOME")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".cache"))
        .join("still")
}

pub fn last_capture_path() -> PathBuf {
    cache_dir().join("last.png")
}

fn remember(png: &[u8]) -> Result<()> {
    let path = last_capture_path();
    fs::create_dir_all(path.parent().unwrap())?;
    png_to(&path, png)
}

pub fn run_actions() -> Result<(Option<Vec<u8>>, Vec<String>)> {
    let settings = Settings::load()?;
    if !settings.copy && !settings.save && !settings.editor && !settings.preview {
        return Err("Enable an after-capture action in Still Settings.".into());
    }
    let Some(raw) = region()? else {
        return Ok((None, Vec::new()));
    };
    let mut errors = Vec::new();
    let rendered = match crate::frames::render_capture(&raw) {
        Ok(image) => image,
        Err(error) => {
            errors.push(format!(
                "Default frame unavailable. Using the original capture: {error}"
            ));
            None
        }
    };
    let output = rendered.as_deref().unwrap_or(&raw);
    if let Err(error) = remember(&raw) {
        errors.push(format!("Could not remember this capture: {error}"));
    }
    if settings.copy {
        if let Err(error) = copy(output) {
            errors.push(format!("Clipboard: {error}"));
        }
    }
    if settings.save {
        match settings.save_capture(output) {
            Ok(path) => println!("{}", path.display()),
            Err(error) => errors.push(format!("Save: {error}")),
        }
    }
    let editor = should_open_editor(&settings, output).then_some(raw);
    Ok((editor, errors))
}

pub fn region() -> Result<Option<Vec<u8>>> {
    let selected = Command::new("slurp").output()?;
    if !selected.status.success() {
        if selected.stderr.is_empty() {
            return Ok(None);
        }
        return Err(String::from_utf8_lossy(&selected.stderr)
            .trim()
            .to_owned()
            .into());
    }
    let geometry = std::str::from_utf8(&selected.stdout)?.trim();
    if geometry.is_empty() {
        return Ok(None);
    }
    let png = Command::new("grim").args(["-g", geometry, "-"]).output()?;
    if !png.status.success() || !png.stdout.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err(format!(
            "grim could not capture the region: {}",
            String::from_utf8_lossy(&png.stderr).trim()
        )
        .into());
    }
    Ok(Some(png.stdout))
}

pub fn copy(png: &[u8]) -> Result<()> {
    let mut child = Command::new("wl-copy")
        .args(["--type", "image/png"])
        .stdin(Stdio::piped())
        .spawn()?;
    let result = child.stdin.take().unwrap().write_all(png);
    let status = child.wait()?;
    result?;
    if !status.success() {
        return Err("wl-copy could not set the clipboard.".into());
    }
    Ok(())
}

pub fn preview(png: &[u8]) -> Result<bool> {
    use gtk::gdk_pixbuf::{InterpType, Pixbuf};
    let image = Pixbuf::from_read(std::io::Cursor::new(png.to_vec()))?;
    let scale = (240. / f64::from(image.width()))
        .min(144. / f64::from(image.height()))
        .min(1.);
    let thumbnail = image
        .scale_simple(
            (f64::from(image.width()) * scale).round().max(1.) as i32,
            (f64::from(image.height()) * scale).round().max(1.) as i32,
            InterpType::Bilinear,
        )
        .ok_or("Could not create a screenshot thumbnail.")?;
    let mut file = tempfile::Builder::new()
        .prefix("still-preview-")
        .suffix(".png")
        .tempfile()?;
    file.write_all(&thumbnail.save_to_bufferv("png", &[])?)?;
    let output = Command::new("timeout")
        .args([
            "20s",
            "notify-send",
            "--app-name=Still",
            "--transient",
            "--expire-time=10000",
            "--action=default=Open in Still",
            "--action=edit=Open editor",
        ])
        .arg(format!(
            "--hint=string:image-path:{}",
            file.path().display()
        ))
        .args([
            "Screenshot captured",
            "Click to edit, or run still --last later",
        ])
        .output()?;
    if output.status.code() == Some(124) {
        return Ok(false);
    }
    if !output.status.success() {
        return Err("Could not show the screenshot preview.".into());
    }
    Ok(matches!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "default" | "edit"
    ))
}

pub fn should_open_editor(settings: &Settings, png: &[u8]) -> bool {
    settings.editor
        || (settings.preview
            && match preview(png) {
                Ok(open) => open,
                Err(error) => {
                    eprintln!("Still: {error}");
                    false
                }
            })
}

pub fn report(error: &str) {
    eprintln!("Still: {error}");
    let _ = Command::new("notify-send").args(["Still", error]).spawn();
}

pub fn png_to(path: &Path, png: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(png)?;
    file.persist(path)?;
    Ok(())
}
