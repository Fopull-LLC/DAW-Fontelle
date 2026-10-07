//! Running models: feeding what they declare, shaping scalars, seeded
//! noise, cancelling.

mod common;

use common::proto::*;
use fontelle_diffsinger::models::{Feed, Model, Out, RuntimeOptions};
use fontelle_sing::SingError;

fn never() -> bool {
    false
}

fn steps_model() -> Vec<u8> {
    // Y = Identity(steps), steps a scalar.
    model(&graph(
        &[node("Identity", &["steps"], &["Y"], &[])],
        &[value_info("steps", INT64, &[])],
        &[value_info("Y", INT64, &[])],
    ))
}

#[test]
fn only_declared_inputs_are_fed_and_missing_ones_are_named() {
    let m = Model::from_bytes(&steps_model(), "steps", &RuntimeOptions::default()).unwrap();
    assert!(m.has_input("steps"));
    assert!(!m.has_input("depth"));
    let out = m
        .run(
            vec![
                ("steps", Feed::I64(vec![1], vec![5])),
                ("depth", Feed::F32(vec![1], vec![0.6])),
            ],
            None,
            &never,
        )
        .unwrap();
    // `[1]` was shaped to the scalar the model declares.
    assert_eq!(out["Y"], Out::I64(vec![], vec![5]));
    let e = m.run(vec![], None, &never).unwrap_err();
    match e {
        SingError::Failed(why) => assert!(why.contains("steps"), "{why}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn noise_follows_the_seed() {
    let m = Model::from_bytes(&noisy_model(), "noisy", &RuntimeOptions::default()).unwrap();
    assert_eq!(m.noise_inputs().len(), 1);
    let run = |seed| {
        let out = m
            .run(
                vec![("X", Feed::F32(vec![4, 8], vec![0.0; 32]))],
                Some((seed, 64)),
                &never,
            )
            .unwrap();
        out["Y"].f32().unwrap().to_vec()
    };
    let a = run(1);
    assert_eq!(a, run(1));
    assert_ne!(a, run(2));
    assert_eq!(a.len(), 32);
    let mean = a.iter().sum::<f32>() / 32.0;
    assert!(mean.abs() < 0.6, "{a:?}");
    assert!(a.iter().any(|v| *v < 0.0) && a.iter().any(|v| *v > 0.0));
}

#[test]
fn a_raised_flag_cancels() {
    let m = Model::from_bytes(&steps_model(), "steps", &RuntimeOptions::default()).unwrap();
    let e = m
        .run(vec![("steps", Feed::I64(vec![], vec![1]))], None, &|| true)
        .unwrap_err();
    assert_eq!(e, SingError::Cancelled);
}

#[test]
fn a_file_that_is_not_a_model_is_an_error() {
    assert!(Model::from_bytes(b"nonsense", "x", &RuntimeOptions::default()).is_err());
}

#[test]
fn a_graph_can_be_left_as_it_is() {
    let options = RuntimeOptions {
        leave_graphs_unchanged: true,
        ..RuntimeOptions::default()
    };
    let m = Model::from_bytes(&noisy_model(), "noisy", &options).unwrap();
    assert!(m.noise_inputs().is_empty());
    let out = m
        .run(
            vec![("X", Feed::F32(vec![2, 2], vec![0.0; 4]))],
            Some((1, 64)),
            &never,
        )
        .unwrap();
    assert_eq!(out["Y"].f32().unwrap().len(), 4);
}
