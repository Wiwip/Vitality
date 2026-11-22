//! one exclusive system resolves dirty entities in dependency order; cross-actor reads settle in later passes

use crate::components::{AppliedEffects, Effect, EffectInactive, EffectTarget};
use crate::conditions::ConditionRng;
use crate::modifier::{
    AttributeModifier, DependencyProxyOf, EffectSubject, ModifierOf, OwnedModifiers,
};
use crate::systems::MarkActorDirty;
use crate::timing::EffectTicker;
use crate::{EffectSource, EffectsRef, Stacks};
use bevy_ecs::component::ComponentId;
use bevy_ecs::entity::{EntityHashMap, EntityHashSet};
use bevy_ecs::prelude::*;
use bevy_ecs::world::FilteredEntityMut;
use bevy_platform::collections::HashMap;
use bevy_utils::Parallel;
use std::any::TypeId;
use std::sync::Arc;
use vitality_core::attributes::{Attribute, ManageAttributes};
use vitality_core::modifier::{Calc, ModOp};
use vitality_core::registry::{
    AppendPairs, AttrComponents, AttrId, AttrSet, AttributeRegistry, Change, DependentsOf,
    FoldModifier, Kind, Mark, ModifierTable, ResetCalcs, ResolveFn, ResolveQueue, ValueBuf,
    WriteCalcs, add_clamp_edges,
};
use vitality_core::warn_once;

/// Passes per tick before leftover cross-actor work waits for the next tick.
const MAX_PASSES: usize = 4;

/// The erased mirror of an `AttributeModifier<T>`; the magnitude stays on the typed component.
#[derive(Component, Clone, Copy, Debug)]
pub struct ModifierSlot {
    pub attribute: TypeId,
    pub op: ModOp,
    pub who: EffectSubject,
}

/// Re-evaluates a live modifier of one attribute type; true when its value moved.
type Recalc = fn(&mut World, Entity) -> bool;

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

fn recalc<T: Attribute>(world: &mut World, modifier: Entity) -> bool {
    if !world.contains_resource::<RecalcState<T>>() {
        let state = RecalcState::<T> {
            modifiers: world.query(),
            effects: world.query(),
            actors: world.query(),
        };
        world.insert_resource(state);
    }
    let Some(value) = world.resource_scope(|world, mut state: Mut<RecalcState<T>>| {
        let world: &World = world;
        let rng = world.resource::<ConditionRng>();
        let (attribute_modifier, of) = state.modifiers.get(world, modifier).ok()?;
        let expr = attribute_modifier.expr.clone();
        let effect = of.0;
        let (source, target) = state.effects.get(world, effect).ok()?;
        let (source, target) = (source.0, target.0);
        state.actors.update_archetypes(world);
        let actors = state.actors.query_manual(world);
        let source_ref = actors.get(source).ok()?;
        let target_ref = actors.get(target).ok()?;
        let effect_ref = actors.get(effect).ok()?;
        let context = crate::context::EffectExprContext {
            source_actor: source_ref,
            target_actor: Some(target_ref),
            effect_holder: effect_ref,
            rng,
        };
        Some(expr.eval(&context))
    }) else {
        return false;
    };
    let Some(mut attribute_modifier) = world.get_mut::<AttributeModifier<T>>(modifier) else {
        return false;
    };
    // NaN != NaN, so a NaN modifier keeps fanning out
    if attribute_modifier.value == value {
        return false;
    }
    attribute_modifier.value = value;
    true
}

