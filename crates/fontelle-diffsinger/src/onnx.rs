//! Seeded noise for models that draw their own.
//!
//! A DiffSinger acoustic, pitch or variance model starts its diffusion or
//! flow from Gaussian noise drawn inside the graph (`RandomNormalLike`).
//! ONNX Runtime seeds that from the clock, once per session, and offers no
//! way to reseed it through its C API, so the same phrase would sing a
//! different take each time. Before a session is made, the model's graph is
//! read in memory and each random node is replaced by a slice of a new
//! input, a pool of noise this crate fills from the render's seed. Nothing
//! else in the graph, and nothing on disk, is touched: the weights are
//! copied through byte for byte.
//!
//! Some voicebanks' terms forbid modifying their model files. The files
//! are never written, and the weights reach ONNX Runtime unchanged; a host
//! that wants the graph run exactly as shipped sets
//! [`crate::models::RuntimeOptions::leave_graphs_unchanged`] and gives up
//! repeatable takes.
//!
//! Only the few protobuf fields this needs are read (ONNX's `ModelProto`,
//! `GraphProto`, `NodeProto`, `AttributeProto`); every other field is
//! copied as it was.

/// A model whose random nodes read from noise inputs.
#[derive(Debug, Clone, PartialEq)]
pub struct Patched {
    pub bytes: Vec<u8>,
    /// The inputs added, each a 1-D float pool of noise at least as long as
    /// the noise the node drew.
    pub noise_inputs: Vec<String>,
}

/// The ops this replaces.
pub const RANDOM_OPS: [&str; 2] = ["RandomNormalLike", "RandomNormal"];

/// Every node's op type, subgraphs included.
pub fn op_types(model: &[u8]) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut found_graph = false;
    for f in fields(model)? {
        if let (MODEL_GRAPH, Payload::Bytes(g)) = (f.no, f.payload) {
            found_graph = true;
            graph_ops(g, &mut out)?;
        }
    }
    if !found_graph {
        return Err("not an ONNX model: it has no graph".into());
    }
    Ok(out)
}

fn graph_ops(graph: &[u8], out: &mut Vec<String>) -> Result<(), String> {
    for f in fields(graph)? {
        if let (GRAPH_NODE, Payload::Bytes(n)) = (f.no, f.payload) {
            let node = Node::read(n)?;
            out.push(node.op_type.clone());
            for g in node.subgraphs()? {
                graph_ops(g, out)?;
            }
        }
    }
    Ok(())
}

/// The model with its random nodes fed from noise inputs; `None` when it
/// draws no noise.
pub fn externalize_noise(model: &[u8]) -> Result<Option<Patched>, String> {
    let ops = op_types(model)?;
    if !ops.iter().any(|o| RANDOM_OPS.contains(&o.as_str())) {
        return Ok(None);
    }
    let mut opset = None;
    for f in fields(model)? {
        if let (MODEL_OPSET, Payload::Bytes(o)) = (f.no, f.payload) {
            let mut domain = String::new();
            let mut version = 0;
            for g in fields(o)? {
                match (g.no, g.payload) {
                    (1, Payload::Bytes(d)) => domain = String::from_utf8_lossy(d).into_owned(),
                    (2, Payload::Varint(v)) => version = v,
                    _ => {}
                }
            }
            if domain.is_empty() || domain == "ai.onnx" {
                opset = Some(version);
            }
        }
    }
    // Slice takes its bounds as inputs from opset 10.
    if opset.is_some_and(|v| v < 10) {
        return Err(format!(
            "the model is for ONNX opset {}, too old to feed its noise",
            opset.unwrap_or(0)
        ));
    }
    let mut pools = Vec::new();
    let mut out = Vec::with_capacity(model.len() + 4096);
    for f in fields(model)? {
        match (f.no, f.payload) {
            (MODEL_GRAPH, Payload::Bytes(g)) => {
                let mut graph = rewrite_graph(g, &mut pools)?;
                for name in &pools {
                    put_bytes(GRAPH_INPUT, &pool_value_info(name), &mut graph);
                }
                put_bytes(MODEL_GRAPH, &graph, &mut out);
            }
            _ => out.extend_from_slice(f.raw),
        }
    }
    Ok(Some(Patched {
        bytes: out,
        noise_inputs: pools,
    }))
}

// --- the fields used ---

