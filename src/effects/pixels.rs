use super::Bitmap;
use crate::capture::Result;
use cairo::{Format, ImageSurface};

#[derive(Clone)]
pub struct Pixels {
    pub width: usize,
    pub height: usize,
    pub data: Vec<u8>,
}

pub fn alpha(pixel: u32) -> u32 {
    pixel >> 24
}

pub fn unpremultiply(pixel: u32) -> [u8; 4] {
    let a = alpha(pixel);
    let channel = |shift: u32| {
        let value = (pixel >> shift) & 0xff;
        (value * 255 + a / 2)
            .checked_div(a)
            .map_or(0, |scaled| scaled.min(255) as u8)
    };
    [a as u8, channel(16), channel(8), channel(0)]
}

pub fn premultiply([a, r, g, b]: [u8; 4]) -> u32 {
    let scale = |value: u8| (u32::from(value) * u32::from(a) + 127) / 255;
    u32::from(a) << 24 | scale(r) << 16 | scale(g) << 8 | scale(b)
}

impl Pixels {
    pub fn read(surface: &ImageSurface) -> Result<Self> {
        let (width, height) = (surface.width() as usize, surface.height() as usize);
        let stride = surface.stride() as usize;
        let mut data = Vec::with_capacity(width * height * 4);
        surface.with_data(|source| {
            for row in source.chunks(stride).take(height) {
                data.extend_from_slice(&row[..width * 4]);
            }
        })?;
        Ok(Self {
            width,
            height,
            data,
        })
    }
    pub fn from_bitmap(bitmap: &Bitmap) -> Result<Self> {
        Self::read(&bitmap.surface)
    }
    pub fn into_surface(self) -> Result<ImageSurface> {
        let stride = (self.width * 4) as i32;
        Ok(ImageSurface::create_for_data(
            self.data,
            Format::ARgb32,
            self.width as i32,
            self.height as i32,
            stride,
        )?)
    }
    pub fn get(&self, x: usize, y: usize) -> u32 {
        let at = (y * self.width + x) * 4;
        u32::from_ne_bytes(self.data[at..at + 4].try_into().unwrap())
    }
    pub fn set(&mut self, x: usize, y: usize, pixel: u32) {
        let at = (y * self.width + x) * 4;
        self.data[at..at + 4].copy_from_slice(&pixel.to_ne_bytes());
    }
    pub fn map(&mut self, mut f: impl FnMut(u32) -> u32) {
        let (chunks, _) = self.data.as_chunks_mut::<4>();
        for bytes in chunks {
            *bytes = f(u32::from_ne_bytes(*bytes)).to_ne_bytes();
        }
    }
    pub fn padded(&self, margin: usize) -> Self {
        let width = self.width + 2 * margin;
        let mut data = vec![0; width * (self.height + 2 * margin) * 4];
        for (y, row) in self.data.chunks_exact(self.width * 4).enumerate() {
            let at = ((y + margin) * width + margin) * 4;
            data[at..at + row.len()].copy_from_slice(row);
        }
        Self {
            width,
            height: self.height + 2 * margin,
            data,
        }
    }
    pub fn scale_alpha(&mut self, factor: f64) {
        self.map(|pixel| {
            let a = (f64::from(alpha(pixel)) * factor).round().clamp(0., 255.) as u32;
            a << 24
        });
    }
}

pub fn box_blur(pixels: &mut Pixels, range: usize) {
    if range <= 1 {
        return;
    }
    let half = range / 2;
    let (w, h) = (pixels.width, pixels.height);
    for _ in 0..2 {
        box_pass(&mut pixels.data, w, h, 4, w * 4, half);
        box_pass(&mut pixels.data, h, w, w * 4, 4, half);
    }
}

fn box_pass(data: &mut [u8], len: usize, lines: usize, step: usize, line_step: usize, half: usize) {
    let mut out = vec![0u8; len * 4];
    for line in 0..lines {
        let base = line * line_step;
        for channel in 0..4 {
            let at = |i: usize| u32::from(data[base + i * step + channel]);
            let (mut sum, mut low, mut high) = (0u32, 0usize, 0usize);
            for x in 0..len {
                let new_high = (x + half + 1).min(len);
                while high < new_high {
                    sum += at(high);
                    high += 1;
                }
                let new_low = x.saturating_sub(half);
                while low < new_low {
                    sum -= at(low);
                    low += 1;
                }
                out[x * 4 + channel] = (sum / (high - low) as u32) as u8;
            }
        }
        for x in 0..len {
            data[base + x * step..base + x * step + 4].copy_from_slice(&out[x * 4..x * 4 + 4]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(values: &[u8]) -> Pixels {
        Pixels {
            width: values.len(),
            height: 1,
            data: values.iter().flat_map(|&v| [v; 4]).collect(),
        }
    }

    #[test]
    fn box_blur_keeps_flat_areas_and_spreads_edges() {
        let mut flat = row(&[90; 9]);
        box_blur(&mut flat, 5);
        assert!(flat.data.iter().all(|&v| v == 90));
        let mut edge = row(&[0, 0, 0, 0, 255, 255, 255, 255]);
        box_blur(&mut edge, 3);
        let first = edge.data[0];
        let middle = edge.data[4 * 4];
        assert!(first < 20 && middle > 100 && middle < 200);
    }

    #[test]
    fn premultiply_round_trips_opaque_and_clear_pixels() {
        assert_eq!(premultiply([255, 10, 20, 30]), 0xff0a141e);
        assert_eq!(unpremultiply(0xff0a141e), [255, 10, 20, 30]);
        assert_eq!(unpremultiply(0), [0, 0, 0, 0]);
    }
}
