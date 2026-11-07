use crate::assets::AbilityDef;
use bevy::asset::Handle;
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
