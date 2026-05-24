use crate::assets::{AbilityDef, ActorDef, EffectDef};
use crate::registry::ability_registry::{AbilityRegistry, AbilityToken};
use crate::registry::effect_registry::{EffectRegistry, EffectToken};
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use crate::registry::actor_registry::{ActorRegistry, ActorToken};

pub mod ability_registry;
pub mod effect_registry;
pub mod actor_registry;

pub struct RegistryPlugin;

impl Plugin for RegistryPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(EffectRegistry::default());
        app.insert_resource(AbilityRegistry::default());
        app.insert_resource(ActorRegistry::default());
    }
}

#[derive(SystemParam)]
pub struct Registry<'w> {
    pub ability_registry: Res<'w, AbilityRegistry>,
    pub ability_assets: Res<'w, Assets<AbilityDef>>,
    
    pub effect_registry: Res<'w, EffectRegistry>,
    
    pub actor_registry: Res<'w, ActorRegistry>,
}

impl Registry<'_> {
    pub fn ability_definitions(&self) -> &Assets<AbilityDef> {
        &self.ability_assets
    }
    
    pub fn effect(&self, name: &EffectToken) -> &Handle<EffectDef> {
        self.effect_registry.get(name).expect("effect not found")
    }

    pub fn ability(&self, name: &AbilityToken) -> Handle<AbilityDef> {
        self.ability_registry.get(name).clone()
    }

    pub fn actor(&self, name: &ActorToken) -> Handle<ActorDef> {
        self.actor_registry.get(name).clone()
    }
}

#[derive(SystemParam)]
pub struct RegistryMut<'w> {
    pub ability_registry: ResMut<'w, AbilityRegistry>,
    pub ability_assets: ResMut<'w, Assets<AbilityDef>>,
 
    pub effect_registry: ResMut<'w, EffectRegistry>,
    pub effect_assets: ResMut<'w, Assets<EffectDef>>,
 
    pub actor_registry: ResMut<'w, ActorRegistry>,
    pub actor_assets: ResMut<'w, Assets<ActorDef>>,
}

impl RegistryMut<'_> {
    pub fn ability_definitions(&self) -> &Assets<AbilityDef> {
        &self.ability_assets
    }

    pub fn add_effect(&mut self, name: EffectToken, effect: EffectDef) {
        let handle = self.effect_assets.add(effect);
        self.effect_registry.add(name, handle);
    }

    pub fn effect(&self, name: &EffectToken) -> &Handle<EffectDef> {
        self.effect_registry.get(&name).expect("Effect must exist.")
    }

    pub fn add_ability(&mut self, name: AbilityToken, ability: AbilityDef) {
        let handle = self.ability_assets.add(ability);
        self.ability_registry.add(name, handle);
    }

    pub fn ability(&self, name: &AbilityToken) -> Handle<AbilityDef> {
        self.ability_registry.get(name).clone()
    }

    pub fn add_actor(&mut self, name: ActorToken, actor: ActorDef) {
        let handle = self.actor_assets.add(actor);
        self.actor_registry.add(name, handle);
    }

    pub fn actor(&self, name: &ActorToken) -> Handle<ActorDef> {
        self.actor_registry.get(name).clone()
    }
}
