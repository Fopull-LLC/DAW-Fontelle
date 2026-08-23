use fontelle_types::{AssetId, AssetRef};
use slotmap::SlotMap;

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct AssetTable {
    pub assets: SlotMap<AssetId, AssetRef>,
}
