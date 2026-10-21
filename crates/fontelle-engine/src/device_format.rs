//! A device's own sample format, for the backends that write it themselves
//! (WASAPI's exclusive mode plays what the device plays). The conversion
//! runs on the audio thread: arithmetic on slices, nothing else.

/// What a device plays, little-endian.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceFormat {
    F32,
    I16,
    /// Packed, three bytes a sample.
    I24,
    /// 24 significant bits in a 32-bit container, high-aligned.
    I24In32,
    I32,
}

/// A sample as a device may be handed it: a number, inside full scale.
///
/// Every output path goes through this, cpal's floats too: a NaN or an
/// infinity made after a plugin's own check (an effect blown up by what it
/// was fed) went to the sound server as whatever it made of it.
pub fn device_sample(x: f32) -> f32 {
    if x.is_nan() { 0.0 } else { x.clamp(-1.0, 1.0) }
}

impl DeviceFormat {
    pub fn bytes(self) -> usize {
        match self {
            Self::I16 => 2,
            Self::I24 => 3,
            Self::F32 | Self::I24In32 | Self::I32 => 4,
        }
    }

    /// Writes `samples` into `dst` in this format, clipped to ±1, as many
    /// as fit.
    pub fn write(self, samples: &[f32], dst: &mut [u8]) {
        let width = self.bytes();
        for (out, x) in dst.chunks_exact_mut(width).zip(samples) {
            let x = device_sample(*x);
            match self {
                Self::F32 => out.copy_from_slice(&x.to_le_bytes()),
                Self::I16 => {
                    out.copy_from_slice(&((f64::from(x) * 32_767.0).round() as i16).to_le_bytes())
                }
                Self::I24 => {
                    let v = (f64::from(x) * 8_388_607.0).round() as i32;
                    out.copy_from_slice(&v.to_le_bytes()[..3]);
                }
                Self::I24In32 => {
                    let v = ((f64::from(x) * 8_388_607.0).round() as i32) << 8;
                    out.copy_from_slice(&v.to_le_bytes());
                }
                Self::I32 => out.copy_from_slice(
                    &((f64::from(x) * 2_147_483_647.0).round() as i32).to_le_bytes(),
                ),
            }
        }
    }
}
