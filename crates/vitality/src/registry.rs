use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use vitality_abilities::AbilityDef;
pub use vitality_abilities::registry as ability_registry;
use vitality_abilities::registry::AbilityToken;
use vitality_effects::definition::EffectDef;
pub use vitality_effects::registry as effect_registry;
use vitality_effects::registry::EffectToken;

#[derive(SystemParam)]
pub struct Registry<'w> {
    effects: vitality_effects::registry::Registry<'w>,
    abilities: vitality_abilities::registry::Registry<'w>,
}

impl Registry<'_> {
    pub fn effect(&self, name: &EffectToken) -> Handle<EffectDef> {
        self.effects.effect(name)
    }

    pub fn ability(&self, name: &AbilityToken) -> Handle<AbilityDef> {
        self.abilities.ability(name)
    }
}

#[derive(SystemParam)]
pub struct RegistryMut<'w> {
    effects: vitality_effects::registry::RegistryMut<'w>,
    abilities: vitality_abilities::registry::RegistryMut<'w>,
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
}
