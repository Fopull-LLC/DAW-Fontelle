//! ONNX Runtime sessions for a bank's models.
//!
//! `ort` 2.0.0-rc.13 links ONNX Runtime 1.28 statically (pyke's prebuilt
//! CPU build; the `cuda` feature takes their CUDA 13 build) on Linux,
//! Windows (MSVC) and Apple-silicon macOS. Intel macOS has no prebuilt
//! build; there the library is loaded at run time (`ORT_DYLIB_PATH`, else
//! `libonnxruntime.dylib` on the library path) and a missing one is an
//! error the voice reports, not a crash. Community DiffSinger models are
//! exported at opsets 11 to 17 (checked here on banks from 2024–2026), well
//! inside what 1.28 runs. A session is shared by every render of its bank,
//! one run at a time; ONNX Runtime spreads each run over its own threads.

use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use fontelle_sing::SingError;
use ort::session::Session;

/// How sessions are made.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuntimeOptions {
    /// Threads each run may use; 0 lets ONNX Runtime choose.
    pub threads: usize,
    /// Try CUDA first (with the `cuda` feature); CPU otherwise.
    pub cuda: bool,
    /// Run each model's graph exactly as the file has it: its noise is then
    /// ONNX Runtime's, unseeded, and takes differ run to run. Off by
    /// default; see [`crate::onnx`].
    pub leave_graphs_unchanged: bool,
}

/// An input to feed.
#[derive(Debug, Clone, PartialEq)]
pub enum Feed {
    F32(Vec<usize>, Vec<f32>),
    I64(Vec<usize>, Vec<i64>),
    Bool(Vec<usize>, Vec<bool>),
}

impl Feed {
    /// A `[1, n]` row.
    pub fn row_f32(values: Vec<f32>) -> Self {
        Feed::F32(vec![1, values.len()], values)
    }

    pub fn row_i64(values: Vec<i64>) -> Self {
        Feed::I64(vec![1, values.len()], values)
    }

    pub fn row_bool(values: Vec<bool>) -> Self {
        Feed::Bool(vec![1, values.len()], values)
    }

    /// About how much memory it holds.
    pub fn bytes(&self) -> usize {
        match self {
            Feed::F32(_, v) => v.len() * 4,
            Feed::I64(_, v) => v.len() * 8,
            Feed::Bool(_, v) => v.len(),
        }
    }
}

/// An output, copied out of the session.
#[derive(Debug, Clone, PartialEq)]
pub enum Out {
    F32(Vec<usize>, Vec<f32>),
    I64(Vec<usize>, Vec<i64>),
    Bool(Vec<usize>, Vec<bool>),
}

impl Out {
    pub fn into_feed(self) -> Feed {
        match self {
            Out::F32(s, v) => Feed::F32(s, v),
            Out::I64(s, v) => Feed::I64(s, v),
            Out::Bool(s, v) => Feed::Bool(s, v),
        }
    }

    pub fn f32(&self) -> Option<&[f32]> {
        match self {
            Out::F32(_, v) => Some(v),
            _ => None,
        }
    }

    /// About how much memory it holds.
    pub fn bytes(&self) -> usize {
        match self {
            Out::F32(_, v) => v.len() * 4,
            Out::I64(_, v) => v.len() * 8,
            Out::Bool(_, v) => v.len(),
        }
    }

    pub fn shape(&self) -> &[usize] {
        match self {
            Out::F32(s, _) | Out::I64(s, _) | Out::Bool(s, _) => s,
        }
    }
}

pub struct Model {
    name: String,
    session: Mutex<Session>,
    /// Declared inputs and their ranks.
    inputs: Vec<(String, usize)>,
    noise_inputs: Vec<String>,
    /// What it was asked lately and answered, oldest first.
    memory: Mutex<VecDeque<Asked>>,
    runs: AtomicUsize,
}

/// Answers a model keeps: enough for a phrase's preview and final and an
/// edit or two, and never more than [`REMEMBERED_BYTES`] of inputs and
/// outputs (a vocoder's take of a long phrase is a few megabytes).
const REMEMBERED: usize = 6;
const REMEMBERED_BYTES: usize = 24 << 20;