pub(crate) fn fold_modifier<T: Attribute>(
    scratch: &mut ValueBuf,
    kind: Kind,
    id: AttrId,
    world: &World,
    modifier: Entity,
    stacks: u32,
) {
    let Some(attribute_modifier) = world.get::<AttributeModifier<T>>(modifier) else {
        return;
    };
    let folded = scratch.calc_mut::<T::Property>(kind, id);
    folded.combine(&Calc::from_op_stacked(
        attribute_modifier.operation,
        attribute_modifier.value,
        stacks,
    ));
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

/// an entity carrying attributes with no `ManageAttributes`, and so no [`ModifierTable`] to hold a fold
type CarrierItem = (Entity, FilteredEntityMut<'static, 'static>);
type CarrierQuery = QueryState<CarrierItem, Without<ManageAttributes>>;

/// one attribute's components, in the order the erased view wants them granted
type AttrIds = (ComponentId, ComponentId, ComponentId, ComponentId);

/// hands the erased view one access per component of every registered attribute
fn grant_attributes(builder: &mut QueryBuilder<'_, ()>, ids: &[AttrIds]) {
    for &(component, dependents, clamp, bounds) in ids {
        builder.mut_id(component);
        builder.ref_id(dependents);
        builder.mut_id(clamp);
        builder.ref_id(bounds);
    }
}

/// copied out of the registry so a walk can use them while it holds the world
#[derive(Clone, Copy)]
struct AttrVTable {
    kind: Kind,
    ids: AttrComponents,
    resolve: ResolveFn,
    dependents_of: DependentsOf,
    fold_modifier: FoldModifier,
}

impl AttrVTable {
    fn of(info: &vitality_core::registry::AttrInfo) -> Self {
        Self {
            kind: info.kind,
            ids: info.ids,
            resolve: info.resolve,
            dependents_of: info.dependents_of,
            fold_modifier: info.fold_modifier,
        }
    }
}

#[derive(Clone, Copy)]
struct KindPlan {
    kind: Kind,
    members: AttrSet,
    reset: ResetCalcs,
    write: WriteCalcs,
}

struct GatherState {
    by_type: HashMap<TypeId, AttrId>,
    vtables: Vec<AttrVTable>,
    kinds: Vec<KindPlan>,
}

impl GatherState {
    fn of(registry: &AttributeRegistry) -> Self {
        Self {
            by_type: registry
                .infos()
                .iter()
                .map(|info| (info.type_id, info.id))
                .collect(),
            vtables: registry.infos().iter().map(AttrVTable::of).collect(),
            kinds: registry
                .kinds()
                .iter()
                .enumerate()
                .map(|(kind, info)| KindPlan {
                    kind,
                    members: info.members,
                    reset: info.reset_calcs,
                    write: info.write_calcs,
                })
                .collect(),
        }
    }
}

/// the registry copies every pass walks, rebuilt only when the registry itself moves
struct Plan {
    revision: u64,
    gather: GatherState,
    order: Vec<AttrId>,
    readers: Vec<Vec<AttrId>>,
}

impl Plan {
    fn of(registry: &AttributeRegistry) -> Self {
        Self {
            revision: registry.revision(),
            gather: GatherState::of(registry),
            order: registry.order().to_vec(),
            readers: (0..registry.len())
                .map(|i| registry.readers(AttrId(i as u16)).to_vec())
                .collect(),
        }
    }
}

#[derive(Default)]
struct Output {
    changes: Vec<(Entity, AttrId, Change)>,
    pairs: ValueBuf,
    readers: Vec<Entity>,
    /// the limits of a clamp evaluated for the first time, before their edges exist
    clamp_reads: Vec<(Entity, TypeId, TypeId)>,
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
    carriers: Option<CarrierQuery>,
    built_for: usize,
    plan: Arc<Plan>,
    applied: QueryState<&'static AppliedEffects>,
    effects: EffectFlags,
    slots: QueryState<&'static ModifierSlot>,
    slot_effects: QueryState<(&'static ModifierSlot, &'static ModifierOf)>,
    targets: QueryState<&'static EffectTarget>,
    proxies: QueryState<&'static DependencyProxyOf>,
    /// kept so its buffers stay allocated
    scratch: ValueBuf,
    /// one fold buffer per marked carrier, kept so a steady world allocates nothing
    carrier_calcs: Vec<ValueBuf>,
    output: Parallel<Output>,
}

impl FromWorld for ResolverState {
    fn from_world(world: &mut World) -> Self {
        Self {
            actors: None,
            carriers: None,
            built_for: 0,
            plan: Arc::new(Plan::of(world.resource::<AttributeRegistry>())),
            applied: world.query(),
            effects: world.query_filtered(),
            slots: world.query(),
            slot_effects: world.query(),
            targets: world.query(),
            proxies: world.query(),
            scratch: ValueBuf::default(),
            carrier_calcs: Vec::new(),
            output: Parallel::default(),
        }
    }
}

impl ResolverState {
    /// the pass walks ride on shared registry copies, so only a moved registry rebuilds them
    fn sync_plan(&mut self, world: &World) {
        let registry = world.resource::<AttributeRegistry>();
        if registry.revision() != self.plan.revision {
            self.plan = Arc::new(Plan::of(registry));
        }
    }

    fn actor_query(&mut self, world: &mut World) -> &mut ActorQuery {
        let registry = world.resource::<AttributeRegistry>();
        if self.actors.is_none() || self.carriers.is_none() || self.built_for != registry.len() {
            let ids: Vec<AttrIds> = registry
                .infos()
                .iter()
                .map(|info| {
                    let ids = info.ids;
                    (ids.component, ids.dependents, ids.clamp, ids.clamp_bounds)
                })
                .collect();
            let len = registry.len();
            let mut builder = QueryBuilder::<ActorItem, With<ManageAttributes>>::new(world);
            builder.optional(|builder| grant_attributes(builder, &ids));
            // clamp expressions read any attribute of the actor
            self.actors = Some(builder.build());
            let mut builder = QueryBuilder::<CarrierItem, Without<ManageAttributes>>::new(world);
            builder.optional(|builder| grant_attributes(builder, &ids));
            self.carriers = Some(builder.build());
            self.built_for = len;
        }
        self.actors.as_mut().expect("built above")
    }

    /// The fold of every attribute in `wanted` that `entity`'s own effects carry, into `scratch`.
    fn gather(
        &mut self,
        world: &World,
        state: &GatherState,
        entity: Entity,
        wanted: AttrSet,
        scratch: &mut ValueBuf,
    ) {
        for plan in &state.kinds {
            let ids = wanted.intersection(&plan.members);
            if !ids.is_empty() {
                (plan.reset)(scratch, plan.kind, ids);
            }
        }
        // an effect entity is the write home of the modifiers that name it, so it folds its own children
        let own = self.effects.get(world, entity).is_ok();
        let applied = self.applied.get(world, entity).ok();
        for effect in applied
            .into_iter()
            .flat_map(|applied| applied.iter())
            .chain(own.then_some(entity))
        {
            let Ok((modifiers, stacks, inactive, ticking)) = self.effects.get(world, effect) else {
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
                // an `Effect` modifier folds onto the effect entity, the rest onto the entity carrying it
                if matches!(slot.who, EffectSubject::Effect) != own {
                    continue;
                }
                let Some(&id) = state.by_type.get(&slot.attribute) else {
                    continue;
                };
                if !wanted.contains(id) {
                    continue;
                }
                let vtable = state.vtables[id.index()];
                (vtable.fold_modifier)(scratch, vtable.kind, id, world, modifier, stacks);
            }
        }
    }

    fn regather(&mut self, world: &mut World, marks: &EntityHashMap<Mark>) {
        let mut marked: Vec<(Entity, AttrSet)> = marks
            .iter()
            .filter(|(_, mark)| !mark.regather.is_empty())
            .map(|(entity, mark)| (*entity, mark.regather))
            .collect();
        if marked.is_empty() {
            return;
        }
        // hash map order isn't deterministic
        marked.sort_by_key(|(entity, _)| *entity);
        let plan = self.plan.clone();
        let state = &plan.gather;
        let mut scratch = std::mem::take(&mut self.scratch);
        // taken out so the gather can borrow self
        self.actor_query(world);
        let mut actors = self.actors.take().expect("built above");
        for (actor, wanted) in &marked {
            self.gather(world, state, *actor, *wanted, &mut scratch);
            if let Ok((_, _, mut table)) = actors.get_mut(world, *actor) {
                for plan in &state.kinds {
                    let ids = wanted.intersection(&plan.members);
                    if !ids.is_empty() {
                        (plan.write)(&mut table, plan.kind, ids, &scratch);
                    }
                }
            }
        }
        self.actors = Some(actors);
        self.scratch = scratch;
    }

    fn resolve(&mut self, world: &mut World, marks: &EntityHashMap<Mark>) {
        // the query borrows the world mutably, so the registry rides along as a shared plan
        let plan = self.plan.clone();
        let order = &plan.order;
        let readers = &plan.readers;
        let vtables = &plan.gather.vtables;

        let dirty: EntityHashSet = marks
            .iter()
            .filter(|(_, mark)| !mark.dirty.is_empty())
            .map(|(entity, _)| *entity)
            .collect();
        let output = &self.output;
        let query_state = self.actors.as_mut().expect("built by regather");
        let mut query = query_state.query_mut(world);
        query.par_iter_many_unique_mut(dirty).for_each(|item| {
            let Ok((entity, mut view, table)) = item else {
                return;
            };
            let Some(mark) = marks.get(&entity) else {
                return;
            };
            let mut todo = mark.dirty;
            let mut local = output.borrow_local_mut();
            let Output {
                changes,
                pairs,
                readers: walked_readers,
                clamp_reads,
            } = &mut *local;
            for &id in order {
                if !todo.contains(id) {
                    continue;
                }
                let vtable = vtables[id.index()];
                let Some(change) = (vtable.resolve)(
                    &mut view,
                    vtable.ids,
                    vtable.kind,
                    id,
                    &table.values,
                    pairs,
                    clamp_reads,
                ) else {
                    continue;
                };
                changes.push((entity, id, change));
                (vtable.dependents_of)(&view.as_readonly(), vtable.ids, walked_readers);
                for &reader in &readers[id.index()] {
                    todo.insert(reader);
                }
            }
        });
    }

    /// no `ManageAttributes`, so no [`ModifierTable`]: fold and write in one step, missing attributes nothing
    fn resolve_carriers(&mut self, world: &mut World, marks: &EntityHashMap<Mark>) {
        let mut marked: Vec<(Entity, Mark)> = marks
            .iter()
            .filter(|(_, mark)| !mark.dirty.is_empty())
            .map(|(entity, mark)| (*entity, *mark))
            .filter(|(entity, _)| world.get::<ManageAttributes>(*entity).is_none())
            .collect();
        // hash map order isn't deterministic
        marked.sort_by_key(|(entity, _)| *entity);
        if marked.is_empty() {
            return;
        }

        // every attribute, because a value that moves can pull another one of the same entity into the walk
        let everything = AttrSet::all(world.resource::<AttributeRegistry>().len());
        let plan = self.plan.clone();
        let order = &plan.order;
        let readers = &plan.readers;
        // the gather needs &mut self, so the pool leaves it for the walk and comes back at the end
        let mut pool = std::mem::take(&mut self.carrier_calcs);
        pool.resize_with(marked.len(), ValueBuf::default);
        for (index, (entity, _)) in marked.iter().enumerate() {
            self.gather(world, &plan.gather, *entity, everything, &mut pool[index]);
        }
        let vtables = &plan.gather.vtables;

        let output = &self.output;
        let query = self.carriers.as_mut().expect("built by actor_query");
        for ((entity, mark), calcs) in marked.iter().zip(&pool) {
            let Ok((_, mut view)) = query.get_mut(world, *entity) else {
                continue;
            };
            let mut todo = mark.dirty;
            let mut local = output.borrow_local_mut();
            let Output {
                changes,
                pairs,
                readers: walked_readers,
                clamp_reads,
            } = &mut *local;
            for &id in order {
                if !todo.contains(id) {
                    continue;
                }
                let vtable = vtables[id.index()];
                let Some(change) = (vtable.resolve)(
                    &mut view,
                    vtable.ids,
                    vtable.kind,
                    id,
                    calcs,
                    pairs,
                    clamp_reads,
                ) else {
                    continue;
                };
                changes.push((*entity, id, change));
                (vtable.dependents_of)(&view.as_readonly(), vtable.ids, walked_readers);
                for &reader in &readers[id.index()] {
                    todo.insert(reader);
                }
            }
        }
        self.carrier_calcs = pool;
    }

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
        for modifier in modifiers {
            let Ok((slot, of)) = self.slot_effects.get(world, modifier) else {
                continue;
            };
            let (attribute, who, effect) = (slot.attribute, slot.who, of.0);
            // just the pointer, the walk needs the world back to run it
            let Some(recalc) = world
                .resource::<ModifierRecalcs>()
                .0
                .get(&attribute)
                .copied()
            else {
                continue;
            };
            if !recalc(world, modifier) {
                continue;
            }
            let Ok(target) = self.targets.get(world, effect) else {
                continue;
            };
            let Some(id) = world.resource::<AttributeRegistry>().id_of(attribute) else {
                continue;
            };
            // the new value dirties the entity the fold lands on, the effect itself for an `Effect` modifier
            let holder = if matches!(who, EffectSubject::Effect) {
                effect
            } else {
                target.0
            };
            let mark = next.entry(holder).or_default();
            mark.dirty.insert(id);
            mark.regather.insert(id);
        }
        next
    }

    /// Merges every thread's output, shifting each row's pair index by its kind's offset.
    fn merge_threads(
        &mut self,
        append: &[AppendPairs],
        changes: &mut Vec<(Entity, AttrId, Change)>,
        all_pairs: &mut ValueBuf,
        readers: &mut Vec<Entity>,
        clamp_reads: &mut Vec<(Entity, TypeId, TypeId)>,
    ) {
        let mut offset = vec![0u32; append.len()];
        for out in self.output.iter_mut() {
            let out = &mut *out;
            for (kind, append) in append.iter().enumerate() {
                offset[kind] = append(all_pairs, &mut out.pairs, kind);
            }
            for row in out.changes.iter_mut() {
                row.2.pair += offset[row.2.kind];
            }
            changes.append(&mut out.changes);
            readers.append(&mut out.readers);
            clamp_reads.append(&mut out.clamp_reads);
        }
    }
}

