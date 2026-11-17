use crate::AbilityRef;
use crate::ability_state::{AbilityEvent, AbilityMachine, AbilityState};
use crate::definition::AbilityDef;
use crate::lifecycle::{ActivationError, check_activation};
use crate::registry::{AbilityToken, Registry};
use crate::tasks::TaskScope;
use crate::tasks::{Task, Tasks};
use crate::{Ability, AbilityError, AbilityOf, GrantedAbilities, TargetData};
use bevy::ecs::resource::IsResource;
use bevy::ecs::system::SystemParam;
use bevy::ecs::system::lifetimeless::Read;
use bevy::prelude::*;
use hfsm_bevy::MachineQuery;
use vitality_core::attributes::ManageAttributes;

#[derive(SystemParam)]
pub struct Abilities<'w, 's> {
    pub abilities:
        Query<'w, 's, (Read<Ability>, AbilityRef<'static, 'static>), Without<IsResource>>,
    pub actors: Query<
        'w,
        's,
        (
            Read<ManageAttributes>,
            AbilityRef<'static, 'static>,
            Read<GrantedAbilities>,
        ),
        Without<IsResource>,
    >,
    pub tasks: Query<'w, 's, Read<Tasks>, With<Task>>,
    owners: Query<'w, 's, Read<AbilityOf>>,
    pub registry: Registry<'w>,
    pub machines: MachineQuery<'w, 's, AbilityMachine>,
    pub commands: Commands<'w, 's>,
}

impl<'w, 's> Abilities<'w, 's> {
    pub fn grant_ability(
        &mut self,
        ability: &Handle<AbilityDef>,
        ability_parent: Entity,
    ) -> Result<Entity, AbilityError> {
        if !self.actors.contains(ability_parent) {
            return Err(AbilityError::GrantingAbilityToNonActor(ability_parent));
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
            .inspect_err(|err| {
                warn_once!(
                    "[{}] AbilityMachine is_in_state failed: {err}",
                    ability_entity
                )
            })
            .unwrap_or_default()
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
            if let Ok((ability, _)) = self.abilities.get(ability_entity)
                && ability.handle == *handle
            {
                return Some(ability_entity);
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

        if let Err(err) = self.machines.dispatch_event(
            ability_entity,
            AbilityEvent::TryActivate {
                source: entity,
                target: target_data,
            },
        ) {
            error!("[{}] AbilityMachine dispatch failed: {err}", ability_entity);
        }
    }

    /// asks the actor's granted copy of `handle` to activate; false when it isn't granted, and the outcome arrives as `AbilityActivated` or `AbilityActivationFailed`
    pub fn try_activate(
        &mut self,
        actor: Entity,
        handle: &Handle<AbilityDef>,
        target: TargetData,
    ) -> bool {
        let Some(ability) = self.get_ability_entity_by_handle(actor, handle) else {
            return false;
        };
        if let Err(err) = self.machines.dispatch_event(
            ability,
            AbilityEvent::TryActivate {
                source: actor,
                target,
            },
        ) {
            error!("[{ability}] AbilityMachine dispatch failed: {err}");
        }
        true
    }

    pub fn try_activate_by_tag<T: Component + Reflect>(
        &mut self,
        actor_id: Entity,
        target_data: TargetData,
    ) {
        let abilities = self.get_abilities_by_tag::<T>(actor_id);

        for ability_id in abilities {
            if let Err(err) = self.machines.dispatch_event(
                ability_id,
                AbilityEvent::TryActivate {
                    source: actor_id,
                    target: target_data,
                },
            ) {
                error!("[{}] AbilityMachine dispatch failed: {err}", ability_id);
            }
        }
    }

    /// Same checks the machine runs on activation, without activating.
    pub fn can_activate(&self, ability: Entity, target: TargetData) -> Result<(), ActivationError> {
        let (ability_component, ability_ref) = self
            .abilities
            .get(ability)
            .map_err(|_| ActivationError::NotGranted)?;
        let def = self
            .registry
            .ability_definitions()
            .get(&ability_component.handle)
            .ok_or(ActivationError::NotGranted)?;
        let caster = self
            .owners
            .get(ability)
            .map_err(|_| ActivationError::NotGranted)?
            .0;
        let (_, caster_ref, _) = self
            .actors
            .get(caster)
            .map_err(|_| ActivationError::NotGranted)?;
        if self.is_in(ability, AbilityState::Active) {
            return Err(ActivationError::AlreadyActive);
        }
        if self.is_in(ability, AbilityState::Recovery) {
            return Err(ActivationError::OnCooldown);
        }
        let target_ref = match target {
            TargetData::SelfCast => Some(caster_ref),
            TargetData::Target(target) => self.actors.get(target).ok().map(|(_, r, _)| r),
            TargetData::Location(_) => None,
        };
        check_activation(def, &caster_ref, &ability_ref, target_ref)
    }

    fn is_in(&self, ability: Entity, state: AbilityState) -> bool {
        self.machines.is_in_state(ability, state).unwrap_or(false)
    }

    pub fn is_active(&self, ability: Entity) -> bool {
        self.is_in(ability, AbilityState::Active)
    }

    /// Ends an active ability early; it goes on cooldown as if it had finished.
    pub fn cancel(&mut self, ability: Entity) {
        if let Err(err) = self
            .machines
            .dispatch_event(ability, AbilityEvent::CancelAbility)
        {
            warn!("[{ability}] AbilityMachine dispatch failed: {err}");
        }
    }

    pub fn cancel_by_tag<T: Component + Reflect>(&mut self, actor: Entity) {
        for ability in self.get_abilities_by_tag::<T>(actor) {
            if self.is_active(ability) {
                self.cancel(ability);
            }
        }
    }

    /// Removes a granted ability from an actor; an active one is cancelled first.
    pub fn revoke(&mut self, actor: Entity, handle: &Handle<AbilityDef>) -> bool {
        let Some(ability) = self.get_ability_entity_by_handle(actor, handle) else {
            return false;
        };
        if self.is_active(ability) {
            self.cancel(ability);
        }
        self.commands.entity(ability).try_despawn();
        true
    }

    pub fn task<'a>(&'a mut self, task_id: Entity) -> TaskScope<'a, 'w, 's> {
        /*let Ok(tasks) = self.tasks.get(task_id) else {
            return TaskScope::empty(&mut self.commands);
        };*/

        TaskScope::new(task_id, /*tasks.iter(),*/ &mut self.commands)
    }
}