const MODEL_GRAPH: u64 = 7;
const MODEL_OPSET: u64 = 8;
const GRAPH_NODE: u64 = 1;
const GRAPH_INPUT: u64 = 11;
const NODE_INPUT: u64 = 1;
const NODE_OUTPUT: u64 = 2;
const NODE_NAME: u64 = 3;
const NODE_OP_TYPE: u64 = 4;
const NODE_ATTRIBUTE: u64 = 5;
const ATTR_NAME: u64 = 1;
const ATTR_F: u64 = 2;
const ATTR_I: u64 = 3;
const ATTR_T: u64 = 5;
const ATTR_G: u64 = 6;
const ATTR_INTS: u64 = 8;
const ATTR_GRAPHS: u64 = 11;
const ATTR_TYPE: u64 = 20;
const TYPE_INT: u64 = 2;
const TYPE_TENSOR: u64 = 4;
const ELEM_FLOAT: u64 = 1;
const ELEM_INT64: u64 = 7;

enum Payload<'a> {
    Varint(u64),
    Fixed32(u32),
    Fixed64,
    Bytes(&'a [u8]),
}

struct Field<'a> {
    no: u64,
    payload: Payload<'a>,
    /// The whole field as it was written, key included.
    raw: &'a [u8],
}

fn read_varint(data: &[u8], at: &mut usize) -> Result<u64, String> {
    let mut v = 0u64;
    for shift in (0..64).step_by(7) {
        let b = *data.get(*at).ok_or("truncated protobuf varint")?;
        *at += 1;
        v |= u64::from(b & 0x7f) << shift;
        if b & 0x80 == 0 {
            return Ok(v);
        }
    }
    Err("protobuf varint too long".into())
}

fn fields(data: &[u8]) -> Result<Vec<Field<'_>>, String> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < data.len() {
        let begin = at;
        let key = read_varint(data, &mut at)?;
        let no = key >> 3;
        let payload = match key & 7 {
            0 => Payload::Varint(read_varint(data, &mut at)?),
            1 => {
                let b = data.get(at..at + 8).ok_or("truncated protobuf field")?;
                at += 8;
                let _ = b;
                Payload::Fixed64
            }
            2 => {
                let len = usize::try_from(read_varint(data, &mut at)?)
                    .map_err(|_| "protobuf length too large")?;
                let end = at.checked_add(len).ok_or("protobuf length too large")?;
                let b = data.get(at..end).ok_or("truncated protobuf field")?;
                at = end;
                Payload::Bytes(b)
            }
            5 => {
                let b = data.get(at..at + 4).ok_or("truncated protobuf field")?;
                at += 4;
                Payload::Fixed32(u32::from_le_bytes(b.try_into().unwrap_or_default()))
            }
            w => return Err(format!("unsupported protobuf wire type {w}")),
        };
        if no == 0 {
            return Err("protobuf field number 0".into());
        }
        out.push(Field {
            no,
            payload,
            raw: &data[begin..at],
        });
    }
    Ok(out)
}

fn put_varint(mut v: u64, out: &mut Vec<u8>) {
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

fn put_bytes(no: u64, data: &[u8], out: &mut Vec<u8>) {
    put_varint(no << 3 | 2, out);
    put_varint(data.len() as u64, out);
    out.extend_from_slice(data);
}

fn put_int(no: u64, v: u64, out: &mut Vec<u8>) {
    put_varint(no << 3, out);
    put_varint(v, out);
}

/// What a node is, as far as this module cares.
struct Node<'a> {
    op_type: String,
    inputs: Vec<String>,
    outputs: Vec<String>,
    /// Each attribute's whole encoding.
    attributes: Vec<&'a [u8]>,
}

impl<'a> Node<'a> {
    fn read(data: &'a [u8]) -> Result<Self, String> {
        let mut n = Node {
            op_type: String::new(),
            inputs: Vec::new(),
            outputs: Vec::new(),
            attributes: Vec::new(),
        };
        for f in fields(data)? {
            let Payload::Bytes(b) = f.payload else {
                continue;
            };
            let text = || String::from_utf8_lossy(b).into_owned();
            match f.no {
                NODE_INPUT => n.inputs.push(text()),
                NODE_OUTPUT => n.outputs.push(text()),
                NODE_OP_TYPE => n.op_type = text(),
                NODE_ATTRIBUTE => n.attributes.push(b),
                _ => {}
            }
        }
        Ok(n)
    }

