use fontelle_types::{AssetId, AssetRef};

use crate::arena::Arena;

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct AssetTable {
    pub assets: Arena<AssetId, AssetRef>,
}
