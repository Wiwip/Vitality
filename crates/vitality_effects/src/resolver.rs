//! The attribute-agnostic resolver: one exclusive system that folds each dirty actor's modifiers
//! and walks its attributes in dependency order, actors in parallel. Cross-actor reads settle in
//! extra passes inside the same tick.

use crate::components::{AppliedEffects, Effect, EffectInactive, EffectTarget};
use crate::modifier::{AttributeModifier, DependencyProxyOf, ModifierOf, OwnedModifiers};
use crate::systems::MarkActorDirty;
use crate::timing::EffectTicker;
use crate::{EffectSource, EffectsRef, Stacks};
use bevy::ecs::entity::{EntityHashMap, EntityHashSet};
use bevy::ecs::world::FilteredEntityMut;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::utils::Parallel;
use std::any::TypeId;
use vitality_core::attributes::{Attribute, ManageAttributes};
use vitality_core::modifier::ModOp;
use vitality_core::num_traits::AsPrimitive;
use vitality_core::registry::{
    AttrId, AttributeRegistry, ErasedCalc, Mark, ModifierTable, ResolveQueue,
};

/// Passes per tick before leftover cross-actor work waits for the next tick.
pub const MAX_PASSES: usize = 4;

/// The erased mirror of an `AttributeModifier<T>`, which is all a regather reads.
#[derive(Component, Clone, Copy, Debug)]
pub struct ModifierSlot {
    pub attribute: TypeId,
    pub op: ModOp,
    pub value: f64,
}

/// Re-evaluates a live modifier of one attribute type; the effects crate registers one per type.
type Recalc = fn(&mut World, Entity) -> Option<f64>;

#[derive(Resource, Default)]
pub struct ModifierRecalcs(HashMap<TypeId, Recalc>);

impl ModifierRecalcs {
    pub fn register<T: Attribute>(&mut self) {
        self.0.insert(TypeId::of::<T>(), recalc::<T>);
    }
}

#[derive(Resource)]
struct RecalcState<T: Attribute> {
    modifiers: QueryState<(&'static AttributeModifier<T>, &'static ModifierOf)>,
    effects: QueryState<(&'static EffectSource, &'static EffectTarget)>,
    actors: QueryState<EffectsRef<'static, 'static>>,
}

fn recalc<T: Attribute>(world: &mut World, modifier: Entity) -> Option<f64> {
    if !world.contains_resource::<RecalcState<T>>() {
        let state = RecalcState::<T> {
            modifiers: world.query(),
            effects: world.query(),
            actors: world.query(),
        };
        world.insert_resource(state);
    }
    let value = world.resource_scope(|world, mut state: Mut<RecalcState<T>>| {
        let (attribute_modifier, of) = state.modifiers.get(world, modifier).ok()?;
        let expr = attribute_modifier.expr.clone();
        let effect = of.0;
        let (source, target) = state.effects.get(world, effect).ok()?;
        let (source, target) = (source.0, target.0);
        state.actors.update_archetypes(world);
        let actors = state.actors.query_manual(world);
        let source_ref = actors.get(source).ok()?;
        let target_ref = actors.get(target).ok()?;
        let context = crate::context::EffectExprContext {
            source_actor: source_ref,
            target_actor: Some(target_ref),
            effect_holder: source_ref,
        };
        Some(expr.eval(&context))
    })?;
    if let Some(mut attribute_modifier) = world.get_mut::<AttributeModifier<T>>(modifier) {
        attribute_modifier.value = value;
    }
    Some(value.as_())
}

/// Every structural change to an actor's effects arrives as `MarkActorDirty`.
pub(crate) fn mark_actor_dirty_dag(
    trigger: On<MarkActorDirty>,
    registry: Res<AttributeRegistry>,
    queue: Res<ResolveQueue>,
) {
    queue.everything(trigger.entity, registry.len());
}

type ActorItem = (
    Entity,
    FilteredEntityMut<'static, 'static>,
    &'static mut ModifierTable,
);
type ActorQuery = QueryState<ActorItem, With<ManageAttributes>>;

#[derive(Default)]
struct Output {
    changes: Vec<(Entity, AttrId, f64, f64)>,
    readers: Vec<Entity>,
}

/// The per-effect flags the walk needs to know whether an effect folds into a value at all.
type EffectFlags = QueryState<
    (
        &'static OwnedModifiers,
        Option<&'static Stacks>,
        Has<EffectInactive>,
        Has<EffectTicker>,
    ),
    With<Effect>,
>;

