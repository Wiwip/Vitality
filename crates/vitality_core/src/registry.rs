//! Erased per-attribute operations and the type-level dependency DAG the resolver walks.

use crate::CurrentValueChanged;
use crate::attributes::{Attribute, AttributeDependents, Clamp, ClampBounds};
use crate::context::ActorExprContext;
use crate::modifier::{AttributeCalculator, ModOp};
use crate::util::pretty_type_name;
use bevy::ecs::component::ComponentId;
use bevy::ecs::world::{FilteredEntityMut, FilteredEntityRef};
use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::*;
use num_traits::{AsPrimitive, FromPrimitive};
use std::any::TypeId;
use std::sync::Mutex;

/// Dense index of a registered attribute.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Reflect)]
pub struct AttrId(pub u16);

impl AttrId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// An [`AttributeCalculator`] with every value widened to f64, so one table can hold any attribute.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ErasedCalc {
    pub set: Option<f64>,
    pub add: f64,
    pub sub: f64,
    pub increase: f64,
    pub mul: f64,
}

impl Default for ErasedCalc {
    fn default() -> Self {
        Self {
            set: None,
            add: 0.0,
            sub: 0.0,
            increase: 0.0,
            mul: 1.0,
        }
    }
}

impl ErasedCalc {
    /// Same stacking rules as [`AttributeCalculator::from_op_stacked`].
    pub fn from_op(op: ModOp, value: f64, stacks: u32) -> Self {
        let stacks = stacks.max(1) as f64;
        let mut calc = Self::default();
        match op {
            ModOp::Set => calc.set = Some(value),
            ModOp::Add => calc.add = value * stacks,
            ModOp::Sub => calc.sub = value * stacks,
            ModOp::Increase => calc.increase = value * stacks,
            ModOp::Mul => calc.mul = value.powf(stacks),
        }
        calc
    }

    pub fn combine(&mut self, other: &ErasedCalc) {
        self.set = self.set.or(other.set);
        self.add += other.add;
        self.sub += other.sub;
        self.increase += other.increase;
        self.mul *= other.mul;
    }

    fn typed<T: Attribute>(&self) -> AttributeCalculator<T> {
        let narrow = |value: f64| {
            T::Property::from_f64(value).unwrap_or_else(|| {
                if value.is_sign_negative() {
                    <T::Property as num_traits::Bounded>::min_value()
                } else {
                    <T::Property as num_traits::Bounded>::max_value()
                }
            })
        };
        AttributeCalculator::from_parts(
            self.set.map(narrow),
            narrow(self.add),
            narrow(self.sub),
            self.increase,
            self.mul,
        )
    }
}

/// What the resolver needs to do to one attribute without knowing its type.
pub struct AttrInfo {
    pub id: AttrId,
    pub name: String,
    pub type_id: TypeId,
    pub ids: AttrComponents,
    /// Recomputes current from base, the folded modifiers and the clamp, writing only on change.
    /// None when the actor doesn't have the attribute.
    pub resolve: fn(&mut FilteredEntityMut, AttrComponents, &ErasedCalc) -> Option<Resolved>,
    /// Modifier entities reading this attribute on the actor.
    pub dependents_of: fn(&FilteredEntityRef, AttrComponents, &mut Vec<Entity>),
    pub notify: fn(&mut Commands, Entity, f64, f64),
    /// Actors of this attribute whose base moved since the last scan, for [`scan_base_writes`].
    pub base_write_scan: fn(&mut World, AttrId, &mut Vec<(Entity, AttrId)>),
}

/// Component ids of an attribute and the components that ride along with it, resolved once.
#[derive(Clone, Copy, Debug)]
pub struct AttrComponents {
    pub component: ComponentId,
    pub dependents: ComponentId,
    pub clamp: ComponentId,
    pub clamp_bounds: ComponentId,
}

#[derive(Clone, Copy, Debug)]
pub struct Resolved {
    pub base: f64,
    pub moved: Option<(f64, f64)>,
}