struct Asked {
    feeds: Vec<(String, Feed)>,
    noise: Option<(u64, usize)>,
    out: HashMap<String, Out>,
    bytes: usize,
}

impl std::fmt::Debug for Model {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Model")
            .field("name", &self.name)
            .field("inputs", &self.inputs)
            .field("noise_inputs", &self.noise_inputs)
            .finish()
    }
}

impl Model {
    pub fn load(path: &Path, options: &RuntimeOptions) -> Result<Model, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        Self::from_bytes(&bytes, &name, options).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// A model from its bytes; its in-graph noise is fed from the seed
    /// given to [`Model::run`] (see [`crate::onnx`]).
    pub fn from_bytes(bytes: &[u8], name: &str, options: &RuntimeOptions) -> Result<Model, String> {
        // A model too old to have its noise fed still sings, unseeded.
        let patched = if options.leave_graphs_unchanged {
            None
        } else {
            crate::onnx::externalize_noise(bytes).ok().flatten()
        };
        let (bytes, noise_inputs) = match &patched {
            Some(p) => (&p.bytes[..], p.noise_inputs.clone()),
            None => (bytes, Vec::new()),
        };
        let session = builder(options)?
            .commit_from_memory(bytes)
            .map_err(|e| e.to_string())?;
        let inputs = session
            .inputs()
            .iter()
            .filter(|i| !noise_inputs.iter().any(|n| n == i.name()))
            .map(|i| {
                let rank = match i.dtype() {
                    ort::value::ValueType::Tensor { shape, .. } => shape.len(),
                    _ => 0,
                };
                (i.name().to_string(), rank)
            })
            .collect();
        Ok(Model {
            name: name.to_string(),
            session: Mutex::new(session),
            inputs,
            noise_inputs,
            memory: Mutex::new(VecDeque::new()),
            runs: AtomicUsize::new(0),
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn has_input(&self, name: &str) -> bool {
        self.inputs.iter().any(|(n, _)| n == name)
    }

    /// The inputs that carry noise, filled at each run.
    pub fn noise_inputs(&self) -> &[String] {
        &self.noise_inputs
    }

    /// How many times the graph itself has run (an answer from memory is
    /// not a run).
    pub fn runs(&self) -> usize {
        self.runs.load(Ordering::Relaxed)
    }

    /// Runs the model. Feeds the model does not declare are left out; one
    /// it declares and is not given is an error. A one-value feed is shaped
    /// to the rank the model declares (`[1]` or a scalar). `noise` is the
    /// seed and the pool length (values per noise input). `cancelled` is
    /// polled while the run is in progress and ends it.
    ///
    /// Asked again what it was asked lately — the same declared inputs and
    /// noise — it answers from memory: what it gave is what it would give.
    /// An edit to a lane that only the acoustic model reads leaves the
    /// duration, pitch and variance models' inputs as they were, so a
    /// preview after it runs the acoustic model and the vocoder only.
    pub fn run(
        &self,
        feeds: Vec<(&str, Feed)>,
        noise: Option<(u64, usize)>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<HashMap<String, Out>, SingError> {
        if cancelled() {
            return Err(SingError::Cancelled);
        }
        let fail = |why: String| SingError::Failed(format!("{}: {why}", self.name));
        let mut feeds: HashMap<&str, Feed> = feeds.into_iter().collect();
        let mut declared: Vec<(String, Feed)> = Vec::with_capacity(self.inputs.len());
        for (name, _) in &self.inputs {
            let feed = feeds.remove(name.as_str()).ok_or_else(|| {
                fail(format!(
                    "the model needs an input \"{name}\" this runtime does not know how to give"
                ))
            })?;
            declared.push((name.clone(), feed));
        }
        // Noise only matters to a model that draws it.
        let noise = noise.filter(|_| !self.noise_inputs.is_empty());
        if let Some(out) = self.remembered(&declared, noise) {
            return Ok(out);
        }
        let out = self.run_graph(declared.clone(), noise, cancelled)?;
        self.runs.fetch_add(1, Ordering::Relaxed);
        self.remember(declared, noise, &out);
        Ok(out)
    }

    fn remembered(
        &self,
        feeds: &[(String, Feed)],
        noise: Option<(u64, usize)>,
    ) -> Option<HashMap<String, Out>> {
        let mut memory = self.memory.lock().unwrap_or_else(|e| e.into_inner());
        let at = memory
            .iter()
            .position(|a| a.noise == noise && a.feeds == feeds)?;
        // Most recently asked last: the oldest is forgotten first.
        let asked = memory.remove(at)?;
        let out = asked.out.clone();
        memory.push_back(asked);
        Some(out)
    }

    fn remember(
        &self,
        feeds: Vec<(String, Feed)>,
        noise: Option<(u64, usize)>,
        out: &HashMap<String, Out>,
    ) {
        let bytes = feeds.iter().map(|(_, f)| f.bytes()).sum::<usize>()
            + out.values().map(Out::bytes).sum::<usize>();
        if bytes > REMEMBERED_BYTES {
            return;
        }
        let mut memory = self.memory.lock().unwrap_or_else(|e| e.into_inner());
        memory.push_back(Asked {
            feeds,
            noise,
            out: out.clone(),
            bytes,
        });
        let mut total: usize = memory.iter().map(|a| a.bytes).sum();
        while memory.len() > REMEMBERED || total > REMEMBERED_BYTES {
            let Some(old) = memory.pop_front() else {
                break;
            };
            total -= old.bytes;
        }
    }

    fn run_graph(
        &self,
        declared: Vec<(String, Feed)>,
        noise: Option<(u64, usize)>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<HashMap<String, Out>, SingError> {
        let fail = |why: String| SingError::Failed(format!("{}: {why}", self.name));
        let mut values: Vec<(String, ort::value::DynValue)> = Vec::new();
        for ((name, feed), (_, rank)) in declared.into_iter().zip(&self.inputs) {
            values.push((name, value(feed, *rank).map_err(fail)?));
        }
        let (seed, pool) = noise.unwrap_or((0, 1));
        for (k, name) in self.noise_inputs.iter().enumerate() {
            let mut g =
                crate::noise::Gaussian::new(seed ^ (k as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15));
            let mut v = vec![0.0f32; pool.max(1)];
            g.fill(&mut v);
            let t =
                ort::value::Tensor::from_array(([v.len()], v)).map_err(|e| fail(e.to_string()))?;
            values.push((name.clone(), t.into_dyn()));
        }
        let mut session = self.session.lock().unwrap_or_else(|e| e.into_inner());
        let options = ort::session::RunOptions::new().map_err(|e| fail(e.to_string()))?;
        let result = std::thread::scope(|scope| {
            let session: &mut Session = &mut session;
            let options = &options;
            let handle = scope.spawn(move || -> Result<HashMap<String, Out>, String> {
                let outputs = session
                    .run_with_options(values, options)
                    .map_err(|e| e.to_string())?;
                let mut out = HashMap::new();
                for (name, v) in outputs.iter() {
                    out.insert(name.to_string(), extract(&v)?);
                }
                Ok(out)
            });
            let mut stopped = false;
            while !handle.is_finished() {
                if !stopped && cancelled() {
                    let _ = options.terminate();
                    stopped = true;
                }
                // A small model answers in well under a millisecond: a
                // coarse poll was a floor of 5 ms on each of a phrase's
                // half-dozen runs.
                std::thread::sleep(std::time::Duration::from_micros(500));
            }
            let r = handle
                .join()
                .unwrap_or_else(|_| Err("the run panicked".to_string()));
            (r, stopped)
        });
        match result {
            (_, true) => Err(SingError::Cancelled),
            (Ok(out), false) => Ok(out),
            (Err(e), false) => Err(fail(e)),
        }
    }
}

/// Whether ONNX Runtime is linked in, or loaded at run time.
const LOADED_AT_RUN_TIME: bool = cfg!(any(
    all(windows, target_env = "gnu"),
    all(target_os = "macos", target_arch = "x86_64")
));

/// Loads ONNX Runtime where it is not linked in; once.
fn runtime_ready() -> Result<(), String> {
    if !LOADED_AT_RUN_TIME {
        return Ok(());
    }
    #[cfg(any(
        all(windows, target_env = "gnu"),
        all(target_os = "macos", target_arch = "x86_64")
    ))]
    {
        static READY: std::sync::OnceLock<Result<(), String>> = std::sync::OnceLock::new();
        return READY
            .get_or_init(|| {
                let path = std::env::var("ORT_DYLIB_PATH")
                    .ok()
                    .filter(|p| !p.is_empty())
                    .unwrap_or_else(|| {
                        if cfg!(windows) {
                            "onnxruntime.dll".into()
                        } else {
                            "libonnxruntime.dylib".into()
                        }
                    });
                ort::init_from(&path)
                    .map(|env| {
                        env.commit();
                    })
                    .map_err(|e| format!("ONNX Runtime could not be loaded from {path}: {e}"))
            })
            .clone();
    }
    #[allow(unreachable_code)]
    Ok(())
}

fn builder(options: &RuntimeOptions) -> Result<ort::session::builder::SessionBuilder, String> {
    runtime_ready()?;
    // Failures come back as errors; ONNX Runtime's own log would only
    // repeat them on stderr (a cancelled run is logged as an error).
    let mut b = Session::builder()
        .map_err(|e| e.to_string())?
        .with_log_level(ort::logging::LogLevel::Fatal)
        .map_err(|e| e.to_string())?;
    if options.threads > 0 {
        b = b
            .with_intra_threads(options.threads)
            .map_err(|e| e.to_string())?;
    }
    #[cfg(feature = "cuda")]
    if options.cuda {
        b = b
            .with_execution_providers([ort::ep::CUDA::default().build()])
            .map_err(|e| e.to_string())?;
    }
    Ok(b)
}

/// A feed as a value, shaped to the model's rank where that is only a
/// matter of a one-value tensor being a scalar or not.
fn value(feed: Feed, rank: usize) -> Result<ort::value::DynValue, String> {
    fn shape(mut s: Vec<usize>, rank: usize) -> Vec<usize> {
        let count: usize = s.iter().product();
        if s.len() != rank && count == 1 {
            s = vec![1; rank];
        }
        s
    }
    let v = match feed {
        Feed::F32(s, d) => {
            ort::value::Tensor::from_array((shape(s, rank), d)).map(|t| t.into_dyn())
        }
        Feed::I64(s, d) => {
            ort::value::Tensor::from_array((shape(s, rank), d)).map(|t| t.into_dyn())
        }
        Feed::Bool(s, d) => {
            ort::value::Tensor::from_array((shape(s, rank), d)).map(|t| t.into_dyn())
        }
    };
    v.map_err(|e| e.to_string())
}

fn extract(v: &ort::value::ValueRef<'_>) -> Result<Out, String> {
    use ort::value::{TensorElementType, ValueType};
    let dims = |s: &ort::value::Shape| s.iter().map(|&d| d.max(0) as usize).collect::<Vec<_>>();
    match v.dtype() {
        ValueType::Tensor {
            ty: TensorElementType::Float32,
            ..
        } => {
            let (s, d) = v.try_extract_tensor::<f32>().map_err(|e| e.to_string())?;
            Ok(Out::F32(dims(s), d.to_vec()))
        }
        ValueType::Tensor {
            ty: TensorElementType::Int64,
            ..
        } => {
            let (s, d) = v.try_extract_tensor::<i64>().map_err(|e| e.to_string())?;
            Ok(Out::I64(dims(s), d.to_vec()))
        }
        ValueType::Tensor {
            ty: TensorElementType::Bool,
            ..
        } => {
            let (s, d) = v.try_extract_tensor::<bool>().map_err(|e| e.to_string())?;
            Ok(Out::Bool(dims(s), d.to_vec()))
        }
        other => Err(format!(
            "an output of a type this runtime does not read: {other:?}"
        )),
    }
}
