use crate::definition::AbilityDef;
use bevy::asset::Handle;
use bevy::ecs::system::SystemParam;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use smol_str::SmolStr;

#[derive(Clone, PartialEq, Eq, Hash, Reflect, Default)]
pub struct AbilityToken(SmolStr);

impl AbilityToken {
    /// Construct a new [`AbilityToken`] from a [`SmolStr`].
    pub const fn new(text: SmolStr) -> Self {
        Self(text)
    }

    /// Construct a new [`AbilityToken`] from a static string.
    pub const fn new_static(text: &'static str) -> Self {
        Self(SmolStr::new_static(text))
    }
}

impl core::fmt::Display for AbilityToken {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl core::fmt::Debug for AbilityToken {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "AbilityToken({:?})", self.0)
    }
}

#[derive(Resource, Default)]
pub struct AbilityRegistry {
    map: HashMap<AbilityToken, Handle<AbilityDef>>,
}

impl AbilityRegistry {
    pub fn add(&mut self, token: AbilityToken, handle: Handle<AbilityDef>) {
        self.map.insert(token, handle);
    }

    pub fn get(&self, token: &AbilityToken) -> &Handle<AbilityDef> {
        self.map
            .get(token)
            .expect(format!("{:?} not registered", token).as_str())
    }
}

#[derive(SystemParam)]
pub struct Registry<'w> {
    pub ability_registry: Res<'w, AbilityRegistry>,
    pub ability_assets: Res<'w, Assets<AbilityDef>>,
}

impl Registry<'_> {
    pub fn ability_definitions(&self) -> &Assets<AbilityDef> {
        &self.ability_assets
    }

    pub fn ability(&self, name: &AbilityToken) -> Handle<AbilityDef> {
        self.ability_registry.get(name).clone()
    }
}

#[derive(SystemParam)]
pub struct RegistryMut<'w> {
    pub ability_registry: ResMut<'w, AbilityRegistry>,
    pub ability_assets: ResMut<'w, Assets<AbilityDef>>,
}

impl RegistryMut<'_> {
    pub fn add_ability(&mut self, name: AbilityToken, ability: AbilityDef) {
        let handle = self.ability_assets.add(ability);
        self.ability_registry.add(name, handle);
    }

    pub fn ability(&self, name: &AbilityToken) -> Handle<AbilityDef> {
        self.ability_registry.get(name).clone()
    }
}