pub(crate) struct ResolverState {
    actors: Option<ActorQuery>,
    built_for: usize,
    applied: QueryState<&'static AppliedEffects>,
    effects: EffectFlags,
    slots: QueryState<&'static ModifierSlot>,
    slot_effects: QueryState<(&'static ModifierSlot, &'static ModifierOf)>,
    targets: QueryState<&'static EffectTarget>,
    proxies: QueryState<&'static DependencyProxyOf>,
    output: Parallel<Output>,
}

impl FromWorld for ResolverState {
    fn from_world(world: &mut World) -> Self {
        Self {
            actors: None,
            built_for: 0,
            applied: world.query(),
            effects: world.query_filtered(),
            slots: world.query(),
            slot_effects: world.query(),
            targets: world.query(),
            proxies: world.query(),
            output: Parallel::default(),
        }
    }
}

impl ResolverState {
    fn actor_query(&mut self, world: &mut World) -> &mut ActorQuery {
        let registry = world.resource::<AttributeRegistry>();
        if self.actors.is_none() || self.built_for != registry.len() {
            let ids: Vec<_> = registry
                .infos()
                .iter()
                .map(|info| {
                    let ids = info.ids;
                    (ids.component, ids.dependents, ids.clamp, ids.clamp_bounds)
                })
                .collect();
            let len = registry.len();
            let mut builder = QueryBuilder::<ActorItem, With<ManageAttributes>>::new(world);
            builder.optional(|builder| {
                for &(component, dependents, clamp, bounds) in &ids {
                    builder.mut_id(component);
                    builder.ref_id(dependents);
                    builder.mut_id(clamp);
                    builder.ref_id(bounds);
                }
            });
            // clamp expressions read any attribute of the actor
            self.actors = Some(builder.build());
            self.built_for = len;
        }
        self.actors.as_mut().expect("built above")
    }

    fn regather(&mut self, world: &mut World, marks: &EntityHashMap<Mark>) {
        let mut folded: Vec<(Entity, Vec<(AttrId, ErasedCalc)>)> = Vec::new();
        {
            let registry = world.resource::<AttributeRegistry>();
            for (&actor, mark) in marks {
                if mark.regather.is_empty() {
                    continue;
                }
                let mut calcs: Vec<(AttrId, ErasedCalc)> = Vec::new();
                for id in registry.order() {
                    if mark.regather.contains(*id) {
                        calcs.push((*id, ErasedCalc::default()));
                    }
                }
                if let Ok(applied) = self.applied.get(world, actor) {
                    for effect in applied.iter() {
                        let Ok((modifiers, stacks, inactive, ticking)) =
                            self.effects.get(world, effect)
                        else {
                            continue;
                        };
                        if inactive || ticking {
                            continue;
                        }
                        let stacks = stacks.map_or(1, |stacks| stacks.current_value());
                        for modifier in modifiers.iter() {
                            let Ok(slot) = self.slots.get(world, modifier) else {
                                continue;
                            };
                            let Some(id) = registry.id_of(slot.attribute) else {
                                continue;
                            };
                            if let Some((_, calc)) = calcs.iter_mut().find(|(a, _)| *a == id) {
                                calc.combine(&ErasedCalc::from_op(slot.op, slot.value, stacks));
                            }
                        }
                    }
                }
                folded.push((actor, calcs));
            }
        }
        let query = self.actor_query(world);
        for (actor, calcs) in folded {
            if let Ok((_, _, mut table)) = query.get_mut(world, actor) {
                for (id, calc) in calcs {
                    table.set_calc(id, calc);
                }
            }
        }
    }

    fn resolve(&mut self, world: &mut World, marks: &EntityHashMap<Mark>) {
        let registry = world.resource::<AttributeRegistry>();
        // the query borrows the world mutably, so the registry rides along as raw parts
        let order: Vec<AttrId> = registry.order().to_vec();
        let readers: Vec<Vec<AttrId>> = (0..registry.len())
            .map(|i| registry.readers(AttrId(i as u16)).to_vec())
            .collect();
        let vtables: Vec<_> = registry
            .infos()
            .iter()
            .map(|info| (info.resolve, info.ids, info.dependents_of))
            .collect();

        let dirty: EntityHashSet = marks
            .iter()
            .filter(|(_, mark)| !mark.dirty.is_empty())
            .map(|(entity, _)| *entity)
            .collect();
        let output = &self.output;
        let query_state = self.actors.as_mut().expect("built by regather");
        let mut query = query_state.query_mut(world);
        query.par_iter_many_unique_mut(dirty).for_each(|item| {
            let Ok((entity, mut view, mut table)) = item else {
                return;
            };
            let Some(mark) = marks.get(&entity) else {
                return;
            };
            let mut todo = mark.dirty;
            let mut out = output.borrow_local_mut();
            for &id in &order {
                if !todo.contains(id) {
                    continue;
                }
                let (resolve, ids, dependents_of) = vtables[id.index()];
                let calc = table.calc(id);
                let Some(resolved) = resolve(&mut view, ids, &calc) else {
                    continue;
                };
                table.set_base(id, resolved.base);
                if let Some((old, new)) = resolved.moved {
                    out.changes.push((entity, id, old, new));
                    dependents_of(&view.as_readonly(), ids, &mut out.readers);
                    for &reader in &readers[id.index()] {
                        todo.insert(reader);
                    }
                }
            }
        });
    }

