//! A little protobuf writer, enough to build tiny ONNX models by hand.
#![allow(dead_code)]

pub fn varint(mut v: u64, out: &mut Vec<u8>) {
    loop {
        let b = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(b);
            return;
        }
        out.push(b | 0x80);
    }
}

pub fn bytes_field(no: u64, data: &[u8], out: &mut Vec<u8>) {
    varint(no << 3 | 2, out);
    varint(data.len() as u64, out);
    out.extend_from_slice(data);
}

pub fn int_field(no: u64, v: u64, out: &mut Vec<u8>) {
    varint(no << 3, out);
    varint(v, out);
}

pub fn node(op: &str, inputs: &[&str], outputs: &[&str], attributes: &[Vec<u8>]) -> Vec<u8> {
    let mut n = Vec::new();
    for i in inputs {
        bytes_field(1, i.as_bytes(), &mut n);
    }
    for o in outputs {
        bytes_field(2, o.as_bytes(), &mut n);
    }
    bytes_field(3, format!("node_{op}_{}", outputs[0]).as_bytes(), &mut n);
    bytes_field(4, op.as_bytes(), &mut n);
    for a in attributes {
        bytes_field(5, a, &mut n);
    }
    n
}

pub fn graph_attribute(name: &str, graph: &[u8]) -> Vec<u8> {
    let mut a = Vec::new();
    bytes_field(1, name.as_bytes(), &mut a);
    bytes_field(6, graph, &mut a);
    int_field(20, 5, &mut a);
    a
}

/// A tensor value: `dims` of `None` are symbolic.
pub fn value_info(name: &str, elem_type: u64, dims: &[Option<i64>]) -> Vec<u8> {
    let mut shape = Vec::new();
    for (i, d) in dims.iter().enumerate() {
        let mut dim = Vec::new();
        match d {
            Some(v) => int_field(1, *v as u64, &mut dim),
            None => bytes_field(2, format!("{name}_{i}").as_bytes(), &mut dim),
        }
        bytes_field(1, &dim, &mut shape);
    }
    let mut tensor = Vec::new();
    int_field(1, elem_type, &mut tensor);
    bytes_field(2, &shape, &mut tensor);
    let mut ty = Vec::new();
    bytes_field(1, &tensor, &mut ty);
    let mut v = Vec::new();
    bytes_field(1, name.as_bytes(), &mut v);
    bytes_field(2, &ty, &mut v);
    v
}

pub fn graph(nodes: &[Vec<u8>], inputs: &[Vec<u8>], outputs: &[Vec<u8>]) -> Vec<u8> {
    let mut g = Vec::new();
    for n in nodes {
        bytes_field(1, n, &mut g);
    }
    bytes_field(2, b"g", &mut g);
    for i in inputs {
        bytes_field(11, i, &mut g);
    }
    for o in outputs {
        bytes_field(12, o, &mut g);
    }
    g
}

pub fn model(graph: &[u8]) -> Vec<u8> {
    let mut m = Vec::new();
    int_field(1, 8, &mut m); // ir_version
    bytes_field(7, graph, &mut m);
    let mut opset = Vec::new();
    bytes_field(1, b"", &mut opset);
    int_field(2, 17, &mut opset);
    bytes_field(8, &opset, &mut m);
    m
}

pub const FLOAT: u64 = 1;
pub const BOOL: u64 = 9;

pub const INT64: u64 = 7;

/// Y = RandomNormalLike(X) + X.
pub fn noisy_model() -> Vec<u8> {
    model(&graph(
        &[
            node("RandomNormalLike", &["X"], &["R"], &[]),
            node("Add", &["R", "X"], &["Y"], &[]),
        ],
        &[value_info("X", FLOAT, &[None, None])],
        &[value_info("Y", FLOAT, &[None, None])],
    ))
}

/// Y = cond ? RandomNormalLike(X) : X, the noise inside a branch.
pub fn branching_model() -> Vec<u8> {
    let then_branch = graph(
        &[node("RandomNormalLike", &["X"], &["R"], &[])],
        &[],
        &[value_info("R", FLOAT, &[None, None])],
    );
    let else_branch = graph(
        &[node("Identity", &["X"], &["I"], &[])],
        &[],
        &[value_info("I", FLOAT, &[None, None])],
    );
    model(&graph(
        &[node(
            "If",
            &["cond"],
            &["Y"],
            &[
                graph_attribute("then_branch", &then_branch),
                graph_attribute("else_branch", &else_branch),
            ],
        )],
        &[
            value_info("X", FLOAT, &[None, None]),
            value_info("cond", BOOL, &[]),
        ],
        &[value_info("Y", FLOAT, &[None, None])],
    ))
}
