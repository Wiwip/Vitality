use bevy::ecs::system::lifetimeless::Read;
use crate::ability::{
    Ability, AbilityCooldown, AbilityError, GrantAbilityCommand, GrantedAbilities, TargetData, TryActivateAbility,
};
use crate::actors::Actor;
use crate::assets::AbilityDef;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use crate::registry::{RegistryMut, ability_registry::AbilityToken};

#[derive(SystemParam)]
pub struct Abilities<'w, 's> {
    abilities: Query<'w, 's, Read<Ability>>,
    ability_entities: Query<'w, 's, (Read<Ability>, Option<Read<AbilityCooldown>>)>,
    actors: Query<'w, 's, (Read<Actor>, Read<GrantedAbilities>)>,
    registry: RegistryMut<'w>,
    commands: Commands<'w, 's>,
}

impl<'w, 's> Abilities<'w, 's> {
    pub fn grant_ability_by_token(&mut self, entity: Entity, token: &AbilityToken) -> Result<Entity, AbilityError> {
        let handle = self.get_ability_from_token(&token);
        self.grant_ability(&handle, entity)
    }

    pub fn grant_ability(
        &mut self,
        ability: &Handle<AbilityDef>,
        grant_ability_target_entity: Entity,
    ) -> Result<Entity, AbilityError> {
        if !self.actors.contains(grant_ability_target_entity) {
            return Err(
                AbilityError::GrantingAbilityToNonActor(grant_ability_target_entity).into(),
            );
        }

        let ability_id = self
            .commands
            .spawn_empty()
            .queue(GrantAbilityCommand {
                parent: grant_ability_target_entity,
                handle: ability.clone(),
            })
            .id();

        Ok(ability_id)
    }

    pub fn try_activate_by_tag<T: Component + Reflect>(&mut self, entity: Entity, target_data: TargetData) {
        self.commands.trigger(TryActivateAbility::by_tag::<T>(
            entity,
            target_data,
        ));
    }

    pub fn try_activate_by_id(
        &mut self,
        entity: Entity,
        definition: AssetId<AbilityDef>,
        target_data: TargetData,
    ) {
        self.commands.trigger(TryActivateAbility::by_def(
            entity,
            definition,
            target_data,
        ));
    }

    pub fn ability_def(&self, entity: Entity) -> Result<&AbilityDef, AbilityError> {
        let ability = self
            .abilities
            .get(entity)
            .or(Err(AbilityError::AbilityDoesNotExist(entity)))?;
        let definition = self
            .registry
            .ability_definitions()
            .get(&ability.0)
            .ok_or(AbilityError::AbilityDoesNotExist(entity))?;

        Ok(definition)
    }

    pub fn register_ability(&mut self, token: AbilityToken, ability: AbilityDef) {
        self.registry.add_ability(token, ability);
    }

    pub fn is_ability_ready(&self, ability_entity: Entity) -> bool {
        if let Ok((_, cooldown)) = self.ability_entities.get(ability_entity) {
            if let Some(cooldown) = cooldown {
                return cooldown.timer.is_finished();
            }
        }
        true
    }

    pub fn get_ability_from_token(&self, token: &AbilityToken) -> Handle<AbilityDef> {
        self.registry.ability(token)
    }

    pub fn get_ability_entity(
        &self,
        actor: Entity,
        token: &AbilityToken,
    ) -> Option<Entity> {
        let handle = self.get_ability_from_token(token);
        self.get_ability_entity_by_handle(actor, &handle)
    }

    pub fn get_ability_entity_by_handle(
        &self,
        actor: Entity,
        handle: &Handle<AbilityDef>,
    ) -> Option<Entity> {
        let (_, granted) = self.actors.get(actor).ok()?;
        for &ability_entity in granted.iter() {
            if let Ok((ability, _)) = self.ability_entities.get(ability_entity) {
                if ability.0 == *handle {
                    return Some(ability_entity);
                }
            }
        }
        None
    }

    pub fn try_activate_by_token(&mut self, entity: Entity, token: &AbilityToken, target_data: TargetData) {
        let handle = self.get_ability_from_token(token);
        self.try_activate_by_id(entity, handle.id(), target_data);
    }
}
