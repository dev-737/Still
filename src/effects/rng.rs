pub struct Rng(u64);

impl Rng {
    pub fn seeded(seed: u64) -> Self {
        Self(seed | 1)
    }
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    pub fn range(&mut self, low: i32, high: i32) -> i32 {
        if high <= low {
            return low;
        }
        low + (self.next() >> 33) as i32 % (high - low)
    }
    pub fn pick(&mut self, len: usize) -> usize {
        self.range(0, len as i32) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_reproducible_and_in_range() {
        let (mut a, mut b) = (Rng::seeded(7), Rng::seeded(7));
        for _ in 0..100 {
            let value = a.range(-5, 5);
            assert_eq!(value, b.range(-5, 5));
            assert!((-5..5).contains(&value));
        }
        assert_eq!(a.range(3, 3), 3);
    }
}
