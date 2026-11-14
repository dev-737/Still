use cairo::Context;
use std::f64::consts::{FRAC_PI_2, PI};

pub fn rounded_rect(c: &Context, x: f64, y: f64, w: f64, h: f64, radius: f64) {
    let r = radius.min(w / 2.).min(h / 2.).max(0.);
    c.new_path();
    if r == 0. {
        c.rectangle(x, y, w, h);
        return;
    }
    c.arc(x + w - r, y + r, r, -FRAC_PI_2, 0.);
    c.arc(x + w - r, y + h - r, r, 0., FRAC_PI_2);
    c.arc(x + r, y + h - r, r, FRAC_PI_2, PI);
    c.arc(x + r, y + r, r, PI, PI * 1.5);
    c.close_path();
}

pub fn fits(size: (i32, i32), (x, y, w, h): (i32, i32, i32, i32)) -> bool {
    x >= 0 && y >= 0 && x + w <= size.0 && y + h <= size.1
}
