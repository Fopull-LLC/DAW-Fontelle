//! A model's in-graph noise, fed from a seed instead. The models here are
//! written byte by byte in ONNX's protobuf form and run in ONNX Runtime.

mod common;

use common::proto::*;
use fontelle_diffsinger::onnx::{externalize_noise, op_types};
use ort::session::Session;
use ort::value::Tensor;

fn run(bytes: &[u8], inputs: Vec<(String, ort::value::DynValue)>) -> Vec<f32> {
    let mut s = Session::builder()
        .unwrap()
        .commit_from_memory(bytes)
        .unwrap();
    let out = s.run(inputs).unwrap();
    let (_, y) = out["Y"].try_extract_tensor::<f32>().unwrap();
    y.to_vec()
}

fn zeros() -> ort::value::DynValue {
    Tensor::from_array(([2usize, 3], vec![0f32; 6]))
        .unwrap()
        .into_dyn()
}

fn pool(n: usize) -> ort::value::DynValue {
    Tensor::from_array(([n], (0..n).map(|i| i as f32 + 0.5).collect::<Vec<_>>()))
        .unwrap()
        .into_dyn()
}

#[test]
fn op_types_include_subgraphs() {
    let ops = op_types(&branching_model()).unwrap();
    assert!(ops.contains(&"If".to_string()));
    assert!(ops.contains(&"RandomNormalLike".to_string()));
    assert!(ops.contains(&"Identity".to_string()));
}

#[test]
fn a_model_without_noise_is_left_alone() {
    let m = model(&graph(
        &[node("Identity", &["X"], &["Y"], &[])],
        &[value_info("X", FLOAT, &[None, None])],
        &[value_info("Y", FLOAT, &[None, None])],
    ));
    assert_eq!(externalize_noise(&m).unwrap(), None);
}

#[test]
fn the_random_node_reads_the_pool_instead() {
    let patched = externalize_noise(&noisy_model()).unwrap().unwrap();
    assert_eq!(patched.noise_inputs.len(), 1);
    assert!(
        !op_types(&patched.bytes)
            .unwrap()
            .iter()
            .any(|o| o.starts_with("Random"))
    );
    let y = run(
        &patched.bytes,
        vec![
            ("X".into(), zeros()),
            (patched.noise_inputs[0].clone(), pool(10)),
        ],
    );
    assert_eq!(y, vec![0.5, 1.5, 2.5, 3.5, 4.5, 5.5]);
}

#[test]
fn noise_inside_a_branch_reads_the_pool_too() {
    let patched = externalize_noise(&branching_model()).unwrap().unwrap();
    assert_eq!(patched.noise_inputs.len(), 1);
    let cond = |b: bool| Tensor::from_array(((), vec![b])).unwrap().into_dyn();
    let y = run(
        &patched.bytes,
        vec![
            ("X".into(), zeros()),
            ("cond".into(), cond(true)),
            (patched.noise_inputs[0].clone(), pool(6)),
        ],
    );
    assert_eq!(y, vec![0.5, 1.5, 2.5, 3.5, 4.5, 5.5]);
    let y = run(
        &patched.bytes,
        vec![
            ("X".into(), zeros()),
            ("cond".into(), cond(false)),
            (patched.noise_inputs[0].clone(), pool(6)),
        ],
    );
    assert_eq!(y, vec![0.0; 6]);
}

#[test]
fn garbage_is_an_error_not_a_panic() {
    assert!(externalize_noise(&[0xff, 0xff, 0xff]).is_err());
    assert!(op_types(&[0x0a, 0x50, 0x01]).is_err());
}

/// A vocoder that draws its own noise draws it per sample, not per frame:
/// Canary DS's tgm_hifigan (RefineGAN) asks for samples x 9 harmonics, and
/// a frame's worth of 1024 values ran out ("Input shape:{156672},
/// requested shape:{1,78336,9}" for 153 frames at hop 512).
#[test]
fn a_vocoders_noise_pool_covers_every_sample_and_its_harmonics() {
    let frames = 153;
    let hop = 512;
    assert!(fontelle_diffsinger::render::vocoder_noise_pool(frames, hop) >= frames * hop * 9);
}

/// A duration or linguistic model may draw noise too: Yoko's duration
/// model asked for {1, 2, 4} and was fed a pool of one value. Its pool is
/// per phoneme, like a sampling model's per frame.
#[test]
fn a_phoneme_models_noise_pool_covers_its_phonemes() {
    assert!(fontelle_diffsinger::render::phoneme_noise_pool(2) >= 2 * 4);
    assert!(fontelle_diffsinger::render::phoneme_noise_pool(300) >= 300 * 256);
}
