use std::cell::{Cell, RefCell};

use gtk::{gdk, glib, prelude::*};

const BASE_STYLE: &str = include_str!("style.css");
const CONTROL_STYLE: &str = include_str!("controls.css");
const MIN_FACTOR: f64 = 0.5;
const MAX_FACTOR: f64 = 2.;
const NEUTRAL_FACTOR: f64 = 1.;
const NEUTRAL_TOLERANCE: f64 = 1e-3;
const DEFAULT_DPI: i32 = 96 * 1024;

type Follower = Box<dyn Fn(f64) -> bool>;

thread_local! {
    static FACTOR: Cell<f64> = const { Cell::new(NEUTRAL_FACTOR) };
    static BASE_DPI: Cell<Option<i32>> = const { Cell::new(None) };
    static STYLE: RefCell<Option<gtk::CssProvider>> = const { RefCell::new(None) };
    static FOLLOWERS: RefCell<Vec<Follower>> = const { RefCell::new(Vec::new()) };
}

pub fn factor() -> f64 {
    FACTOR.with(Cell::get)
}

pub fn scaled(pixels: i32) -> i32 {
    scaled_by(pixels, factor())
}

pub fn scaled_by(pixels: i32, factor: f64) -> i32 {
    (f64::from(pixels) * factor).round() as i32
}

pub fn follow<W: IsA<glib::Object>>(widget: &W, update: impl Fn(&W, f64) + 'static) {
    update(widget, factor());
    let weak = widget.downgrade();
    let follower = move |factor: f64| match weak.upgrade() {
        Some(widget) => {
            update(&widget, factor);
            true
        }
        None => false,
    };
    FOLLOWERS.with(|followers| followers.borrow_mut().push(Box::new(follower)));
}

pub fn apply(requested: f64) {
    let factor = requested.clamp(MIN_FACTOR, MAX_FACTOR);
    FACTOR.with(|current| current.set(factor));
    apply_text_scale(factor);
    apply_style(factor);
    FOLLOWERS.with(|followers| followers.borrow_mut().retain(|follower| follower(factor)));
}

fn apply_text_scale(factor: f64) {
    let Some(settings) = gtk::Settings::default() else {
        return;
    };
    let base = BASE_DPI.with(|base| match base.get() {
        Some(known) => known,
        None => {
            let reported = settings.gtk_xft_dpi();
            let measured = if reported > 0 { reported } else { DEFAULT_DPI };
            base.set(Some(measured));
            measured
        }
    });
    settings.set_gtk_xft_dpi(scaled_by(base, factor));
}

fn apply_style(factor: f64) {
    let Some(display) = gdk::Display::default() else {
        return;
    };
    let provider = gtk::CssProvider::new();
    provider.load_from_string(&stylesheet(factor));
    STYLE.with(|current| {
        if let Some(previous) = current.replace(Some(provider.clone())) {
            gtk::style_context_remove_provider_for_display(&display, &previous);
        }
    });
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

fn stylesheet(factor: f64) -> String {
    if (factor - NEUTRAL_FACTOR).abs() < NEUTRAL_TOLERANCE {
        return BASE_STYLE.to_owned();
    }
    scaled_css(&format!("{CONTROL_STYLE}{BASE_STYLE}"), factor)
}

fn scaled_css(css: &str, factor: f64) -> String {
    let mut out = String::with_capacity(css.len());
    let bytes = css.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit()
            && (i == 0 || !bytes[i - 1].is_ascii_alphanumeric() && bytes[i - 1] != b'#')
        {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                i += 1;
            }
            if css[i..].starts_with("px") {
                let v: f64 = css[start..i].parse().unwrap_or(0.);
                out.push_str(&format!("{:.2}", v * factor));
            } else {
                out.push_str(&css[start..i]);
            }
        } else {
            let ch = css[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn css_pixel_sizes_follow_the_interface_scale() {
        let css = scaled_css(
            ".a { min-width: 34px; color: #4bd996; margin: 0 .5em; opacity: .15; }",
            1.5,
        );
        assert!(css.contains("min-width: 51.00px"));
        assert!(css.contains("#4bd996") && css.contains(".5em") && css.contains(".15"));
    }

    #[test]
    fn neutral_factor_keeps_the_theme_controls_untouched() {
        assert_eq!(stylesheet(1.), BASE_STYLE);
        assert!(stylesheet(1.5).len() > BASE_STYLE.len());
    }

    #[test]
    fn controls_scale_with_the_factor() {
        assert!(stylesheet(2.).contains("min-height: 48.00px"));
    }

    #[test]
    fn pixel_sizes_round_to_whole_pixels() {
        assert_eq!(scaled_by(61, 1.5), 92);
        assert_eq!(scaled_by(20, 0.5), 10);
    }
}
