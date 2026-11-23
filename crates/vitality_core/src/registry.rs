//! Erased per-attribute operations and the type-level dependency DAG the resolver walks.

use crate::CurrentValueChanged;
use crate::attributes::{Attribute, AttributeDependents, Clamp, ClampBounds, record_dependencies};
use crate::context::ActorExprContext;
use crate::modifier::Calc;
use crate::util::pretty_type_name;
use crate::value::AttributeValue;
use bevy_ecs::change_detection::Tick;
use bevy_ecs::component::ComponentId;
use bevy_ecs::prelude::*;
use bevy_ecs::world::{FilteredEntityMut, FilteredEntityRef, World};
use bevy_platform::collections::{HashMap, HashSet};
use bevy_reflect::prelude::*;
use std::any::{Any, TypeId};
use std::fmt;
use std::sync::{Mutex, OnceLock};
use tracing::warn;

/// Dense index of a registered attribute.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Reflect)]
pub struct AttrId(pub u16);

impl AttrId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// Index of a distinct property type; picks the typed buffer an attribute's values live in.
pub type Kind = usize;

/// Where a changed value's old/new pair sits in a [`ValueBuf`].
#[derive(Clone, Copy, Debug)]
pub struct Change {
    pub kind: Kind,
    pub pair: u32,
}

type Slots = Vec<Option<Box<dyn Any + Send + Sync>>>;

/// One typed buffer per [`Kind`]; a wrong downcast is an internal bug, so the generic fns panic.
#[derive(Default)]
pub struct ValueBuf {
    /// `Vec<Calc<P>>` per kind, dense by [`AttrId`]
    calcs: Slots,
    /// `Vec<(P, P)>` per kind, in walk order
    pairs: Slots,
}

impl fmt::Debug for ValueBuf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ValueBuf({} kind(s) of calcs, {} kind(s) of pairs)",
            self.calcs.len(),
            self.pairs.len()
        )
    }
}

impl ValueBuf {
    pub fn calc<P: AttributeValue>(&self, kind: Kind, id: AttrId) -> Option<Calc<P>> {
        self.calcs
            .get(kind)?
            .as_ref()?
            .downcast_ref::<Vec<Calc<P>>>()?
            .get(id.index())
            .copied()
    }

    pub fn calcs_of<P: AttributeValue>(&self, kind: Kind) -> Option<&[Calc<P>]> {
        self.calcs
            .get(kind)?
            .as_ref()?
            .downcast_ref::<Vec<Calc<P>>>()
            .map(Vec::as_slice)
    }

    pub fn calcs_slot<P: AttributeValue>(&mut self, kind: Kind) -> &mut Vec<Calc<P>> {
        typed_slot::<Vec<Calc<P>>>(&mut self.calcs, kind)
    }

    pub fn calc_mut<P: AttributeValue>(&mut self, kind: Kind, id: AttrId) -> &mut Calc<P> {
        let rows = typed_slot::<Vec<Calc<P>>>(&mut self.calcs, kind);
        reserve(rows, id);
        &mut rows[id.index()]
    }

    /// answers the pair's index for [`ValueBuf::pair`]
    pub fn push_change<P: AttributeValue>(&mut self, kind: Kind, old: P, new: P) -> u32 {
        let rows = typed_slot::<Vec<(P, P)>>(&mut self.pairs, kind);
        rows.push((old, new));
        (rows.len() - 1) as u32
    }

    pub fn pair<P: AttributeValue>(&self, kind: Kind, pair: u32) -> (P, P) {
        let rows = self.pairs[kind]
            .as_ref()
            .and_then(|cell| cell.downcast_ref::<Vec<(P, P)>>())
            .expect("a change names a kind whose pairs were written");
        rows[pair as usize]
    }

    /// answers the offset to shift `other`'s pair indices by
    pub fn append_pairs<P: AttributeValue>(&mut self, other: &mut Self, kind: Kind) -> u32 {
        let rows = typed_slot::<Vec<(P, P)>>(&mut self.pairs, kind);
        let offset = rows.len() as u32;
        // append, not take, so the thread's buffer keeps its capacity
        if let Some(moved) = other
            .pairs
            .get_mut(kind)
            .and_then(|cell| cell.as_mut())
            .map(|cell| {
                cell.downcast_mut::<Vec<(P, P)>>()
                    .expect("a kind holds the property type registered for it")
            })
        {
            rows.append(moved);
        }
        offset
    }
}

