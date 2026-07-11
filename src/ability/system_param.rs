use crate::AttributesRef;
use crate::ability::ability_state::{AbilityEvent, AbilityMachine, AbilityState};
use crate::ability::tasks::{Task, TaskScope, Tasks};
use crate::ability::{Ability, AbilityError, AbilityOf, GrantedAbilities, TargetData};
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
    pub abilities:
        Query<'w, 's, (Read<Ability>, AttributesRef<'static, 'static>), Without<IsResource>>,
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
        ability_parent: Entity,
    ) -> Result<Entity, AbilityError> {
        if !self.actors.contains(ability_parent) {
            return Err(AbilityError::GrantingAbilityToNonActor(ability_parent).into());
        }

        let ability_id = self
            .commands
            .spawn((
                Ability {
                    handle: { ability.clone() },
                },
                AbilityOf(ability_parent),
            ))
            .id();

        Ok(ability_id)
    }

    pub fn ability_def(&self, entity: Entity) -> Result<&AbilityDef, AbilityError> {
        let (ability, _) = self
            .abilities
            .get(entity)
            .or(Err(AbilityError::AbilityDoesNotExist(entity)))?;
        let definition = self
            .registry
            .ability_definitions()
            .get(&ability.handle)
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
            if let Ok((ability, _)) = self.abilities.get(ability_entity) {
                if ability.handle == *handle {
                    return Some(ability_entity);
                }
            }
        }
        None
    }

    pub fn get_abilities_by_tag<T: Component + Reflect>(&self, actor: Entity) -> Vec<Entity> {
        let Ok((_, _, granted)) = self.actors.get(actor) else {
            return Vec::new();
        };

        granted
            .iter()
            .filter_map(|&ability_entity| {
                self.abilities
                    .get(ability_entity)
                    .ok()
                    .and_then(|(_, attrs)| {
                        if attrs.contains::<T>() {
                            Some(ability_entity)
                        } else {
                            None
                        }
                    })
            })
            .collect()
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

        self.machines.dispatch_event(
            ability_entity,
            AbilityEvent::TryActivate {
                source: entity,
                target: target_data,
            },
        );
    }

    pub fn try_activate_by_tag<T: Component + Reflect>(
        &mut self,
        actor_id: Entity,
        target_data: TargetData,
    ) {
        let abilities = self.get_abilities_by_tag::<T>(actor_id);

        for ability_id in abilities {
            self.machines.dispatch_event(
                ability_id,
                AbilityEvent::TryActivate {
                    source: actor_id,
                    target: target_data,
                },
            );
        }
    }

    pub fn task<'a>(&'a mut self, task_id: Entity) -> TaskScope<'a, 'w, 's> {
        /*let Ok(tasks) = self.tasks.get(task_id) else {
            return TaskScope::empty(&mut self.commands);
        };*/

        TaskScope::new(task_id, /*tasks.iter(),*/ &mut self.commands)
    }
}
