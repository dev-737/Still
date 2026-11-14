mod capture;
mod effects;
mod frames;
mod picker;
mod render;
mod scaling;
mod sharex;
mod ui;

fn run() -> capture::Result<()> {
    let mut args = std::env::args_os().skip(1);
    let mut bytes = None;
    let mut preferences = false;
    match args.next().as_deref().and_then(|s| s.to_str()) {
        Some("--help" | "-h") => {
            println!(
                "Still - screenshot editor\n\nstill [IMAGE]\nstill --capture\nstill --last\nstill --settings"
            );
            return Ok(());
        }
        Some("--settings") => preferences = true,
        Some("--last") => {
            let path = capture::last_capture_path();
            if !path.exists() {
                return Err("No capture to reopen yet. Take one with still --capture.".into());
            }
            return ui::launch(None, Some(path), false);
        }
        Some("--capture") => {
            let (capture, errors) = capture::run_actions()?;
            for error in errors {
                capture::report(&error);
            }
            let Some(png) = capture else {
                return Ok(());
            };
            bytes = Some(png);
        }
        Some(path) => return ui::launch(None, Some(std::path::PathBuf::from(path)), false),
        None => {}
    }
    ui::launch(bytes, None, preferences)
}
fn main() {
    if let Err(error) = run() {
        capture::report(&error.to_string());
        std::process::exit(1);
    }
}
