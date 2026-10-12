//! Fontelle's `f32` into every format an ASIO driver may want, and back.

use fontelle_asio_host::convert::{SampleType, read_channel, write_channel};

const ALL: [(i32, SampleType, &str, usize); 18] = [
    (0, SampleType::Int16Msb, "Int16MSB", 2),
    (1, SampleType::Int24Msb, "Int24MSB", 3),
    (2, SampleType::Int32Msb, "Int32MSB", 4),
    (3, SampleType::Float32Msb, "Float32MSB", 4),
    (4, SampleType::Float64Msb, "Float64MSB", 8),
    (8, SampleType::Int32Msb16, "Int32MSB16", 4),
    (9, SampleType::Int32Msb18, "Int32MSB18", 4),
    (10, SampleType::Int32Msb20, "Int32MSB20", 4),
    (11, SampleType::Int32Msb24, "Int32MSB24", 4),
    (16, SampleType::Int16Lsb, "Int16LSB", 2),
    (17, SampleType::Int24Lsb, "Int24LSB", 3),
    (18, SampleType::Int32Lsb, "Int32LSB", 4),
    (19, SampleType::Float32Lsb, "Float32LSB", 4),
    (20, SampleType::Float64Lsb, "Float64LSB", 8),
    (24, SampleType::Int32Lsb16, "Int32LSB16", 4),
    (25, SampleType::Int32Lsb18, "Int32LSB18", 4),
    (26, SampleType::Int32Lsb20, "Int32LSB20", 4),
    (27, SampleType::Int32Lsb24, "Int32LSB24", 4),
];

#[test]
fn every_asio_sample_type_is_known_by_its_number_name_and_size() {
    for (number, ty, name, bytes) in ALL {
        assert_eq!(SampleType::from_asio(number), Some(ty), "{number}");
        assert_eq!(ty.name(), name);
        assert_eq!(ty.bytes(), bytes, "{name}");
    }
    // DSD, and numbers ASIO never gave out.
    for number in [5, 12, 21, 32, 33, 40, -1] {
        assert_eq!(SampleType::from_asio(number), None, "{number}");
    }
}

#[test]
fn a_round_trip_keeps_the_signal_to_the_formats_resolution() {
    let signal = [-1.0f32, -0.5, -0.001, 0.0, 0.25, 0.7, 0.999];
    for (_, ty, name, _) in ALL {
        let bits = match ty {
            SampleType::Int16Lsb | SampleType::Int16Msb => 15,
            SampleType::Int32Lsb16 | SampleType::Int32Msb16 => 15,
            SampleType::Int32Lsb18 | SampleType::Int32Msb18 => 17,
            SampleType::Int32Lsb20 | SampleType::Int32Msb20 => 19,
            SampleType::Int24Lsb
            | SampleType::Int24Msb
            | SampleType::Int32Lsb24
            | SampleType::Int32Msb24 => 23,
            _ => 24,
        };
        let step = 1.0 / (1u64 << bits) as f32;
        // Two channels interleaved; channel 1 written and read back.
        let interleaved: Vec<f32> = signal.iter().flat_map(|s| [9.0, *s]).collect();
        let mut bytes = vec![0u8; signal.len() * ty.bytes()];
        write_channel(ty, &interleaved, 2, 1, &mut bytes);
        let mut back = vec![5.0f32; signal.len() * 2];
        read_channel(ty, &bytes, &mut back, 2, 1);
        for (i, s) in signal.iter().enumerate() {
            assert!(
                (back[i * 2 + 1] - s).abs() <= step,
                "{name}: {s} came back as {}",
                back[i * 2 + 1]
            );
            assert_eq!(back[i * 2], 5.0, "{name}: the other channel is left alone");
        }
    }
}

#[test]
fn the_bytes_are_the_ones_the_driver_expects() {
    let one = |ty: SampleType, x: f32| {
        let mut b = vec![0u8; ty.bytes()];
        write_channel(ty, &[x], 1, 0, &mut b);
        b
    };
    assert_eq!(one(SampleType::Int16Lsb, 1.0), [0xff, 0x7f]);
    assert_eq!(one(SampleType::Int16Lsb, -1.0), [0x01, 0x80]);
    assert_eq!(one(SampleType::Int16Msb, 1.0), [0x7f, 0xff]);
    assert_eq!(one(SampleType::Int24Lsb, 0.5), [0x00, 0x00, 0x40]);
    assert_eq!(one(SampleType::Int24Msb, 0.5), [0x40, 0x00, 0x00]);
    assert_eq!(one(SampleType::Int32Lsb, 1.0), 0x7fff_ffffi32.to_le_bytes());
    assert_eq!(
        one(SampleType::Int32Lsb, -1.0),
        (-0x7fff_ffffi32).to_le_bytes()
    );
    assert_eq!(one(SampleType::Int32Msb, 0.0), [0, 0, 0, 0]);
    assert_eq!(one(SampleType::Float32Lsb, 0.25), 0.25f32.to_le_bytes());
    assert_eq!(one(SampleType::Float32Msb, 0.25), 0.25f32.to_be_bytes());
    assert_eq!(one(SampleType::Float64Lsb, 0.25), 0.25f64.to_le_bytes());
    assert_eq!(one(SampleType::Int32Lsb24, 1.0), 8_388_607i32.to_le_bytes());
    assert_eq!(
        one(SampleType::Int32Lsb16, -1.0),
        (-32_767i32).to_le_bytes()
    );
    assert_eq!(one(SampleType::Int32Lsb18, 1.0), 131_071i32.to_le_bytes());
    assert_eq!(one(SampleType::Int32Lsb20, 1.0), 524_287i32.to_le_bytes());
}

#[test]
fn too_loud_is_clipped_and_not_a_number_is_silence() {
    for (_, ty, name, _) in ALL {
        let mut b = vec![0u8; 3 * ty.bytes()];
        write_channel(ty, &[2.0, -3.0, f32::NAN], 1, 0, &mut b);
        let mut back = [0.0f32; 3];
        read_channel(ty, &b, &mut back, 1, 0);
        if matches!(
            ty,
            SampleType::Float32Lsb
                | SampleType::Float32Msb
                | SampleType::Float64Lsb
                | SampleType::Float64Msb
        ) {
            assert_eq!(back[0], 1.0, "{name}: floats are clipped too");
        }
        assert!((back[0] - 1.0).abs() < 1e-3, "{name}: {back:?}");
        assert!((back[1] + 1.0).abs() < 1e-3, "{name}: {back:?}");
        assert_eq!(back[2], 0.0, "{name}: {back:?}");
    }
}
