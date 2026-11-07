use crate::actors::{ActorDef, ActorToken, SpawnActorCommand};
use crate::registry::Registry;
use bevy::ecs::system::SystemParam;
use bevy::ecs::system::lifetimeless::Read;
use bevy::prelude::*;
use hfsm_bevy::MachineEvent;
use vitality_abilities::ability_state::{AbilityEvent, AbilityMachine};
use vitality_abilities::registry::AbilityToken;
use vitality_abilities::{
    Ability, AbilityDef, AbilityError, AbilityOf, AbilityRecovery, GrantedAbilities, TargetData,
};
use vitality_effects::definition::EffectDef;
use vitality_effects::registry::EffectToken;
use vitality_effects::{ApplyEffectEvent, EffectTargeting};
use vitality_effects::{GlobalActor, GlobalEffects};

#[derive(SystemParam)]
pub struct Vitality<'w, 's> {
    pub global_actor: Query<'w, 's, Entity, With<GlobalActor>>,
    pub granted_abilities: Query<'w, 's, Read<GrantedAbilities>>,
    pub ability_entities: Query<'w, 's, (Read<Ability>, Read<AbilityRecovery>)>,

    pub registry: Registry<'w>,

    pub actors: ResMut<'w, Assets<ActorDef>>,
    pub effects: ResMut<'w, Assets<EffectDef>>,
    pub global_effects: ResMut<'w, GlobalEffects>,

    pub commands: Commands<'w, 's>,
}

impl<'s, 'w> Vitality<'w, 's> {
    pub fn add_effect(&mut self, effect: EffectDef) -> Handle<EffectDef> {
        self.effects.add(effect)
    }

    pub fn get_effect_from_token(&self, token: &EffectToken) -> Handle<EffectDef> {
        self.registry.effect(token)
    }

    pub fn apply_effect_to_target(
        &mut self,
        target: Entity,
        source: Entity,
        handle: &Handle<EffectDef>,
    ) {
        self.commands.trigger(ApplyEffectEvent {
            entity: target,
            targeting: EffectTargeting::new(source, target),
            handle: handle.clone(),
        });
    }

    pub fn apply_effect_to_self(&mut self, source: Entity, handle: &Handle<EffectDef>) {
        self.apply_effect_to_target(source, source, handle);
    }

    pub fn apply_dynamic_effect_to_target(
        &mut self,
        target: Entity,
        source: Entity,
        effect: EffectDef,
    ) -> Handle<EffectDef> {
        let handle = self.effects.add(effect);

        self.commands.trigger(ApplyEffectEvent {
            entity: target,
            targeting: EffectTargeting::new(source, target),
            handle: handle.clone(),
        });
        handle
    }

    pub fn apply_dynamic_effect_to_self(
        &mut self,
        source: Entity,
        effect: EffectDef,
    ) -> Handle<EffectDef> {
        self.apply_dynamic_effect_to_target(source, source, effect)
    }

    pub fn add_actor(&mut self, actor: ActorDef) -> Handle<ActorDef> {
        self.actors.add(actor)
    }

    pub fn get_ability_from_token(&self, token: &AbilityToken) -> Handle<AbilityDef> {
        self.registry.ability(token)
    }

    pub fn get_ability_entity(&self, actor: Entity, handle: &Handle<AbilityDef>) -> Option<Entity> {
        let granted = self.granted_abilities.get(actor).ok()?;
        for &ability_entity in granted.iter() {
            if let Ok((ability, _)) = self.ability_entities.get(ability_entity) {
                if ability.handle == *handle {
                    return Some(ability_entity);
                }
            }
        }
        None
    }

    pub fn try_activate_by_token(
        &mut self,
        entity: Entity,
        token: &AbilityToken,
        target_data: TargetData,
    ) {
        let handle = self.get_ability_from_token(token);

        let Some(ability_entity) = self.get_ability_entity(entity, &handle) else {
            warn_once!("Ability does not exist on entity.");
            return;
        };

        self.commands.trigger(MachineEvent::<AbilityMachine>::new(
            ability_entity,
            AbilityEvent::TryActivate {
                source: entity,
                target: target_data,
            },
        ));
    }

    pub fn grant_ability_by_token_unchecked(
        &mut self,
        entity: Entity,
        token: &AbilityToken,
    ) -> Result<Entity, AbilityError> {
        let handle = self.get_ability_from_token(&token);
        self.grant_ability_unchecked(&handle, entity)
    }

    pub fn grant_ability_unchecked(
        &mut self,
        ability: &Handle<AbilityDef>,
        ability_parent: Entity,
    ) -> Result<Entity, AbilityError> {
        let ability_id = self
            .commands
            .spawn((
                Ability {
                    handle: { ability.clone() },
                },
                AbilityOf(ability_parent),
            ))
            .id();

        info!("[{:?}] Spawned ability.", ability_id);

        Ok(ability_id)
    }

    pub fn spawn_actor(&mut self, token: &ActorToken) -> EntityCommands<'_> {
        let handle = self.registry.actor(token);

        let mut entity_commands = self.commands.spawn_empty();
        entity_commands.queue(SpawnActorCommand { handle });
        entity_commands
    }

    pub fn spawn_actor_from_handle(&mut self, handle: &Handle<ActorDef>) -> EntityCommands<'_> {
        let mut entity_commands = self.commands.spawn_empty();
        entity_commands.queue(SpawnActorCommand {
            handle: handle.clone(),
        });
        entity_commands
    }

    pub fn add_spawn_actor(&mut self, actor: ActorDef) -> EntityCommands<'_> {
        let handle = self.actors.add(actor);
        self.spawn_actor_from_handle(&handle)
    }

    pub fn insert_actor(&mut self, entity: Entity, handle: &Handle<ActorDef>) {
        self.commands.entity(entity).queue(SpawnActorCommand {
            handle: handle.clone(),
        });
    }

    pub fn insert_actor_from_token(&mut self, entity: Entity, token: &ActorToken) {
        let handle = self.registry.actor(token);
        self.insert_actor(entity, &handle);
    }

    pub fn add_global_effect(&mut self, handle: Handle<EffectDef>) {
        self.global_effects.push(handle);
    }

    /// Gets or create the global effect actor.
    /// Global effects are attached to this actor and applied to all existing actors.
    /// This actor can serve as a game state tracker, and the effects can depend on its attributes.
    pub fn get_global_actor(&mut self) -> Entity {
        self.global_actor.single().unwrap()
    }

    pub fn spawn_global_effects(&mut self, target_actor: Entity) {
        let global_actor = self.get_global_actor();
        let effects: Vec<_> = self.global_effects.clone();

        for handle in effects.iter() {
            self.apply_effect_to_target(target_actor, global_actor, &handle);
        }
    }
}