fn reserve<P: AttributeValue>(rows: &mut Vec<Calc<P>>, id: AttrId) {
    if rows.len() <= id.index() {
        rows.resize(id.index() + 1, Calc::default());
    }
}

fn typed_slot<T: Any + Send + Sync + Default>(slots: &mut Slots, kind: Kind) -> &mut T {
    if slots.len() <= kind {
        slots.resize_with(kind + 1, || None);
    }
    let cell = slots[kind].get_or_insert_with(|| Box::new(T::default()));
    cell.downcast_mut::<T>()
        .expect("a kind holds the property type registered for it")
}

pub type AppendPairs = fn(&mut ValueBuf, &mut ValueBuf, Kind) -> u32;

pub type DependentsOf = fn(&FilteredEntityRef, AttrComponents, &mut Vec<Entity>);

/// clears the scratch so a removed modifier stops counting
pub type ResetCalcs = fn(&mut ValueBuf, Kind, AttrSet);

pub type WriteCalcs = fn(&mut ModifierTable, Kind, AttrSet, &ValueBuf);

/// set by the effects crate, which owns the modifier components
pub type FoldModifier = fn(&mut ValueBuf, Kind, AttrId, &World, Entity, u32);

pub type BaseWriteScan = fn(&mut World, AttrId, &mut Vec<(Entity, AttrId)>);

/// Spelled out inside the struct it sits over clippy's complexity limit.
pub type ResolveFn = fn(
    &mut FilteredEntityMut,
    AttrComponents,
    Kind,
    AttrId,
    &ValueBuf,
    Option<&mut ValueBuf>,
    &mut Vec<(Entity, TypeId, TypeId)>,
) -> Option<Change>;

/// What the resolver needs to do to one attribute without knowing its type.
pub struct AttrInfo {
    pub id: AttrId,
    pub name: String,
    pub type_id: TypeId,
    pub kind: Kind,
    pub ids: AttrComponents,
    /// None when the actor lacks it; the sink takes limits a first clamp pass read, see [`add_clamp_edges`]
    pub resolve: ResolveFn,
    pub dependents_of: DependentsOf,
    pub notify: fn(&mut Commands, Entity, Change, &ValueBuf),
    /// Whether anything would receive this attribute's `CurrentValueChanged` trigger.
    pub has_observers: fn(&World) -> bool,
    /// Entities of this attribute whose base moved since the last scan, for [`scan_base_writes`].
    pub base_write_scan: BaseWriteScan,
    pub fold_modifier: FoldModifier,
}

/// Component ids of an attribute and the components that ride along with it, resolved once.
#[derive(Clone, Copy, Debug)]
pub struct AttrComponents {
    pub component: ComponentId,
    pub dependents: ComponentId,
    pub clamp: ComponentId,
    pub clamp_bounds: ComponentId,
}

#[derive(Clone, Debug)]
pub struct KindInfo {
    pub append_pairs: AppendPairs,
    pub members: AttrSet,
    pub reset_calcs: ResetCalcs,
    pub write_calcs: WriteCalcs,
}

