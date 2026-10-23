/// Contains functionality for plugin instances that process data.
pub mod instance;
/// Contains functionality for nodes. Nodes are used to represent metadata.
pub mod node;
/// Contains functionality to describe and instantiate plugins.
pub mod plugin;
/// Contains port to describe IO for plugins.
pub mod port;
/// Contains data about plugin UIs.
pub mod ui;

mod world;

pub use lilv_sys as sys;
pub use world::World;
