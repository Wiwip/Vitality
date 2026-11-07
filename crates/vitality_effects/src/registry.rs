use crate::definition::EffectDef;
use bevy::asset::Handle;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use smol_str::SmolStr;

#[derive(Clone, PartialEq, Eq, Hash, Reflect)]
pub struct EffectToken(SmolStr);

impl EffectToken {
    /// Construct a new [`EffectToken`] from a [`SmolStr`].
    pub const fn new(text: SmolStr) -> Self {
        Self(text)
    }

    /// Construct a new [`EffectToken`] from a static string.
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
            .expect(format!("{:?} not registered", token).as_str())
    }
}

use bevy::ecs::system::SystemParam;

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