fn fold(into: &mut EntityHashMap<Mark>, marks: impl IntoIterator<Item = (Entity, Mark)>) {
    for (entity, mark) in marks {
        let entry = into.entry(entity).or_default();
        entry.dirty.union(&mark.dirty);
        entry.regather.union(&mark.regather);
    }
}

fn resolve_attributes(world: &mut World, state: &mut ResolverState) {
    let (pending, dirty) = world.resource::<ResolveQueue>().take();
    if pending.is_empty() && dirty.is_empty() {
        return;
    }
    let mut marks: EntityHashMap<Mark> = EntityHashMap::default();
    fold(&mut marks, pending);
    for (entity, id) in dirty {
        marks.entry(entity).or_default().dirty.insert(id);
    }
    let (append, notifies, watchers): (Vec<_>, Vec<_>, Vec<_>) = {
        let registry = world.resource::<AttributeRegistry>();
        (
            registry
                .kinds()
                .iter()
                .map(|kind| kind.append_pairs)
                .collect(),
            registry.infos().iter().map(|info| info.notify).collect(),
            registry
                .infos()
                .iter()
                .map(|info| info.has_observers)
                .collect(),
        )
    };

    let mut changes: Vec<(Entity, AttrId, Change)> = Vec::new();
    let mut all_pairs = ValueBuf::default();
    let mut clamp_reads = Vec::new();
    for pass in 0..MAX_PASSES {
        state.actor_query(world);
        state.sync_plan(world);
        state.regather(world, &marks);
        state.resolve(world, &marks);
        state.resolve_carriers(world, &marks);

        // stable sort by entity, so the result doesn't depend on which thread walked what
        let (pass_changes, pass_clamps) = (changes.len(), clamp_reads.len());
        let mut readers = Vec::new();
        state.merge_threads(
            &append,
            &mut changes,
            &mut all_pairs,
            &mut readers,
            &mut clamp_reads,
        );
        changes[pass_changes..].sort_by_key(|change| change.0);
        clamp_reads[pass_clamps..].sort_by_key(|read| read.0);
        readers.sort();
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

    // the walk already read this tick's order, so a new limit edge has to mark the attribute it governs
    let connected = add_clamp_edges(world, clamp_reads.into_iter());
    if !connected.is_empty() {
        let queue = world.resource::<ResolveQueue>();
        for (actor, id) in connected {
            queue.dirty(actor, id);
        }
    }

    if changes.is_empty() {
        return;
    }
    // an unobserved trigger still costs a queued command and a flush, so decide once per attribute
    let watched: Vec<bool> = watchers.iter().map(|probe| probe(world)).collect();
    let mut commands = world.commands();
    for (entity, id, change) in changes {
        if watched[id.index()] {
            (notifies[id.index()])(&mut commands, entity, change, &all_pairs);
        }
    }
    world.flush();
}

pub(crate) fn resolve_system(world: &mut World, mut state: Local<Option<ResolverState>>) {
    let state = state.get_or_insert_with(|| ResolverState::from_world(world));
    resolve_attributes(world, state);
}