    fn subgraphs(&self) -> Result<Vec<&'a [u8]>, String> {
        let mut out = Vec::new();
        for a in &self.attributes {
            for f in fields(a)? {
                if let (ATTR_G | ATTR_GRAPHS, Payload::Bytes(g)) = (f.no, f.payload) {
                    out.push(g);
                }
            }
        }
        Ok(out)
    }

    fn attribute(&self, name: &str) -> Result<Option<Vec<Field<'a>>>, String> {
        for a in &self.attributes {
            let f = fields(a)?;
            let named = f
                .iter()
                .any(|x| matches!((x.no, &x.payload), (ATTR_NAME, Payload::Bytes(n)) if *n == name.as_bytes()));
            if named {
                return Ok(Some(f));
            }
        }
        Ok(None)
    }

    fn float_attribute(&self, name: &str, default: f32) -> Result<f32, String> {
        Ok(self
            .attribute(name)?
            .and_then(|f| {
                f.iter().find_map(|x| match (x.no, &x.payload) {
                    (ATTR_F, Payload::Fixed32(v)) => Some(f32::from_bits(*v)),
                    _ => None,
                })
            })
            .unwrap_or(default))
    }

    fn int_attribute(&self, name: &str) -> Result<Option<i64>, String> {
        Ok(self.attribute(name)?.and_then(|f| {
            f.iter().find_map(|x| match (x.no, &x.payload) {
                (ATTR_I, Payload::Varint(v)) => Some(*v as i64),
                _ => None,
            })
        }))
    }

    fn ints_attribute(&self, name: &str) -> Result<Option<Vec<i64>>, String> {
        let Some(f) = self.attribute(name)? else {
            return Ok(None);
        };
        let mut out = Vec::new();
        for x in &f {
            match (x.no, &x.payload) {
                (ATTR_INTS, Payload::Varint(v)) => out.push(*v as i64),
                (ATTR_INTS, Payload::Bytes(packed)) => {
                    let mut at = 0;
                    while at < packed.len() {
                        out.push(read_varint(packed, &mut at)? as i64);
                    }
                }
                _ => {}
            }
        }
        Ok(Some(out))
    }
}

fn rewrite_graph(graph: &[u8], pools: &mut Vec<String>) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(graph.len() + 1024);
    for f in fields(graph)? {
        let (GRAPH_NODE, Payload::Bytes(n)) = (f.no, &f.payload) else {
            out.extend_from_slice(f.raw);
            continue;
        };
        let node = Node::read(n)?;
        if RANDOM_OPS.contains(&node.op_type.as_str()) {
            let pool = format!("fontelle_noise_{}", pools.len());
            for replacement in replace_random(&node, &pool)? {
                put_bytes(GRAPH_NODE, &replacement, &mut out);
            }
            pools.push(pool);
            continue;
        }
        if node.subgraphs()?.is_empty() {
            out.extend_from_slice(f.raw);
            continue;
        }
        // A node with subgraphs (If, Loop, Scan): its attributes rewritten.
        let mut rebuilt = Vec::with_capacity(n.len() + 1024);
        for nf in fields(n)? {
            let (NODE_ATTRIBUTE, Payload::Bytes(a)) = (nf.no, &nf.payload) else {
                rebuilt.extend_from_slice(nf.raw);
                continue;
            };
            let mut attribute = Vec::with_capacity(a.len() + 1024);
            for af in fields(a)? {
                match (af.no, &af.payload) {
                    (ATTR_G | ATTR_GRAPHS, Payload::Bytes(g)) => {
                        put_bytes(af.no, &rewrite_graph(g, pools)?, &mut attribute);
                    }
                    _ => attribute.extend_from_slice(af.raw),
                }
            }
            put_bytes(NODE_ATTRIBUTE, &attribute, &mut rebuilt);
        }
        put_bytes(GRAPH_NODE, &rebuilt, &mut out);
    }
    Ok(out)
}

fn new_node(
    op: &str,
    inputs: &[&str],
    outputs: &[&str],
    name: &str,
    attributes: &[Vec<u8>],
) -> Vec<u8> {
    let mut n = Vec::new();
    for i in inputs {
        put_bytes(NODE_INPUT, i.as_bytes(), &mut n);
    }
    for o in outputs {
        put_bytes(NODE_OUTPUT, o.as_bytes(), &mut n);
    }
    put_bytes(NODE_NAME, name.as_bytes(), &mut n);
    put_bytes(NODE_OP_TYPE, op.as_bytes(), &mut n);
    for a in attributes {
        put_bytes(NODE_ATTRIBUTE, a, &mut n);
    }
    n
}

/// A `Constant` node's `value`: a tensor of the given element type.
fn tensor_attribute(elem_type: u64, dims: &[i64], raw: &[u8]) -> Vec<u8> {
    let mut t = Vec::new();
    for d in dims {
        put_int(1, *d as u64, &mut t);
    }
    put_int(2, elem_type, &mut t);
    put_bytes(9, raw, &mut t);
    let mut a = Vec::new();
    put_bytes(ATTR_NAME, b"value", &mut a);
    put_bytes(ATTR_T, &t, &mut a);
    put_int(ATTR_TYPE, TYPE_TENSOR, &mut a);
    a
}

