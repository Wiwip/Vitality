use crate::actors::{ActorDef, SpawnActorCommand};
use crate::registry::Registry;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use vitality_core::attributes::Attribute;
use vitality_core::modifier::{AttributeCalculator, ModOp};
use vitality_core::tags::{ChangeTag, TagSpec};
use vitality_effects::definition::EffectDef;
use vitality_effects::{Effects, GlobalActor, GlobalEffects};

/// Actors, effects and attributes in one param. Abilities live in [`vitality_abilities::Abilities`].
#[derive(SystemParam)]
pub struct Vitality<'w, 's> {
    global_actor: Query<'w, 's, Entity, With<GlobalActor>>,
    pub registry: Registry<'w>,
    actor_assets: ResMut<'w, Assets<ActorDef>>,
    effect_assets: ResMut<'w, Assets<EffectDef>>,
    global_effects: ResMut<'w, GlobalEffects>,
    // ahead of `effects`: a SystemParam applies its command buffers in field order, so the spawn lands first
    commands: Commands<'w, 's>,
    pub effects: Effects<'w, 's>,
}

impl<'s, 'w> Vitality<'w, 's> {
    pub fn add_effect(&mut self, effect: EffectDef) -> Handle<EffectDef> {
        self.effect_assets.add(effect)
    }

    pub fn apply_effect_to_target(
        &mut self,
        target: Entity,
        source: Entity,
        handle: &Handle<EffectDef>,
    ) {
        self.effects.apply(source, target, handle);
    }

    pub fn apply_effect_to_self(&mut self, source: Entity, handle: &Handle<EffectDef>) {
        self.effects.apply_to_self(source, handle);
    }

    pub fn apply_dynamic_effect_to_target(
        &mut self,
        target: Entity,
        source: Entity,
        effect: EffectDef,
    ) -> Handle<EffectDef> {
        let handle = self.effect_assets.add(effect);
        self.effects.apply(source, target, &handle);
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
        self.actor_assets.add(actor)
    }

    pub fn spawn_actor_from_handle(&mut self, handle: &Handle<ActorDef>) -> EntityCommands<'_> {
        let mut entity_commands = self.commands.spawn_empty();
        entity_commands.queue(SpawnActorCommand {
            handle: handle.clone(),
        });
        entity_commands
    }

    pub fn add_spawn_actor(&mut self, actor: ActorDef) -> EntityCommands<'_> {
        let handle = self.actor_assets.add(actor);
        self.spawn_actor_from_handle(&handle)
    }

    /// Writes an attribute's base value; the current value follows on the next resolve.
    pub fn set_base<T: Attribute>(&mut self, actor: Entity, value: T::Property) {
        self.modify_base::<T>(actor, ModOp::Set, value);
    }

    /// applies one operation to an attribute's base value, like an instant effect would
    pub fn modify_base<T: Attribute>(&mut self, actor: Entity, op: ModOp, value: T::Property) {
        self.commands
            .entity(actor)
            .queue_silenced(move |mut entity: EntityWorldMut| {
                if let Some(mut attribute) = entity.get_mut::<T>() {
                    let base =
                        AttributeCalculator::<T>::from_op(op, value).eval(attribute.base_value());
                    attribute.set_base_value(base);
                }
            });
    }

    /// Grants a counted tag that stays until a matching [`Self::revoke_tag`].
    pub fn grant_tag<T: Component + Default>(&mut self, actor: Entity) {
        self.commands
            .entity(actor)
            .queue_silenced(ChangeTag::grant(TagSpec::of::<T>()));
    }

    pub fn revoke_tag<T: Component + Default>(&mut self, actor: Entity) {
        self.commands
            .entity(actor)
            .queue_silenced(ChangeTag::revoke(TagSpec::of::<T>()));
    }

    pub fn add_global_effect(&mut self, handle: Handle<EffectDef>) {
        self.global_effects.push(handle);
    }

    /// the actor global effects come from; spawned in `PreStartup`, so None only before the first update
    pub fn global_actor(&self) -> Option<Entity> {
        self.global_actor.single().ok()
    }
}