// SAFETY (all three below): every id in `ids` was registered for exactly the type it is read as.
fn resolve<T: Attribute>(
    entity: &mut FilteredEntityMut,
    ids: AttrComponents,
    kind: Kind,
    id: AttrId,
    folded: &ValueBuf,
    changes: Option<&mut ValueBuf>,
    clamp_reads: &mut Vec<(Entity, TypeId, TypeId)>,
) -> Option<Change> {
    let mut clamp: Option<Clamp<T>> = None;
    let bounded = entity.get_by_id(ids.clamp_bounds).is_some();
    if bounded {
        // a limit only reports the attributes it reads while recording, and one pass per clamp is enough
        let capture = entity
            .get_by_id(ids.clamp)
            .is_some_and(|clamp| unsafe { !clamp.deref::<Clamp<T>>().ready });
        let (limits, reads) = {
            let view = entity.as_readonly();
            let bounds = unsafe { view.get_by_id(ids.clamp_bounds)?.deref::<ClampBounds<T>>() };
            let context = ActorExprContext {
                actor_context: view,
            };
            let eval = || (bounds.min.eval(&context), bounds.max.eval(&context));
            if capture {
                record_dependencies(eval)
            } else {
                (eval(), Default::default())
            }
        };
        clamp_reads.extend(
            reads
                .into_iter()
                .map(|key| (entity.id(), key, TypeId::of::<T>())),
        );
        if let Some(untyped) = entity.get_mut_by_id(ids.clamp) {
            let mut stored = unsafe { untyped.with_type::<Clamp<T>>() };
            if !stored.ready || stored.min_limit != limits.0 || stored.max_limit != limits.1 {
                stored.min_limit = limits.0;
                stored.max_limit = limits.1;
                stored.ready = true;
            }
            clamp = Some(stored.clone());
        }
    } else if let Some(stored) = entity.get_by_id(ids.clamp) {
        // a clamp with no limit expressions is the limits it was written with, actor or not
        clamp = Some(unsafe { stored.deref::<Clamp<T>>().clone() });
    }

    let Some(untyped) = entity.get_mut_by_id(ids.component) else {
        if bounded {
            // the limits above were evaluated for nothing and will be again every pass
            warn_once!(
                "{} not found on {}: that clamp bounds nothing and re-evaluates its limits every \
                 pass, build the actor with .with::<{}>()",
                pretty_type_name::<T>(),
                entity.id(),
                pretty_type_name::<T>(),
            );
        }
        return None;
    };
    let mut attribute = unsafe { untyped.with_type::<T>() };
    let calc = folded.calc::<T::Property>(kind, id).unwrap_or_default();
    let mut base = attribute.base_value();
    if let Some(clamp) = &clamp {
        let clamped = clamp.apply(base);
        if clamped != base {
            attribute.set_base_value(clamped);
            base = clamped;
        }
    }
    let mut new = calc.eval(base);
    if let Some(clamp) = &clamp {
        new = clamp.apply(new);
    }
    let old = attribute.current_value();
    let moved = if old == new {
        None
    } else {
        attribute.set_current_value(new);
        // a value nobody observes has no old/new pair to hand to anyone
        Some(Change {
            kind,
            pair: changes.map_or(0, |changes| changes.push_change(kind, old, new)),
        })
    };
    if attribute.folded_base() != Some(base) {
        attribute.set_folded_base(base);
    }
    moved
}

fn dependents_of<T: Attribute>(
    entity: &FilteredEntityRef,
    ids: AttrComponents,
    out: &mut Vec<Entity>,
) {
    if let Some(ptr) = entity.get_by_id(ids.dependents) {
        let dependents = unsafe { ptr.deref::<AttributeDependents<T>>() };
        out.extend(dependents.iter());
    }
}

fn notify<T: Attribute>(
    commands: &mut Commands,
    entity: Entity,
    change: Change,
    values: &ValueBuf,
) {
    let (old, new) = values.pair::<T::Property>(change.kind, change.pair);
    commands.trigger(CurrentValueChanged::<T> {
        entity,
        phantom_data: Default::default(),
        old,
        new,
    });
}

/// Registering an observer registers the event key, so an unwatched attribute has no key to look up.
fn has_observers<T: Attribute>(world: &World) -> bool {
    let Some(key) = world.event_key::<CurrentValueChanged<T>>() else {
        return false;
    };
    let Some(observers) = world.observers().try_get_observers(key) else {
        return false;
    };
    !observers.global_observers().is_empty()
        || !observers.entity_observers().is_empty()
        || observers.component_observers().values().any(|scoped| {
            !scoped.global_observers().is_empty() || !scoped.entity_component_observers().is_empty()
        })
}

fn reset_calcs<P: AttributeValue>(values: &mut ValueBuf, kind: Kind, ids: AttrSet) {
    let rows = values.calcs_slot::<P>(kind);
    for id in ids.iter() {
        reserve(rows, id);
        rows[id.index()] = Calc::default();
    }
}

