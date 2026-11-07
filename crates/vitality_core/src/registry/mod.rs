use crate::assets::EffectDef;
use crate::registry::effect_registry::{EffectRegistry, EffectToken};
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

pub mod effect_registry;

pub struct RegistryPlugin;

impl Plugin for RegistryPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(EffectRegistry::default());
    }
}

#[derive(SystemParam)]
pub struct Registry<'w> {
    pub effect_registry: Res<'w, EffectRegistry>,
}

impl Registry<'_> {
    pub fn effect(&self, name: &EffectToken) -> Handle<EffectDef> {
        self.effect_registry.get(name).clone()
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
        self.effect_registry.get(&name).clone()
    }
}