fn int64s(values: &[i64]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// The nodes that stand in for a random one: the first `size` values of
/// the pool, shaped like the noise was.
fn replace_random(node: &Node, pool: &str) -> Result<Vec<Vec<u8>>, String> {
    let output = node
        .outputs
        .first()
        .ok_or_else(|| format!("{} node has no output", node.op_type))?;
    let p = format!("{pool}/");
    let name = |s: &str| format!("{p}{s}");
    let mut nodes = Vec::new();
    let constant = |nodes: &mut Vec<Vec<u8>>, id: &str, elem: u64, dims: &[i64], raw: &[u8]| {
        nodes.push(new_node(
            "Constant",
            &[],
            &[&name(id)],
            &name(id),
            &[tensor_attribute(elem, dims, raw)],
        ));
    };
    constant(&mut nodes, "zero", ELEM_INT64, &[1], &int64s(&[0]));
    match node.op_type.as_str() {
        "RandomNormalLike" => {
            let like = node
                .inputs
                .first()
                .ok_or("RandomNormalLike node has no input")?;
            constant(&mut nodes, "one", ELEM_INT64, &[1], &int64s(&[1]));
            nodes.push(new_node(
                "Size",
                &[like],
                &[&name("size")],
                &name("size"),
                &[],
            ));
            nodes.push(new_node(
                "Reshape",
                &[&name("size"), &name("one")],
                &[&name("count")],
                &name("count"),
                &[],
            ));
            nodes.push(new_node(
                "Shape",
                &[like],
                &[&name("shape")],
                &name("shape"),
                &[],
            ));
        }
        _ => {
            let shape = node
                .ints_attribute("shape")?
                .ok_or("RandomNormal node has no shape")?;
            let count: i64 = shape.iter().product();
            constant(&mut nodes, "count", ELEM_INT64, &[1], &int64s(&[count]));
            constant(
                &mut nodes,
                "shape",
                ELEM_INT64,
                &[shape.len() as i64],
                &int64s(&shape),
            );
        }
    }
    nodes.push(new_node(
        "Slice",
        &[pool, &name("zero"), &name("count")],
        &[&name("flat")],
        &name("flat"),
        &[],
    ));
    let mean = node.float_attribute("mean", 0.0)?;
    let scale = node.float_attribute("scale", 1.0)?;
    let cast = node
        .int_attribute("dtype")?
        .filter(|&t| t != ELEM_FLOAT as i64);
    let plain = mean == 0.0 && scale == 1.0 && cast.is_none();
    let shaped = if plain {
        output.clone()
    } else {
        name("shaped")
    };
    nodes.push(new_node(
        "Reshape",
        &[&name("flat"), &name("shape")],
        &[&shaped],
        &name("reshape"),
        &[],
    ));
    if plain {
        return Ok(nodes);
    }
    constant(&mut nodes, "scale", ELEM_FLOAT, &[], &scale.to_le_bytes());
    constant(&mut nodes, "mean", ELEM_FLOAT, &[], &mean.to_le_bytes());
    nodes.push(new_node(
        "Mul",
        &[&shaped, &name("scale")],
        &[&name("scaled")],
        &name("mul"),
        &[],
    ));
    let moved = if cast.is_some() {
        name("moved")
    } else {
        output.clone()
    };
    nodes.push(new_node(
        "Add",
        &[&name("scaled"), &name("mean")],
        &[&moved],
        &name("add"),
        &[],
    ));
    if let Some(to) = cast {
        let mut a = Vec::new();
        put_bytes(ATTR_NAME, b"to", &mut a);
        put_int(ATTR_I, to as u64, &mut a);
        put_int(ATTR_TYPE, TYPE_INT, &mut a);
        nodes.push(new_node("Cast", &[&moved], &[output], &name("cast"), &[a]));
    }
    Ok(nodes)
}

/// The graph input a pool is fed through: 1-D float of any length.
fn pool_value_info(name: &str) -> Vec<u8> {
    let mut dim = Vec::new();
    put_bytes(2, format!("{name}_len").as_bytes(), &mut dim);
    let mut shape = Vec::new();
    put_bytes(1, &dim, &mut shape);
    let mut tensor = Vec::new();
    put_int(1, ELEM_FLOAT, &mut tensor);
    put_bytes(2, &shape, &mut tensor);
    let mut ty = Vec::new();
    put_bytes(1, &tensor, &mut ty);
    let mut v = Vec::new();
    put_bytes(1, name.as_bytes(), &mut v);
    put_bytes(2, &ty, &mut v);
    v
}
