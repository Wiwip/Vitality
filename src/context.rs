use crate::ability::{Ability, AbilityError, AbilityOf, AbilityRecovery, GrantAbilityCommand, GrantedAbilities, TargetData};
use crate::actors::SpawnActorCommand;
use crate::assets::{AbilityDef, ActorDef, EffectDef};
use crate::effect::global_effect::{GlobalActor, GlobalEffects};
use crate::effect::{ApplyEffectEvent, Effect, EffectTargeting};
use crate::registry::ability_registry::AbilityToken;
use crate::registry::actor_registry::ActorToken;
use crate::registry::Registry;
use crate::{AttributesMut, AttributesRef};
use bevy::ecs::system::lifetimeless::Read;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use express_it::expr::{Context, ContextMut};
use hfsm_bevy::{MachineEvent};
use crate::ability::ability_state::{AbilityEvent};

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
                if ability.0 == *handle {
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

        self.commands.trigger(MachineEvent {
            entity: ability_entity,
            event: AbilityEvent::TryActivate {
                source: entity,
                target: target_data,
            },
        });
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
        grant_ability_target_entity: Entity,
    ) -> Result<Entity, AbilityError> {
        let ability_id = self
            .commands
            .spawn_empty()
            .queue(GrantAbilityCommand {
                parent: grant_ability_target_entity,
                handle: ability.clone(),
            })
            .id();

        self.commands
            .entity(grant_ability_target_entity)
            .add_one_related::<AbilityOf>(ability_id);
        info!("[{:?}] Spawned ability.", ability_id);

        Ok(ability_id)
    }

    pub fn spawn_actor(&mut self, token: &ActorToken) -> EntityCommands<'_> {
        let handle = self.registry.actor(&token);

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

pub struct Source;
pub struct Target;
pub struct Caster;

// A trait for contexts that have a "Source" or primary actor
pub trait ActorProvider<'a, 'b, Role> {
    type ActorRef;
    fn get_actor(&self) -> &Self::ActorRef;
}
pub trait ActorProviderMut<'a, 'b, Role> {
    type ActorMut;
    fn get_actor_mut(&mut self) -> &mut Self::ActorMut;
}

/*********************************************************
 * Actor
 *********************************************************/
pub struct ActorExprSchema;
impl Context for ActorExprSchema {
    type ContextItem<'w, 's> = ActorExprContext<'w, 's>;
}
impl ContextMut for ActorExprSchema {
    type ContextItemMut<'w, 's> = ActorExprContextMut<'w, 's>;

    fn as_readonly<'w, 's, 'a>(
        mut_item: &'a Self::ContextItemMut<'w, 's>,
    ) -> Self::ContextItem<'a, 's> {
        ActorExprContext {
            actor_context: mut_item.actor_context.as_readonly(),
        }
    }
}

pub struct ActorExprContext<'w, 's> {
    pub actor_context: AttributesRef<'w, 's>,
}
pub struct ActorExprContextMut<'w, 's> {
    pub actor_context: AttributesMut<'w, 's>,
}

impl<'w, 's> ActorProvider<'w, 's, Source> for ActorExprContext<'w, 's> {
    type ActorRef = AttributesRef<'w, 's>;
    fn get_actor(&self) -> &Self::ActorRef {
        &self.actor_context
    }
}
impl<'w, 's> ActorProviderMut<'w, 's, Source> for ActorExprContextMut<'w, 's> {
    type ActorMut = AttributesMut<'w, 's>;
    fn get_actor_mut(&mut self) -> &mut Self::ActorMut {
        &mut self.actor_context
    }
}

/*********************************************************
 * Effect
 *********************************************************/
pub struct EffectExprSchema;
impl Context for EffectExprSchema {
    type ContextItem<'w, 's> = EffectExprContext<'w, 's>;
}
impl ContextMut for EffectExprSchema {
    type ContextItemMut<'w, 's> = EffectExprContextMut<'w, 's>;

    fn as_readonly<'w, 's, 'a>(
        mut_item: &'a Self::ContextItemMut<'w, 's>,
    ) -> Self::ContextItem<'a, 's> {
        EffectExprContext {
            source_actor: mut_item.source_actor.as_readonly(),
            target_actor: mut_item.target_actor.as_ref().map(|v| v.as_readonly()),
            effect_holder: mut_item.effect_holder.as_readonly(),
        }
    }
}

pub struct EffectExprContext<'w, 's> {
    pub source_actor: AttributesRef<'w, 's>,
    pub target_actor: Option<AttributesRef<'w, 's>>,
    pub effect_holder: AttributesRef<'w, 's>,
}

pub struct EffectExprContextMut<'w, 's> {
    pub source_actor: AttributesMut<'w, 's>,
    pub target_actor: Option<AttributesMut<'w, 's>>,
    pub effect_holder: AttributesMut<'w, 's>,
}