    /// Re-evaluates the live modifiers reading what changed; returns the marks they cause.
    fn recalc(&mut self, world: &mut World, readers: Vec<Entity>) -> EntityHashMap<Mark> {
        let mut next: EntityHashMap<Mark> = EntityHashMap::default();
        if readers.is_empty() {
            return next;
        }
        let mut seen = EntityHashSet::default();
        let modifiers: Vec<Entity> = readers
            .into_iter()
            .map(|reader| {
                self.proxies
                    .get(world, reader)
                    .map_or(reader, |proxy| proxy.0)
            })
            .filter(|modifier| seen.insert(*modifier))
            .collect();
        let recalcs = world.resource::<ModifierRecalcs>().0.clone();
        for modifier in modifiers {
            let Ok((slot, of)) = self.slot_effects.get(world, modifier) else {
                continue;
            };
            let (attribute, old, effect) = (slot.attribute, slot.value, of.0);
            let Some(recalc) = recalcs.get(&attribute) else {
                continue;
            };
            let Some(value) = recalc(world, modifier) else {
                continue;
            };
            if value == old {
                continue;
            }
            if let Some(mut slot) = world.get_mut::<ModifierSlot>(modifier) {
                slot.value = value;
            }
            let Ok(target) = self.targets.get(world, effect) else {
                continue;
            };
            let target = target.0;
            let Some(id) = world.resource::<AttributeRegistry>().id_of(attribute) else {
                continue;
            };
            let mark = next.entry(target).or_default();
            mark.dirty.insert(id);
            mark.regather.insert(id);
        }
        next
    }
}

fn fold(into: &mut EntityHashMap<Mark>, marks: impl IntoIterator<Item = (Entity, Mark)>) {
    for (entity, mark) in marks {
        let entry = into.entry(entity).or_default();
        entry.dirty.union(&mark.dirty);
        entry.regather.union(&mark.regather);
    }
}

pub(crate) fn resolve_attributes(world: &mut World, state: &mut ResolverState) {
    let (pending, dirty) = world.resource::<ResolveQueue>().take();
    if pending.is_empty() && dirty.is_empty() {
        return;
    }
    let mut marks: EntityHashMap<Mark> = EntityHashMap::default();
    fold(&mut marks, pending);
    for (entity, id) in dirty {
        marks.entry(entity).or_default().dirty.insert(id);
    }

    let mut changes = Vec::new();
    for pass in 0..MAX_PASSES {
        state.actor_query(world);
        state.regather(world, &marks);
        state.resolve(world, &marks);

        let mut readers = Vec::new();
        for out in state.output.iter_mut() {
            changes.append(&mut out.changes);
            readers.append(&mut out.readers);
        }
        marks = state.recalc(world, readers);
        if marks.is_empty() {
            break;
        }
        if pass + 1 == MAX_PASSES {
            warn_once!(
                "cross-actor modifiers did not settle in {MAX_PASSES} passes; finishing next tick"
            );
            let queue = world.resource::<ResolveQueue>();
            for (entity, mark) in marks.drain() {
                queue.push(entity, mark);
            }
        }
    }

    if changes.is_empty() {
        return;
    }
    let notifies: Vec<_> = {
        let registry = world.resource::<AttributeRegistry>();
        registry.infos().iter().map(|info| info.notify).collect()
    };
    let mut commands = world.commands();
    for (entity, id, old, new) in changes {
        (notifies[id.index()])(&mut commands, entity, old, new);
    }
    world.flush();
}

pub(crate) fn resolve_system(world: &mut World, mut state: Local<Option<ResolverState>>) {
    let state = state.get_or_insert_with(|| ResolverState::from_world(world));
    resolve_attributes(world, state);
}