fn write_calcs<P: AttributeValue>(
    table: &mut ModifierTable,
    kind: Kind,
    ids: AttrSet,
    scratch: &ValueBuf,
) {
    let Some(from) = scratch.calcs_of::<P>(kind) else {
        return;
    };
    let rows = table.values.calcs_slot::<P>(kind);
    for id in ids.iter() {
        if let Some(calc) = from.get(id.index()) {
            reserve(rows, id);
            rows[id.index()] = *calc;
        }
    }
}

fn fold_nothing(_: &mut ValueBuf, _: Kind, _: AttrId, _: &World, _: Entity, _: u32) {}

fn append_pairs_of<P: AttributeValue>(to: &mut ValueBuf, from: &mut ValueBuf, kind: Kind) -> u32 {
    to.append_pairs::<P>(from, kind)
}

#[derive(Resource, Default)]
pub struct AttributeRegistry {
    infos: Vec<AttrInfo>,
    by_type: HashMap<TypeId, AttrId>,
    by_property: HashMap<TypeId, Kind>,
    kinds: Vec<KindInfo>,
    edges: HashSet<(AttrId, AttrId)>,
    /// Attributes in dependency order; an edge `a -> b` puts `a` first.
    order: Vec<AttrId>,
    readers: Vec<Vec<AttrId>>,
    cyclic: bool,
    /// bumped by every mutation, the cache key for copies of the lists below
    revision: u64,
}

impl AttributeRegistry {
    pub fn register<T: Attribute>(&mut self, world: &mut World) -> AttrId {
        if let Some(id) = self.by_type.get(&TypeId::of::<T>()) {
            return *id;
        }
        // past the cap there is no index to hand out, so refuse by name before any AttrId exists
        assert!(
            self.infos.len() < AttrSet::CAPACITY,
            "cannot register {}: the attribute registry holds at most {} attributes and that cap \
             does not grow; merge two of the attributes you register, or drop one",
            pretty_type_name::<T>(),
            AttrSet::CAPACITY
        );
        let kind = self.kind_of::<T::Property>();
        let id = AttrId(self.infos.len() as u16);
        self.infos.push(AttrInfo {
            id,
            name: pretty_type_name::<T>(),
            type_id: TypeId::of::<T>(),
            kind,
            ids: AttrComponents {
                component: world.register_component::<T>(),
                dependents: world.register_component::<AttributeDependents<T>>(),
                clamp: world.register_component::<Clamp<T>>(),
                clamp_bounds: world.register_component::<ClampBounds<T>>(),
            },
            resolve: resolve::<T>,
            dependents_of: dependents_of::<T>,
            notify: notify::<T>,
            has_observers: has_observers::<T>,
            base_write_scan: base_write_scan::<T>,
            fold_modifier: fold_nothing,
        });
        self.by_type.insert(TypeId::of::<T>(), id);
        self.kinds[kind].members.insert(id);
        self.readers.push(Vec::new());
        self.rebuild_order();
        self.revision += 1;
        id
    }

    pub fn len(&self) -> usize {
        self.infos.len()
    }

