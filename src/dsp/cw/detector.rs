//! CW product detector (BFO → audible pitch).

use crate::source::Complex32;

use super::nco::ComplexNco;

/// Mix baseband to BFO pitch and emit the real (product) component.
#[derive(Clone, Debug)]
pub struct ProductDetector {
    bfo: ComplexNco,
}

impl Default for ProductDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl ProductDetector {
    pub fn new() -> Self {
        Self {
            bfo: ComplexNco::new(),
        }
    }

    pub fn reset_state(&mut self) {
        self.bfo.reset();
    }

    pub fn process(&mut self, sample: Complex32, bfo_hz: f32, sample_rate: f32) -> f32 {
        let mixed = self.bfo.mix_up(sample, bfo_hz, sample_rate);
        mixed.re
    }
}