// SAFETY (all three below): every id in `ids` was registered for exactly the type it is read as.
fn resolve<T: Attribute>(
    entity: &mut FilteredEntityMut,
    ids: AttrComponents,
    calc: &ErasedCalc,
) -> Option<Resolved> {
    let mut clamp: Option<Clamp<T>> = None;
    if entity.get_by_id(ids.clamp_bounds).is_some() {
        let limits = {
            let view = entity.as_readonly();
            let bounds = unsafe { view.get_by_id(ids.clamp_bounds)?.deref::<ClampBounds<T>>() };
            let context = ActorExprContext {
                actor_context: view.clone(),
            };
            (bounds.min.eval(&context), bounds.max.eval(&context))
        };
        if let Some(untyped) = entity.get_mut_by_id(ids.clamp) {
            let mut stored = unsafe { untyped.with_type::<Clamp<T>>() };
            if !stored.ready || stored.min_limit != limits.0 || stored.max_limit != limits.1 {
                stored.min_limit = limits.0;
                stored.max_limit = limits.1;
                stored.ready = true;
            }
            clamp = Some(stored.clone());
        }
    }

    let untyped = entity.get_mut_by_id(ids.component)?;
    let mut attribute = unsafe { untyped.with_type::<T>() };
    let mut base = attribute.base_value();
    if let Some(clamp) = &clamp {
        let clamped = clamp.apply(base);
        if clamped != base {
            attribute.set_base_value(clamped);
            base = clamped;
        }
    }
    let mut new = calc.typed::<T>().eval(base);
    if let Some(clamp) = &clamp {
        new = clamp.apply(new);
    }
    let old = attribute.current_value();
    let moved = if old == new {
        None
    } else {
        attribute.set_current_value(new);
        Some((old.as_(), new.as_()))
    };
    Some(Resolved {
        base: base.as_(),
        moved,
    })
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

fn notify<T: Attribute>(commands: &mut Commands, entity: Entity, old: f64, new: f64) {
    let narrow = |value: f64| T::Property::from_f64(value).unwrap_or_default();
    commands.trigger(CurrentValueChanged::<T> {
        entity,
        phantom_data: Default::default(),
        old: narrow(old),
        new: narrow(new),
    });
}

#[derive(Resource, Default)]
pub struct AttributeRegistry {
    infos: Vec<AttrInfo>,
    by_type: HashMap<TypeId, AttrId>,
    edges: HashSet<(AttrId, AttrId)>,
    /// Attributes in dependency order; an edge `a -> b` puts `a` first.
    order: Vec<AttrId>,
    /// Per attribute, the attributes of the same actor that read it.
    readers: Vec<Vec<AttrId>>,
    cyclic: bool,
}

impl AttributeRegistry {
    pub fn register<T: Attribute>(&mut self, world: &mut World) -> AttrId {
        if let Some(id) = self.by_type.get(&TypeId::of::<T>()) {
            return *id;
        }
        assert!(
            self.infos.len() < AttrSet::CAPACITY,
            "vitality supports at most {} attributes",
            AttrSet::CAPACITY
        );
        let id = AttrId(self.infos.len() as u16);
        self.infos.push(AttrInfo {
            id,
            name: pretty_type_name::<T>(),
            type_id: TypeId::of::<T>(),
            ids: AttrComponents {
                component: world.register_component::<T>(),
                dependents: world.register_component::<AttributeDependents<T>>(),
                clamp: world.register_component::<Clamp<T>>(),
                clamp_bounds: world.register_component::<ClampBounds<T>>(),
            },
            resolve: resolve::<T>,
            dependents_of: dependents_of::<T>,
            notify: notify::<T>,
            base_write_scan: base_write_scan::<T>,
        });
        self.by_type.insert(TypeId::of::<T>(), id);
        self.readers.push(Vec::new());
        self.rebuild_order();
        id
    }

    pub fn len(&self) -> usize {
        self.infos.len()
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

    pub fn is_cyclic(&self) -> bool {
        self.cyclic
    }

    /// `reader` is computed from `read` on the same actor. Returns whether the edge is new.
    pub fn add_edge(&mut self, read: AttrId, reader: AttrId) -> bool {
        if read == reader || !self.edges.insert((read, reader)) {
            return false;
        }
        self.readers[read.index()].push(reader);
        self.rebuild_order();
        true
    }

    fn rebuild_order(&mut self) {
        let n = self.infos.len();
        let mut indegree = vec![0usize; n];
        for &(_, reader) in &self.edges {
            indegree[reader.index()] += 1;
        }
        let mut ready: Vec<AttrId> = (0..n)
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
            warn!("attribute dependency cycle through {names:?}; extra passes will settle it");
            for i in 0..n {
                if indegree[i] > 0 {
                    order.push(AttrId(i as u16));
                }
            }
        }
        self.order = order;
    }
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

/// What a mark asks of the resolver for one actor.
#[derive(Clone, Copy, Debug, Default)]
pub struct Mark {
    /// Attributes whose current value must be recomputed.
    pub dirty: AttrSet,
    /// Attributes whose folded modifiers must be gathered again first.
    pub regather: AttrSet,
}

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

    pub fn dirty_batch(&self, batch: &mut Vec<(Entity, AttrId)>) {
        if batch.is_empty() {
            return;
        }
        self.dirty
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .append(batch);
    }

    /// Everything about this actor's modifiers changed: regather and recompute every attribute.
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
    pub fn take(&self) -> (Vec<(Entity, Mark)>, Vec<(Entity, AttrId)>) {
        (
            std::mem::take(&mut *self.pending.lock().unwrap_or_else(|err| err.into_inner())),
            std::mem::take(&mut *self.dirty.lock().unwrap_or_else(|err| err.into_inner())),
        )
    }
}

/// Per-actor resolver state: the folded modifiers and the base each attribute was last resolved at.
#[derive(Component, Default, Debug, Clone)]
pub struct ModifierTable {
    pub calcs: Vec<ErasedCalc>,
    pub bases: Vec<Option<f64>>,
}

impl ModifierTable {
    pub fn calc(&self, id: AttrId) -> ErasedCalc {
        self.calcs.get(id.index()).copied().unwrap_or_default()
    }

    pub fn set_calc(&mut self, id: AttrId, calc: ErasedCalc) {
        if self.calcs.len() <= id.index() {
            self.calcs.resize(id.index() + 1, ErasedCalc::default());
        }
        self.calcs[id.index()] = calc;
    }

    pub fn base(&self, id: AttrId) -> Option<f64> {
        self.bases.get(id.index()).copied().flatten()
    }

    pub fn set_base(&mut self, id: AttrId, base: f64) {
        if self.bases.len() <= id.index() {
            self.bases.resize(id.index() + 1, None);
        }
        self.bases[id.index()] = Some(base);
    }
}

/// The scan query of one attribute, kept as a resource so the collapsed scan pays nothing per
/// attribute beyond building it once.
#[derive(Resource)]
struct BaseScanState<T: Attribute> {
    actors: QueryState<
        (Entity, &'static T, Option<&'static ModifierTable>),
        (Changed<T>, With<crate::attributes::ManageAttributes>),
    >,
}

/// Marks `T` dirty on actors whose base moved since the last scan. A write that leaves the
/// base where it was, including the resolver's own current-value write, costs a compare.
fn base_write_scan<T: Attribute>(world: &mut World, id: AttrId, out: &mut Vec<(Entity, AttrId)>) {
    if !world.contains_resource::<BaseScanState<T>>() {
        let actors = world.query_filtered();
        world.insert_resource(BaseScanState::<T> { actors });
    }
    world.resource_scope(|world, mut state: Mut<BaseScanState<T>>| {
        for (entity, attribute, table) in state.actors.iter(world) {
            let base: f64 = attribute.base_value().as_();
            if table.and_then(|table| table.base(id)) != Some(base) {
                out.push((entity, id));
            }
        }
    });
}

/// One node for every attribute: asks each registered attribute for the actors whose base moved,
/// so the base-write scan does not add a schedule node per attribute.
pub fn scan_base_writes(world: &mut World, mut batch: Local<Vec<(Entity, AttrId)>>) {
    let scans: Vec<_> = world
        .resource::<AttributeRegistry>()
        .infos()
        .iter()
        .map(|info| (info.id, info.base_write_scan))
        .collect();
    for (id, scan) in scans {
        scan(world, id, &mut batch);
    }
    world.resource::<ResolveQueue>().dirty_batch(&mut batch);
}
