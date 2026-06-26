use crate::AttributesRef;
use crate::ability::ability_state::{AbilityEvent, AbilityMachine, AbilityState};
use crate::ability::tasks::{Task, TaskScope, Tasks};
use crate::ability::{
    Ability, AbilityError, AbilityRecovery, GrantAbilityCommand, GrantedAbilities, TargetData,
    TryActivateAbility,
};
use crate::actors::Actor;
use crate::assets::AbilityDef;
use crate::registry::{Registry, ability_registry::AbilityToken};
use bevy::ecs::resource::IsResource;
use bevy::ecs::system::SystemParam;
use bevy::ecs::system::lifetimeless::Read;
use bevy::prelude::*;
use hfsm_bevy::MachineQuery;

#[derive(SystemParam)]
pub struct Abilities<'w, 's> {
    pub abilities: Query<
        'w,
        's,
        (
            Read<Ability>,
            AttributesRef<'static, 'static>,
            Read<AbilityRecovery>,
        ),
        Without<IsResource>,
    >,
    pub actors: Query<
        'w,
        's,
        (
            Read<Actor>,
            AttributesRef<'static, 'static>,
            Read<GrantedAbilities>,
        ),
        Without<IsResource>,
    >,
    pub tasks: Query<'w, 's, Read<Tasks>, With<Task>>,
    pub registry: Registry<'w>,
    pub machines: MachineQuery<'w, 's, AbilityMachine>,
    pub commands: Commands<'w, 's>,
}

impl<'w, 's> Abilities<'w, 's> {
    pub fn grant_ability_by_token(
        &mut self,
        entity: Entity,
        token: &AbilityToken,
    ) -> Result<Entity, AbilityError> {
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

    pub fn try_activate_by_tag<T: Component + Reflect>(
        &mut self,
        entity: Entity,
        target_data: TargetData,
    ) {
        self.commands
            .trigger(TryActivateAbility::by_tag::<T>(entity, target_data));
    }

    pub fn ability_def(&self, entity: Entity) -> Result<&AbilityDef, AbilityError> {
        let (ability, _, _) = self
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

    pub fn is_ability_ready(&self, ability_entity: Entity) -> bool {
        self.machines
            .is_in_state(ability_entity, AbilityState::Ready)
    }

    pub fn get_ability_from_token(&self, token: &AbilityToken) -> Handle<AbilityDef> {
        self.registry.ability(token)
    }

    pub fn get_ability_entity(&self, actor: Entity, token: &AbilityToken) -> Option<Entity> {
        let handle = self.get_ability_from_token(token);
        self.get_ability_entity_by_handle(actor, &handle)
    }

    pub fn get_ability_entity_by_handle(
        &self,
        actor: Entity,
        handle: &Handle<AbilityDef>,
    ) -> Option<Entity> {
        let (_, _, granted) = self.actors.get(actor).ok()?;
        for &ability_entity in granted.iter() {
            if let Ok((ability, _, _)) = self.abilities.get(ability_entity) {
                if ability.0 == *handle {
                    return Some(ability_entity);
                }
            }
        }
        None
    }

    pub fn has_ability(&self, entity: Entity, token: &AbilityToken) -> bool {
        self.get_ability_entity(entity, token).is_some()
    }

    pub fn try_activate_by_token(
        &mut self,
        entity: Entity,
        token: &AbilityToken,
        target_data: TargetData,
    ) {
        let Some(ability_entity) = self.get_ability_entity(entity, token) else {
            warn_once!("Ability does not exist on entity.");
            return;
        };

        self.machines
            .dispatch_event(
                ability_entity,
                AbilityEvent::TryActivate {
                    source: entity,
                    target: target_data,
                },
            )
            .expect("Failed to dispatch abilities");
    }

    pub fn task<'a>(&'a mut self, task_id: Entity) -> TaskScope<'a, 'w, 's> {
        /*let Ok(tasks) = self.tasks.get(task_id) else {
            return TaskScope::empty(&mut self.commands);
        };*/

        TaskScope::new(task_id, /*tasks.iter(),*/ &mut self.commands)
    }
}
