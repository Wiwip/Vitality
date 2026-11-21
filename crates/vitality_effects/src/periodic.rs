use crate::EffectsRef;
use crate::definition::EffectDef;
use crate::modifier::{ApplyAttributeModifierMessage, AttributeModifier, OwnedModifiers};
use crate::resolver::ModifierSlot;
use crate::{Effect, EffectInactive, EffectSource, EffectTarget, EffectTicker, Stacks};
use bevy_ecs::prelude::*;
use bevy_ecs::resource::IsResource;
use bevy_platform::collections::HashMap;
use std::any::TypeId;
use tracing::warn;
use vitality_core::attributes::Attribute;
use vitality_core::store::Assets;

/// fires one attribute's due writes; [`PeriodicApplies`] holds one per registered attribute so periodic application costs a single `FixedUpdate` node
type PeriodicApply = fn(&mut World, &[DueModifier]);

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

type PeriodicEffectRow<'w, 's> = (
    EffectsRef<'w, 's>,
    &'w Effect,
    &'w Stacks,
    &'w EffectTicker,
    &'w OwnedModifiers,
    &'w EffectTarget,
    &'w EffectSource,
    Option<&'w EffectInactive>,
);

/// one write a due effect owes, with the verdict behind it already taken
#[derive(Clone, Copy)]
struct DueModifier {
    source: Entity,
    target: Entity,
    effect: Entity,
    stacks: u32,
    modifier: Entity,
}

/// kept as a resource so the collapsed node walks the tick table once per tick instead of once per attribute
#[derive(Resource)]
struct PeriodicPass {
    actors: QueryState<EffectsRef<'static, 'static>, Without<IsResource>>,
    effects: QueryState<PeriodicEffectRow<'static, 'static>, Without<IsResource>>,
    slots: QueryState<&'static ModifierSlot>,
    applies: Vec<(TypeId, PeriodicApply)>,
    /// attribute type -> bucket of `buckets`, rebuilt when an attribute registers
    index: HashMap<TypeId, usize>,
    buckets: Vec<Vec<DueModifier>>,
}

impl PeriodicPass {
    fn collect(&mut self, world: &mut World) {
        if self.index.len() != self.applies.len() {
            self.index.clear();
            self.buckets.clear();
            for (index, (attribute, _)) in self.applies.iter().enumerate() {
                self.index.insert(*attribute, index);
                self.buckets.push(Vec::new());
            }
        }
        for bucket in &mut self.buckets {
            bucket.clear();
        }

        // no assets means no definitions, and a panic here would cost every attribute its tick
        let Some(assets) = world.get_resource::<Assets<EffectDef>>() else {
            return;
        };
        // get_manual reads the cached archetypes, so refresh them the way a system param would
        self.actors.update_archetypes(world);
        self.effects.update_archetypes(world);
        self.slots.update_archetypes(world);
        for (effect_ref, effect, stacks, timer, owned_modifiers, target, source, inactive) in
            self.effects.iter(world)
        {
            if !timer.just_finished() {
                continue;
            }
            // the condition pass owns the verdict, so the fire reads the marker instead of rerunning it
            if inactive.is_some() {
                continue;
            }
            if assets.get(&effect.0).is_none() {
                warn!(
                    "Periodic effect {} tick skipped: its EffectDef is not in \
                     Assets<EffectDef>; keep the asset loaded for the effect's whole lifetime",
                    effect_ref.id()
                );
                continue;
            }

            // an actor that is gone has nowhere for the write to land
            if self.actors.get_manual(world, source.0).is_err()
                || self.actors.get_manual(world, target.0).is_err()
            {
                continue;
            }

            for children in owned_modifiers.iter() {
                // the slot says whose queue the write belongs to, so no attribute tries every modifier
                let Ok(slot) = self.slots.get_manual(world, children) else {
                    continue;
                };
                let Some(&index) = self.index.get(&slot.attribute) else {
                    continue;
                };
                self.buckets[index].push(DueModifier {
                    source: source.0,
                    target: target.0,
                    effect: effect_ref.id(),
                    stacks: stacks.current_value(),
                    modifier: children,
                });
            }
        }
    }
}

/// the attribute's own share of this tick's fires, so it never reads another attribute's effects
fn apply_periodic_effects_of<T: Attribute>(world: &mut World, due: &[DueModifier]) {
    if !world.contains_resource::<PeriodicState<T>>() {
        let modifiers = world.query();
        world.insert_resource(PeriodicState::<T> { modifiers });
    }

    // the fires ride out of the world so the query can hold it at the same time
    let fired = world.resource_scope(|world, mut state: Mut<PeriodicState<T>>| {
        let state = &mut *state;
        state.modifiers.update_archetypes(world);
        let mut fired = Vec::new();
        for item in due {
            let Ok(attribute_modifier) = state.modifiers.get_manual(world, item.modifier) else {
                continue;
            };

            fired.push(ApplyAttributeModifierMessage {
                source_entity: item.source,
                target_entity: item.target,
                effect_entity: item.effect,
                modifier: attribute_modifier.clone(),
                stacks: item.stacks,
            });
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

/// asks each registered attribute to fire the periodic effects that are due, so a periodic effect costs one schedule node however many attributes the app has
pub fn apply_all_periodic_effects(world: &mut World) {
    if !world.contains_resource::<PeriodicApplies>() {
        return;
    }
    if !world.contains_resource::<PeriodicPass>() {
        let actors = world.query_filtered();
        let effects = world.query_filtered();
        let slots = world.query();
        world.insert_resource(PeriodicPass {
            actors,
            effects,
            slots,
            applies: Vec::new(),
            index: HashMap::default(),
            buckets: Vec::new(),
        });
    }

    world.resource_scope(|world, mut pass: Mut<PeriodicPass>| {
        let registry = world.resource::<PeriodicApplies>();
        pass.applies.clear();
        pass.applies.extend(registry.0.iter().copied());

        pass.collect(world);
        for (index, (_, apply)) in pass.applies.iter().enumerate() {
            let due = &pass.buckets[index];
            if !due.is_empty() {
                apply(world, due);
            }
        }
    });
}

/// kept as a resource so the collapsed node pays nothing per attribute beyond building its query once
#[derive(Resource)]
struct PeriodicState<T: Attribute> {
    modifiers: QueryState<&'static AttributeModifier<T>>,
}
