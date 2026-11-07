use crate::actors::{ActorDef, ActorRegistry, ActorToken};
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use vitality_abilities::AbilityDef;
use vitality_abilities::registry::AbilityToken;
use vitality_core::assets::EffectDef;
use vitality_core::registry::effect_registry::EffectToken;

pub use vitality_abilities::registry as ability_registry;
pub use vitality_core::registry::effect_registry;

#[derive(SystemParam)]
pub struct Registry<'w> {
    effects: vitality_core::registry::Registry<'w>,
    abilities: vitality_abilities::registry::Registry<'w>,
    actors: Res<'w, ActorRegistry>,
}

impl Registry<'_> {
    pub fn ability_definitions(&self) -> &Assets<AbilityDef> {
        self.abilities.ability_definitions()
    }

    pub fn effect(&self, name: &EffectToken) -> Handle<EffectDef> {
        self.effects.effect(name)
    }

    pub fn ability(&self, name: &AbilityToken) -> Handle<AbilityDef> {
        self.abilities.ability(name)
    }

    pub fn actor(&self, name: &ActorToken) -> Handle<ActorDef> {
        self.actors.get(name).clone()
    }
}

#[derive(SystemParam)]
pub struct RegistryMut<'w> {
    effects: vitality_core::registry::RegistryMut<'w>,
    abilities: vitality_abilities::registry::RegistryMut<'w>,
    actors: ResMut<'w, ActorRegistry>,
    actor_assets: ResMut<'w, Assets<ActorDef>>,
}

impl RegistryMut<'_> {
    pub fn add_effect(&mut self, name: EffectToken, effect: EffectDef) {
        self.effects.add_effect(name, effect);
    }

    pub fn effect(&self, name: &EffectToken) -> Handle<EffectDef> {
        self.effects.effect(name)
    }

    pub fn add_ability(&mut self, name: AbilityToken, ability: AbilityDef) {
        self.abilities.add_ability(name, ability);
    }

    pub fn ability(&self, name: &AbilityToken) -> Handle<AbilityDef> {
        self.abilities.ability(name)
    }

    pub fn add_actor(&mut self, name: ActorToken, actor: ActorDef) {
        let handle = self.actor_assets.add(actor);
        self.actors.add(name, handle);
    }

    pub fn actor(&self, name: &ActorToken) -> Handle<ActorDef> {
        self.actors.get(name).clone()
    }
}
