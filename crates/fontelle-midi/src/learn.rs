use std::collections::HashMap;

use fontelle_types::ParamAddress;

use crate::device::DeviceKey;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TakeoverMode {
    Jump,
    Pickup,
    Scale,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LearnMode {
    Absolute,
    Relative,
    Toggle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CcKey {
    pub channel: u8,
    pub cc: u8,
}

/// `(device, cc) -> ParamAddress`, the same addressing scheme automation uses
/// (TDD §8.2, §14.4). Right-click a parameter -> Learn -> move a control.
#[derive(Debug, Default)]
pub struct MidiLearnTable {
    routes: HashMap<(DeviceKey, CcKey), (ParamAddress, LearnMode, TakeoverMode)>,
}

impl MidiLearnTable {
    pub fn bind(
        &mut self,
        device: DeviceKey,
        cc: CcKey,
        target: ParamAddress,
        mode: LearnMode,
        takeover: TakeoverMode,
    ) {
        self.routes.insert((device, cc), (target, mode, takeover));
    }

    pub fn resolve(
        &self,
        device: &DeviceKey,
        cc: CcKey,
    ) -> Option<&(ParamAddress, LearnMode, TakeoverMode)> {
        self.routes.get(&(device.clone(), cc))
    }
}
