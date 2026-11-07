use crate::assets::EffectDef;
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
