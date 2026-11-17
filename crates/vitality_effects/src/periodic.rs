use crate::EffectsRef;
use crate::context::EffectExprContext;
use crate::definition::EffectDef;
use crate::modifier::{ApplyAttributeModifierMessage, AttributeModifier, OwnedModifiers};
use crate::{Effect, EffectSource, EffectTarget, EffectTicker, Stacks};
use bevy::ecs::resource::IsResource;
use bevy::prelude::*;
use std::any::TypeId;
use vitality_core::attributes::Attribute;

/// Fires one attribute's periodic effects; [`PeriodicApplies`] holds one per registered attribute so
/// periodic application costs a single `FixedUpdate` node for the whole app.
type PeriodicApply = fn(&mut World);

#[derive(Resource, Default)]
pub struct PeriodicApplies(Vec<(TypeId, PeriodicApply)>);

impl PeriodicApplies {
    pub fn register<T: Attribute>(&mut self) {
        let attribute = TypeId::of::<T>();
        if !self
            .0
            .iter()
            .any(|(registered, _)| *registered == attribute)
        {
            self.0.push((attribute, apply_periodic_effects_of::<T>));
        }
    }
}

/// One periodic effect, as the collapsed node reads it.
type PeriodicEffectRow<'w, 's> = (
    EffectsRef<'w, 's>,
    &'w Effect,
    &'w Stacks,
    &'w EffectTicker,
    &'w OwnedModifiers,
    &'w EffectTarget,
    &'w EffectSource,
);

/// The actor, effect and modifier queries of one attribute, kept as a resource so the collapsed node
/// pays nothing per attribute beyond building them once.
#[derive(Resource)]
struct PeriodicState<T: Attribute> {
    actors: QueryState<EffectsRef<'static, 'static>, Without<IsResource>>,
    effects: QueryState<PeriodicEffectRow<'static, 'static>, Without<IsResource>>,
    modifiers: QueryState<&'static AttributeModifier<T>>,
}

/// What [`apply_periodic_effect`] does for one attribute, from world state instead of system state.
fn apply_periodic_effects_of<T: Attribute>(world: &mut World) {
    if !world.contains_resource::<PeriodicState<T>>() {
        let actors = world.query_filtered();
        let effects = world.query_filtered();
        let modifiers = world.query();
        world.insert_resource(PeriodicState::<T> {
            actors,
            effects,
            modifiers,
        });
    }

    // the fires ride out of the world so the queries can hold it at the same time
    let fired = world.resource_scope(|world, mut state: Mut<PeriodicState<T>>| {
        // no assets means no definitions, and a panic here would cost every attribute its tick
        let Some(assets) = world.get_resource::<Assets<EffectDef>>() else {
            return Vec::new();
        };
        let state = &mut *state;
        // get_manual reads the cached archetypes, so refresh them the way a system param would
        state.actors.update_archetypes(world);
        state.modifiers.update_archetypes(world);
        let mut fired = Vec::new();
        for (effect_ref, effect, stacks, timer, owned_modifiers, target, source) in
            state.effects.iter(world)
        {
            if !timer.just_finished() {
                continue;
            }

            let Some(effect_def) = assets.get(&effect.0) else {
                warn!(
                    "Periodic effect {} has no loaded definition.",
                    effect_ref.id()
                );
                continue;
            };
            let (Ok(source_actor_ref), Ok(target_actor_ref)) = (
                state.actors.get_manual(world, source.0),
                state.actors.get_manual(world, target.0),
            ) else {
                continue;
            };

            let context = EffectExprContext {
                target_actor: Some(target_actor_ref),
                source_actor: source_actor_ref,
                effect_holder: effect_ref,
            };

            // Determines whether the effect should activate
            let should_be_activated = effect_def
                .activate_conditions
                .iter()
                .all(|condition| condition.eval(&context));

            if !should_be_activated {
                continue;
            }

            // Timer has triggered. Grab modifiers and apply them.
            for children in owned_modifiers.iter() {
                let Ok(attribute_modifier) = state.modifiers.get_manual(world, children) else {
                    continue;
                };

                fired.push(ApplyAttributeModifierMessage {
                    source_entity: source.0,
                    target_entity: target.0,
                    effect_entity: effect_ref.id(),
                    modifier: attribute_modifier.clone(),
                    stacks: stacks.current_value(),
                });
            }
        }
        fired
    });

    if fired.is_empty() {
        return;
    }
    world
        .get_resource_or_init::<Messages<ApplyAttributeModifierMessage<T>>>()
        .write_batch(fired);
}

/// One node for every attribute: asks each registered attribute to fire the periodic effects that are
/// due, so a periodic effect costs one schedule node however many attributes the app has.
pub fn apply_all_periodic_effects(world: &mut World, mut applies: Local<Vec<PeriodicApply>>) {
    let Some(registry) = world.get_resource::<PeriodicApplies>() else {
        return;
    };
    applies.clear();
    applies.extend(registry.0.iter().map(|(_, apply)| *apply));

    for apply in applies.iter() {
        apply(world);
    }
}