    /// Moves on any mutation; `len` misses an edge or a rewritten fold.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn is_empty(&self) -> bool {
        self.infos.is_empty()
    }

    pub fn id_of(&self, type_id: TypeId) -> Option<AttrId> {
        self.by_type.get(&type_id).copied()
    }

    pub fn info(&self, id: AttrId) -> &AttrInfo {
        &self.infos[id.index()]
    }

    pub fn infos(&self) -> &[AttrInfo] {
        &self.infos
    }

    pub fn order(&self) -> &[AttrId] {
        &self.order
    }

    pub fn readers(&self, id: AttrId) -> &[AttrId] {
        &self.readers[id.index()]
    }

    pub fn kinds(&self) -> &[KindInfo] {
        &self.kinds
    }

    pub fn kind_of_attr(&self, id: AttrId) -> Kind {
        self.infos[id.index()].kind
    }

    pub fn set_fold_modifier<T: Attribute>(&mut self, fold: FoldModifier) {
        let attribute = TypeId::of::<T>();
        if let Some(info) = self.infos.iter_mut().find(|info| info.type_id == attribute) {
            info.fold_modifier = fold;
            self.revision += 1;
        }
    }

    fn kind_of<P: AttributeValue>(&mut self) -> Kind {
        let property = TypeId::of::<P>();
        if let Some(kind) = self.by_property.get(&property) {
            return *kind;
        }
        let kind = self.kinds.len();
        self.kinds.push(KindInfo {
            append_pairs: append_pairs_of::<P>,
            members: AttrSet::default(),
            reset_calcs: reset_calcs::<P>,
            write_calcs: write_calcs::<P>,
        });
        self.by_property.insert(property, kind);
        kind
    }

    /// `reader` is computed from `read` on the same actor.
    ///
    /// A cycle here panics, so guard with `closes_cycle` first, as `add_clamp_edges` does.
    pub fn add_edge(&mut self, read: AttrId, reader: AttrId) -> bool {
        if read == reader || !self.edges.insert((read, reader)) {
            return false;
        }
        self.readers[read.index()].push(reader);
        self.rebuild_order();
        self.revision += 1;
        true
    }

    /// Would `read -> reader` close a cycle?
    pub fn closes_cycle(&self, read: AttrId, reader: AttrId) -> bool {
        let mut seen = AttrSet::default();
        seen.insert(reader);
        let mut walking = vec![reader];
        while let Some(next) = walking.pop() {
            for &downstream in &self.readers[next.index()] {
                if downstream == read {
                    return true;
                }
                if seen.insert(downstream) {
                    walking.push(downstream);
                }
            }
        }
        false
    }

    fn rebuild_order(&mut self) {
        let n = self.infos.len();
        let mut indegree = vec![0usize; n];
        for &(_, reader) in &self.edges {
            indegree[reader.index()] += 1;
        }
        // a heap, so the order depends on the edges and not on insertion order
        let mut ready: std::collections::BinaryHeap<AttrId> = (0..n)
            .filter(|&i| indegree[i] == 0)
            .map(|i| AttrId(i as u16))
            .collect();
        let mut order = Vec::with_capacity(n);
        while let Some(next) = ready.pop() {
            order.push(next);
            for &reader in &self.readers[next.index()] {
                indegree[reader.index()] -= 1;
                if indegree[reader.index()] == 0 {
                    ready.push(reader);
                }
            }
        }
        self.cyclic = order.len() != n;
        if self.cyclic {
            let names: Vec<_> = (0..n)
                .filter(|&i| indegree[i] > 0)
                .map(|i| self.infos[i].name.clone())
                .collect();
            panic!(
                "attribute dependency cycle through {names:?}: break it by dropping one of the \
                 reads between them, attributes have to form a DAG"
            );
        }
        self.order = order;
    }
}

/// refused (read, reader) type pairs: `warn_once!` is per call site so the latch is manual, and it keys on types because ids are renumbered per app
static REFUSED_CLAMPS: OnceLock<Mutex<HashSet<(TypeId, TypeId)>>> = OnceLock::new();

/// Limits resolve first; unknown or cyclic edges drop and attributes walked before a new edge come back.
pub fn add_clamp_edges(
    world: &mut World,
    reads: impl Iterator<Item = (Entity, TypeId, TypeId)>,
) -> Vec<(Entity, AttrId)> {
    let mut reads = reads.peekable();
    if reads.peek().is_none() {
        return Vec::new();
    }
    let bindings = world.resource::<crate::AppAttributeBindings>().clone();
    let bindings = bindings
        .internal
        .read()
        .unwrap_or_else(|err| err.into_inner());
    let mut registry = world.resource_mut::<AttributeRegistry>();
    let mut fresh: Vec<(AttrId, AttrId)> = Vec::new();
    let mut connected = Vec::new();
    for (actor, key, clamped) in reads {
        let Some(dependency) = bindings.dependency(&key) else {
            continue;
        };
        let Some(read) = registry.id_of(dependency.attribute) else {
            continue;
        };
        let Some(reader) = registry.id_of(clamped) else {
            continue;
        };
        if registry.closes_cycle(read, reader) {
            let first_refusal = REFUSED_CLAMPS
                .get_or_init(Mutex::default)
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .insert((dependency.attribute, clamped));
            if first_refusal {
                let (read_name, reader_name) = (
                    registry.info(read).name.clone(),
                    registry.info(reader).name.clone(),
                );
                warn!(
                    "a clamp on {reader_name} reads {read_name}, which closes an attribute dependency \
                     cycle: the edge is dropped, so {reader_name} keeps the clamping it had before, \
                     without {read_name} resolved ahead of it"
                );
            }
            continue;
        }
        if registry.add_edge(read, reader) {
            fresh.push((read, reader));
        }
        if fresh.contains(&(read, reader)) {
            connected.push((actor, reader));
        }
    }
    connected
}

