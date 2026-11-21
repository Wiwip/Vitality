use crate::definition::EffectDef;
use bevy_ecs::prelude::*;
use bevy_platform::collections::HashMap;
use bevy_reflect::prelude::*;
use smol_str::SmolStr;
use vitality_core::store::Assets;
use vitality_core::store::Handle;

#[derive(Clone, PartialEq, Eq, Hash, Reflect)]
pub struct EffectToken(SmolStr);

impl EffectToken {
    pub const fn new(text: SmolStr) -> Self {
        Self(text)
    }

    pub const fn new_static(text: &'static str) -> Self {
        Self(SmolStr::new_static(text))
    }
}

impl core::fmt::Display for EffectToken {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl core::fmt::Debug for EffectToken {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "EffectToken({:?})", self.0)
    }
}

#[derive(Resource, Default)]
pub struct EffectRegistry {
    map: HashMap<EffectToken, Handle<EffectDef>>,
}

impl EffectRegistry {
    pub fn add(&mut self, token: EffectToken, handle: Handle<EffectDef>) {
        self.map.insert(token, handle);
    }

    pub fn get(&self, token: &EffectToken) -> &Handle<EffectDef> {
        self.map
            .get(token)
            .unwrap_or_else(|| panic!("{:?} not registered", token))
    }
}

use bevy_ecs::system::SystemParam;

#[derive(SystemParam)]
pub struct Registry<'w> {
    pub effect_registry: Res<'w, EffectRegistry>,
}

impl Registry<'_> {
    pub fn effect(&self, name: &EffectToken) -> Handle<EffectDef> {
        *self.effect_registry.get(name)
    }
}

#[derive(SystemParam)]
pub struct RegistryMut<'w> {
    pub effect_registry: ResMut<'w, EffectRegistry>,
    pub effect_assets: ResMut<'w, Assets<EffectDef>>,
}

impl RegistryMut<'_> {
    pub fn add_effect(&mut self, name: EffectToken, effect: EffectDef) {
        let handle = self.effect_assets.add(effect);
        self.effect_registry.add(name, handle);
    }

    pub fn effect(&self, name: &EffectToken) -> Handle<EffectDef> {
        *self.effect_registry.get(name)
    }
}
