use super::{Bitmap, Effect, Point, rng::Rng, value::Sides};
use crate::capture::Result;
use cairo::Context;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct TornEdge {
    depth: i32,
    range: i32,
    sides: Sides,
    curved_edges: bool,
}

impl Default for TornEdge {
    fn default() -> Self {
        Self {
            depth: 15,
            range: 20,
            sides: Sides::default(),
            curved_edges: true,
        }
    }
}

impl TornEdge {
    fn outline(&self, w: i32, h: i32) -> Vec<(i32, i32)> {
        let (d, r, sides) = (self.depth, self.range, self.sides);
        let (across, down) = (w / r, h / r);
        let mut rng = Rng::seeded(0x9e37_79b9 ^ (w as u64) << 20 ^ h as u64);
        let mut points = vec![];
        if sides.top && across > 1 {
            let start = if sides.left && down > 1 { d } else { 0 };
            let end = if sides.right && down > 1 { w - d } else { w };
            points.extend(
                (start..end)
                    .step_by(r as usize)
                    .map(|x| (x, rng.range(0, d))),
            );
        } else {
            points.extend([(0, 0), (w, 0)]);
        }
        if sides.right && down > 1 {
            let start = if sides.top && across > 1 { d } else { 0 };
            let end = if sides.bottom && across > 1 { h - d } else { h };
            points.extend(
                (start..end)
                    .step_by(r as usize)
                    .map(|y| (w - d + rng.range(0, d), y)),
            );
        } else {
            points.extend([(w, 0), (w, h)]);
        }
        if sides.bottom && across > 1 {
            let mut x = if sides.right && down > 1 { w - d } else { w };
            let end = if sides.left && down > 1 { d } else { 0 };
            while x > end {
                points.push((x, h - d + rng.range(0, d)));
                x = (x / r - 1) * r;
            }
        } else {
            points.extend([(w, h), (0, h)]);
        }
        if sides.left && down > 1 {
            let mut y = if sides.bottom && across > 1 { h - d } else { h };
            let end = if sides.top && across > 1 { d } else { 0 };
            while y > end {
                points.push((rng.range(0, d), y));
                y = (y / r - 1) * r;
            }
        } else {
            points.extend([(0, h), (0, 0)]);
        }
        let mut unique = Vec::with_capacity(points.len());
        for point in points {
            if !unique.contains(&point) {
                unique.push(point);
            }
        }
        unique
    }
}

fn smooth_path(c: &Context, points: &[Point]) {
    let n = points.len();
    let at = |i: usize| points[i % n];
    c.move_to(at(0).0, at(0).1);
    for i in 0..n {
        let (p0, p1, p2, p3) = (at(i + n - 1), at(i), at(i + 1), at(i + 2));
        c.curve_to(
            p1.0 + (p2.0 - p0.0) / 6.,
            p1.1 + (p2.1 - p0.1) / 6.,
            p2.0 - (p3.0 - p1.0) / 6.,
            p2.1 - (p3.1 - p1.1) / 6.,
            p2.0,
            p2.1,
        );
    }
    c.close_path();
}

impl Effect for TornEdge {
    fn apply(&self, input: Bitmap) -> Result<Bitmap> {
        let (w, h) = input.size();
        if self.depth < 1
            || self.range < 1
            || self.sides.is_empty()
            || (w / self.range < 2 && h / self.range < 2)
        {
            return Ok(input);
        }
        let outline: Vec<Point> = self
            .outline(w, h)
            .into_iter()
            .map(|(x, y)| (f64::from(x), f64::from(y)))
            .collect();
        if outline.len() < 3 {
            return Ok(input);
        }
        let out = input.blank_like()?;
        let c = Context::new(&out.surface)?;
        if self.curved_edges {
            smooth_path(&c, &outline);
        } else {
            for (i, (x, y)) in outline.iter().enumerate() {
                if i == 0 {
                    c.move_to(*x, *y);
                } else {
                    c.line_to(*x, *y);
                }
            }
            c.close_path();
        }
        c.clip();
        input.paint(&c, 0., 0.)?;
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::tests::{RED, pixel, solid};

    #[test]
    fn tears_the_edges_but_keeps_the_middle_and_is_reproducible() {
        let torn = TornEdge::default();
        let a = torn.apply(solid(200, 120, RED)).unwrap();
        assert_eq!(a.size(), (200, 120));
        assert_eq!(pixel(&a, 100, 60), 0xffff0000);
        assert_eq!(pixel(&a, 0, 0), 0);
        assert_eq!(torn.outline(200, 120), torn.outline(200, 120));
    }

    #[test]
    fn tiny_images_are_left_alone() {
        let out = TornEdge::default().apply(solid(10, 10, RED)).unwrap();
        assert_eq!(pixel(&out, 0, 0), 0xffff0000);
    }
}