/// Bitset over [`AttrId`]s.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AttrSet([u64; 2]);

impl AttrSet {
    pub const CAPACITY: usize = 128;

    pub fn insert(&mut self, id: AttrId) -> bool {
        let (word, bit) = (id.index() / 64, id.index() % 64);
        let was = self.0[word] & (1 << bit) != 0;
        self.0[word] |= 1 << bit;
        !was
    }

    /// lowest first
    pub fn iter(&self) -> impl Iterator<Item = AttrId> + '_ {
        (0..self.0.len()).flat_map(move |word| {
            let mut bits = self.0[word];
            std::iter::from_fn(move || {
                (bits != 0).then(|| {
                    let bit = bits.trailing_zeros();
                    bits &= !(1 << bit);
                    AttrId((word * 64 + bit as usize) as u16)
                })
            })
        })
    }

    pub fn intersection(&self, other: &AttrSet) -> AttrSet {
        AttrSet([self.0[0] & other.0[0], self.0[1] & other.0[1]])
    }

    pub fn contains(&self, id: AttrId) -> bool {
        let (word, bit) = (id.index() / 64, id.index() % 64);
        self.0[word] & (1 << bit) != 0
    }

    pub fn union(&mut self, other: &AttrSet) {
        self.0[0] |= other.0[0];
        self.0[1] |= other.0[1];
    }

    pub fn is_empty(&self) -> bool {
        self.0 == [0, 0]
    }

    pub fn clear(&mut self) {
        self.0 = [0, 0];
    }

    pub fn all(len: usize) -> Self {
        let mut set = Self::default();
        for i in 0..len.min(Self::CAPACITY) {
            set.insert(AttrId(i as u16));
        }
        set
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Mark {
    pub dirty: AttrSet,
    /// Attributes whose folded modifiers must be gathered again first.
    pub regather: AttrSet,
}

/// The tuple [`ResolveQueue::take`] returns; spelled out it sits over clippy's complexity limit.
type PendingDrain = (Vec<(Entity, Mark)>, Vec<(Entity, AttrId)>);

/// Work for the next resolve. Observers and systems push here; the resolver drains it once a tick.
#[derive(Resource, Default)]
pub struct ResolveQueue {
    pending: Mutex<Vec<(Entity, Mark)>>,
    dirty: Mutex<Vec<(Entity, AttrId)>>,
}

impl ResolveQueue {
    pub fn push(&self, entity: Entity, mark: Mark) {
        self.pending
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .push((entity, mark));
    }

    pub fn dirty(&self, entity: Entity, attr: AttrId) {
        self.dirty
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .push((entity, attr));
    }

    fn dirty_batch(&self, batch: &mut Vec<(Entity, AttrId)>) {
        if batch.is_empty() {
            return;
        }
        self.dirty
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .append(batch);
    }

    pub fn everything(&self, entity: Entity, attributes: usize) {
        let all = AttrSet::all(attributes);
        self.push(
            entity,
            Mark {
                dirty: all,
                regather: all,
            },
        );
    }

    /// Drains both queues: marks, then plain dirty pairs.
    pub fn take(&self) -> PendingDrain {
        (
            std::mem::take(&mut *self.pending.lock().unwrap_or_else(|err| err.into_inner())),
            std::mem::take(&mut *self.dirty.lock().unwrap_or_else(|err| err.into_inner())),
        )
    }
}

#[derive(Component, Default, Debug)]
pub struct ModifierTable {
    pub values: ValueBuf,
}

type BaseScanQuery<T> = QueryState<(Entity, &'static T), Changed<T>>;

/// kept as a resource so the collapsed scan pays nothing per attribute beyond building its query once
#[derive(Resource)]
struct BaseScanState<T: Attribute> {
    carriers: BaseScanQuery<T>,
    component: ComponentId,
}

/// a summary tick older than `last_run` means no row of that column changed, so the whole scan is skipped
fn anything_written(world: &World, component: ComponentId, last_run: Tick, this_run: Tick) -> bool {
    world.storages().tables.iter().any(|table| {
        table
            .get_summary_tick(component)
            .is_some_and(|tick| tick.get().is_newer_than(last_run, this_run))
    })
}

/// marks `T` dirty where its base moved since the last scan, actor or not; an unchanged base costs a compare
fn base_write_scan<T: Attribute>(world: &mut World, id: AttrId, out: &mut Vec<(Entity, AttrId)>) {
    if !world.contains_resource::<BaseScanState<T>>() {
        let carriers = world.query_filtered();
        let component = world.register_component::<T>();
        world.insert_resource(BaseScanState::<T> {
            carriers,
            component,
        });
    }
    if <T as Component>::HAS_SUMMARY_TICK {
        // the same pair of ticks the Changed filter is about to compare against
        let (last_run, this_run) = (world.last_change_tick(), world.change_tick());
        let component = world.resource::<BaseScanState<T>>().component;
        if !anything_written(world, component, last_run, this_run) {
            return;
        }
    }
    world.resource_scope(|world, mut state: Mut<BaseScanState<T>>| {
        for (entity, attribute) in state.carriers.iter(world) {
            if attribute.folded_base() != Some(attribute.base_value()) {
                out.push((entity, id));
            }
        }
    });
}

/// one node for every attribute, so the base-write scan adds no schedule node of its own per attribute
pub fn scan_base_writes(
    world: &mut World,
    mut batch: Local<Vec<(Entity, AttrId)>>,
    mut scans: Local<Vec<(AttrId, BaseWriteScan)>>,
) {
    scans.clear();
    scans.extend(
        world
            .resource::<AttributeRegistry>()
            .infos()
            .iter()
            .map(|info| (info.id, info.base_write_scan)),
    );
    for &(id, scan) in scans.iter() {
        scan(world, id, &mut batch);
    }
    world.resource::<ResolveQueue>().dirty_batch(&mut batch);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attributes::Attribute;
    use crate::modifier::ModOp;
    use crate::prelude::{ReflectAccessAttribute, ReflectComponent};
    use bevy_ecs::prelude::World;

    fn register_one<T: Attribute>(registry: &mut AttributeRegistry, world: &mut World) {
        registry.register::<T>(world);
    }

    macro_rules! cap_pool {
        ($( $name:ident ),+ $(,)?) => {
            $( crate::attribute!($name); )+

            /// One entry per type, so the test loops instead of spelling out 129 calls.
            static REGISTRATION: &[fn(&mut AttributeRegistry, &mut World)] =
                &[$(register_one::<$name>),+];
        };
    }

    cap_pool!(
        Attr0, Attr1, Attr2, Attr3, Attr4, Attr5, Attr6, Attr7, Attr8, Attr9, Attr10, Attr11,
        Attr12, Attr13, Attr14, Attr15, Attr16, Attr17, Attr18, Attr19, Attr20, Attr21, Attr22,
        Attr23, Attr24, Attr25, Attr26, Attr27, Attr28, Attr29, Attr30, Attr31, Attr32, Attr33,
        Attr34, Attr35, Attr36, Attr37, Attr38, Attr39, Attr40, Attr41, Attr42, Attr43, Attr44,
        Attr45, Attr46, Attr47, Attr48, Attr49, Attr50, Attr51, Attr52, Attr53, Attr54, Attr55,
        Attr56, Attr57, Attr58, Attr59, Attr60, Attr61, Attr62, Attr63, Attr64, Attr65, Attr66,
        Attr67, Attr68, Attr69, Attr70, Attr71, Attr72, Attr73, Attr74, Attr75, Attr76, Attr77,
        Attr78, Attr79, Attr80, Attr81, Attr82, Attr83, Attr84, Attr85, Attr86, Attr87, Attr88,
        Attr89, Attr90, Attr91, Attr92, Attr93, Attr94, Attr95, Attr96, Attr97, Attr98, Attr99,
        Attr100, Attr101, Attr102, Attr103, Attr104, Attr105, Attr106, Attr107, Attr108, Attr109,
        Attr110, Attr111, Attr112, Attr113, Attr114, Attr115, Attr116, Attr117, Attr118, Attr119,
        Attr120, Attr121, Attr122, Attr123, Attr124, Attr125, Attr126, Attr127, Attr128
    );

    /// names 129; `should_panic` matches text: in the cranelift dev build a panic escapes `catch_unwind`
    #[test]
    #[should_panic(
        expected = "register Attr128: the attribute registry holds at most 128 attributes"
    )]
    fn attribute_129_panics_naming_itself_and_the_cap() {
        assert_eq!(
            REGISTRATION.len(),
            AttrSet::CAPACITY + 1,
            "one type past the cap"
        );
        let mut world = World::new();
        let mut registry = AttributeRegistry::default();
        for register in &REGISTRATION[..AttrSet::CAPACITY] {
            register(&mut registry, &mut world);
        }
        assert_eq!(
            registry.len(),
            AttrSet::CAPACITY,
            "an app at the cap still registers"
        );
        for register in &REGISTRATION[AttrSet::CAPACITY..] {
            register(&mut registry, &mut world);
        }
    }

    crate::attribute!(Attack);
    crate::attribute!(Power);
    crate::attribute!(Haste);
    crate::attribute!(Armor);

    /// A cycle is a startup error, and the message names the attributes in it, not the downstream ones.
    #[test]
    #[should_panic(
        expected = r#"cycle through ["Attack", "Power"]: break it by dropping one of the reads"#
    )]
    fn a_cycle_panics_naming_the_attributes_in_it() {
        let mut world = World::new();
        let mut registry = AttributeRegistry::default();
        let attack = registry.register::<Attack>(&mut world);
        let power = registry.register::<Power>(&mut world);
        let haste = registry.register::<Haste>(&mut world);
        let armor = registry.register::<Armor>(&mut world);
        registry.add_edge(haste, armor);
        // add_edge takes (read, reader), so these two are Attack and Power reading each other.
        registry.add_edge(attack, power);
        registry.add_edge(power, attack);
    }

    crate::attribute!(Frost, i32);
    crate::attribute!(Ember, f64);

    #[test]
    fn one_kind_per_property_type() {
        let mut world = World::new();
        let mut registry = AttributeRegistry::default();
        let attack = registry.register::<Attack>(&mut world);
        let power = registry.register::<Power>(&mut world);
        let frost = registry.register::<Frost>(&mut world);
        let ember = registry.register::<Ember>(&mut world);

        assert_eq!(registry.kinds().len(), 3);
        assert_eq!(registry.kind_of_attr(attack), registry.kind_of_attr(power));
        assert_ne!(registry.kind_of_attr(frost), registry.kind_of_attr(ember));

        let mut values = ValueBuf::default();
        let kind = registry.kind_of_attr(attack);
        *values.calc_mut(kind, attack) = Calc::from_op(ModOp::Add, 3.0f32);
        *values.calc_mut(kind, power) = Calc::from_op(ModOp::Mul, 2.0f32);
        assert_eq!(values.calc::<f32>(kind, attack).unwrap().eval(10.0), 13.0);
        assert_eq!(values.calc::<f32>(kind, power).unwrap().eval(10.0), 20.0);

        values.push_change(kind, 0.0f32, 0.5f32);
        let mut thread = ValueBuf::default();
        let first = thread.push_change(kind, 1.0f32, 2.0f32);
        let second = thread.push_change(kind, 3.0f32, 4.0f32);
        let offset = values.append_pairs::<f32>(&mut thread, kind);
        assert_eq!(offset, 1);
        assert_eq!(values.pair::<f32>(kind, first + offset), (1.0, 2.0));
        assert_eq!(values.pair::<f32>(kind, second + offset), (3.0, 4.0));
    }
}
