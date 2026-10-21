//! The mix into a device's own sample format — WASAPI's exclusive mode
//! plays what the device plays, which is integers more often than floats.

use fontelle_engine::DeviceFormat;

fn bytes(format: DeviceFormat, samples: &[f32]) -> Vec<u8> {
    let mut out = vec![0xAA; samples.len() * format.bytes()];
    format.write(samples, &mut out);
    out
}

#[test]
fn each_format_writes_little_endian_full_scale_and_clips() {
    assert_eq!(bytes(DeviceFormat::F32, &[0.25]), 0.25f32.to_le_bytes());
    assert_eq!(
        bytes(DeviceFormat::I16, &[1.0, -1.0, 0.0]),
        [0xff, 0x7f, 0x01, 0x80, 0, 0]
    );
    assert_eq!(bytes(DeviceFormat::I24, &[0.5]), [0x00, 0x00, 0x40]);
    assert_eq!(
        bytes(DeviceFormat::I32, &[1.0]),
        0x7fff_ffffi32.to_le_bytes()
    );
    // 24 bits carried in 32: the top three bytes, the bottom one zero.
    assert_eq!(
        bytes(DeviceFormat::I24In32, &[1.0]),
        (8_388_607i32 << 8).to_le_bytes()
    );
    assert_eq!(
        bytes(DeviceFormat::I16, &[3.0, -3.0, f32::NAN]),
        [0xff, 0x7f, 0x01, 0x80, 0, 0]
    );
    assert_eq!(
        bytes(DeviceFormat::F32, &[2.0]),
        1.0f32.to_le_bytes(),
        "floats clip too"
    );
}

#[test]
fn sizes_and_a_short_destination() {
    assert_eq!(DeviceFormat::F32.bytes(), 4);
    assert_eq!(DeviceFormat::I16.bytes(), 2);
    assert_eq!(DeviceFormat::I24.bytes(), 3);
    assert_eq!(DeviceFormat::I24In32.bytes(), 4);
    assert_eq!(DeviceFormat::I32.bytes(), 4);
    // Only as many samples as fit.
    let mut out = [0u8; 4];
    DeviceFormat::I16.write(&[0.0, 0.0, 1.0], &mut out);
    assert_eq!(out, [0, 0, 0, 0]);
}

/// What every output path hands the device: WASAPI's bytes had this, and
/// cpal's floats went out raw — a NaN after the plugin's own check (an
/// effect blown up by what it was fed) went to the sound server as
/// whatever it made of it. Reported as white noise, 2026-10-20.
#[test]
fn a_sample_for_the_device_is_a_number_inside_full_scale() {
    use fontelle_engine::device_sample;
    assert_eq!(device_sample(f32::NAN), 0.0);
    assert_eq!(device_sample(f32::INFINITY), 1.0);
    assert_eq!(device_sample(f32::NEG_INFINITY), -1.0);
    assert_eq!(device_sample(2.5), 1.0);
    assert_eq!(device_sample(-0.25), -0.25);
}