impl<'w, 's> ActorProvider<'w, 's, Source> for EffectExprContext<'w, 's> {
    type ActorRef = AttributesRef<'w, 's>;
    fn get_actor(&self) -> &Self::ActorRef {
        &self.source_actor
    }
}
impl<'w, 's> ActorProviderMut<'w, 's, Source> for EffectExprContextMut<'w, 's> {
    type ActorMut = AttributesMut<'w, 's>;
    fn get_actor_mut(&mut self) -> &mut Self::ActorMut {
        &mut self.source_actor
    }
}
impl<'w, 's> ActorProvider<'w, 's, Target> for EffectExprContext<'w, 's> {
    type ActorRef = AttributesRef<'w, 's>;
    fn get_actor(&self) -> &Self::ActorRef {
        if let Some(target) = &self.target_actor {
            target
        } else {
            &self.source_actor
        }
    }
}
impl<'w, 's> ActorProviderMut<'w, 's, Target> for EffectExprContextMut<'w, 's> {
    type ActorMut = AttributesMut<'w, 's>;
    fn get_actor_mut(&mut self) -> &mut Self::ActorMut {
        if let Some(target) = &mut self.target_actor {
            target
        } else {
            &mut self.source_actor
        }
    }
}
impl<'w, 's> ActorProvider<'w, 's, Effect> for EffectExprContext<'w, 's> {
    type ActorRef = AttributesRef<'w, 's>;
    fn get_actor(&self) -> &Self::ActorRef {
        &self.effect_holder
    }
}
impl<'w, 's> ActorProviderMut<'w, 's, Effect> for EffectExprContextMut<'w, 's> {
    type ActorMut = AttributesMut<'w, 's>;
    fn get_actor_mut(&mut self) -> &mut Self::ActorMut {
        &mut self.effect_holder
    }
}

/*********************************************************
 * Ability
 *********************************************************/
pub struct AbilityExprSchema;
impl Context for AbilityExprSchema {
    type ContextItem<'w, 's> = AbilityExprContext<'w, 's>;
}
impl ContextMut for AbilityExprSchema {
    type ContextItemMut<'w, 's> = AbilityExprContextMut<'w, 's>;

    fn as_readonly<'w, 's, 'a>(
        mut_item: &'a Self::ContextItemMut<'w, 's>,
    ) -> Self::ContextItem<'a, 's> {
        AbilityExprContext {
            caster_ref: mut_item.caster_mut.as_readonly(),
            ability_ref: mut_item.ability_mut.as_readonly(),
            target_ref: mut_item.target_mut.as_ref().map(|v| v.as_readonly()),
        }
    }
}

pub struct AbilityExprContext<'w, 's> {
    pub caster_ref: AttributesRef<'w, 's>,
    pub target_ref: Option<AttributesRef<'w, 's>>,
    pub ability_ref: AttributesRef<'w, 's>,
}

pub struct AbilityExprContextMut<'w, 's> {
    pub caster_mut: AttributesMut<'w, 's>,
    pub target_mut: Option<AttributesMut<'w, 's>>,
    pub ability_mut: AttributesMut<'w, 's>,
}

impl<'w, 's> ActorProvider<'w, 's, Caster> for AbilityExprContext<'w, 's> {
    type ActorRef = AttributesRef<'w, 's>;
    fn get_actor(&self) -> &Self::ActorRef {
        &self.caster_ref
    }
}
impl<'w, 's> ActorProvider<'w, 's, Source> for AbilityExprContext<'w, 's> {
    type ActorRef = AttributesRef<'w, 's>;
    fn get_actor(&self) -> &Self::ActorRef {
        &self.caster_ref
    }
}
impl<'w, 's> ActorProviderMut<'w, 's, Caster> for AbilityExprContextMut<'w, 's> {
    type ActorMut = AttributesMut<'w, 's>;
    fn get_actor_mut(&mut self) -> &mut Self::ActorMut {
        &mut self.caster_mut
    }
}
impl<'w, 's> ActorProviderMut<'w, 's, Source> for AbilityExprContextMut<'w, 's> {
    type ActorMut = AttributesMut<'w, 's>;
    fn get_actor_mut(&mut self) -> &mut Self::ActorMut {
        &mut self.caster_mut
    }
}

impl<'w, 's> ActorProvider<'w, 's, Target> for AbilityExprContext<'w, 's> {
    type ActorRef = AttributesRef<'w, 's>;
    fn get_actor(&self) -> &Self::ActorRef {
        if let Some(target) = &self.target_ref {
            target
        } else {
            &self.caster_ref
        }
    }
}
impl<'w, 's> ActorProviderMut<'w, 's, Target> for AbilityExprContextMut<'w, 's> {
    type ActorMut = AttributesMut<'w, 's>;
    fn get_actor_mut(&mut self) -> &mut Self::ActorMut {
        if let Some(target) = &mut self.target_mut {
            target
        } else {
            &mut self.caster_mut
        }
    }
}

impl<'w, 's> ActorProvider<'w, 's, Ability> for AbilityExprContext<'w, 's> {
    type ActorRef = AttributesRef<'w, 's>;
    fn get_actor(&self) -> &Self::ActorRef {
        &self.ability_ref
    }
}
impl<'w, 's> ActorProviderMut<'w, 's, Ability> for AbilityExprContextMut<'w, 's> {
    type ActorMut = AttributesMut<'w, 's>;
    fn get_actor_mut(&mut self) -> &mut Self::ActorMut {
        &mut self.ability_mut
    }
}
