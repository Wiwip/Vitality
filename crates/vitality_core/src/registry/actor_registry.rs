use crate::assets::ActorDef;
use bevy::prelude::*;
use smol_str::SmolStr;
use std::collections::HashMap;

#[derive(Default, Clone, PartialEq, Eq, Hash, Reflect)]
pub struct ActorToken(SmolStr);

impl ActorToken {
    /// Construct a new [`AbilityToken`] from a [`SmolStr`].
    pub const fn new(text: SmolStr) -> Self {
        Self(text)
    }

    /// Construct a new [`AbilityToken`] from a static string.
    pub const fn new_static(text: &'static str) -> Self {
        Self(SmolStr::new_static(text))
    }
}

impl core::fmt::Display for ActorToken {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl core::fmt::Debug for ActorToken {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "AbilityToken({:?})", self.0)
    }
}

#[derive(Resource, Default)]
pub struct ActorRegistry {
    map: HashMap<ActorToken, Handle<ActorDef>>,
}

impl ActorRegistry {
    pub fn add(&mut self, token: ActorToken, handle: Handle<ActorDef>) {
        self.map.insert(token, handle);
    }

    pub fn get(&self, token: &ActorToken) -> &Handle<ActorDef> {
        self.map
            .get(&token)
            .expect(format!("{:?} not registered", token).as_str())
    }
}
