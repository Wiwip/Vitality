//! Erased per-attribute operations and the type-level dependency DAG the resolver walks.

use crate::CurrentValueChanged;
use crate::attributes::{Attribute, AttributeDependents, Clamp, ClampBounds, record_dependencies};
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

/// The resolve step and the sink it fills with `(actor, limit key, clamped attribute)` triples;
/// spelled out inside the struct it sits over clippy's complexity limit.
type ResolveFn = fn(
    &mut FilteredEntityMut,
    AttrComponents,
    &ErasedCalc,
    &mut Vec<(Entity, TypeId, TypeId)>,
) -> Option<Resolved>;

/// What the resolver needs to do to one attribute without knowing its type.
pub struct AttrInfo {
    pub id: AttrId,
    pub name: String,
    pub type_id: TypeId,
    pub ids: AttrComponents,
    /// recomputes current from base, the folded modifiers and the clamp, writing only on change; None when the actor doesn't have the attribute
    /// the sink collects the limits a first clamp pass read, see [`add_clamp_edges`]
    pub resolve: ResolveFn,
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
    pub moved: Option<(f64, f64)>,
}

// SAFETY (all three below): every id in `ids` was registered for exactly the type it is read as.
fn resolve<T: Attribute>(
    entity: &mut FilteredEntityMut,
    ids: AttrComponents,
    calc: &ErasedCalc,
    clamp_reads: &mut Vec<(Entity, TypeId, TypeId)>,
) -> Option<Resolved> {
    let mut clamp: Option<Clamp<T>> = None;
    if entity.get_by_id(ids.clamp_bounds).is_some() {
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
    if attribute.folded_base() != Some(base) {
        attribute.set_folded_base(base);
    }
    Some(Resolved { moved })
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
    readers: Vec<Vec<AttrId>>,
    cyclic: bool,
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
            panic!(
                "attribute dependency cycle through {names:?}: break it by dropping one of the \
                 reads between them, attributes have to form a DAG"
            );
        }
        self.order = order;
    }
}

/// Clamped attributes read their limits' attributes, so those resolve first. `reads` pairs an actor
/// with a dependency key and the attribute clamped by it; a key no binding knows is dropped. The
/// attributes walked before an edge that only appeared in this call come back, so they resolve again
/// against the order the walk never had.
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
        let Some(read) = bindings
            .dependency(&key)
            .and_then(|binding| registry.id_of(binding.attribute))
        else {
            continue;
        };
        let Some(reader) = registry.id_of(clamped) else {
            continue;
        };
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

#[derive(Component, Default, Debug, Clone)]
pub struct ModifierTable {
    pub calcs: Vec<ErasedCalc>,
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
}

type BaseScanQuery<T> =
    QueryState<(Entity, &'static T), (Changed<T>, With<crate::attributes::ManageAttributes>)>;

/// kept as a resource so the collapsed scan pays nothing per attribute beyond building its query once
#[derive(Resource)]
struct BaseScanState<T: Attribute> {
    actors: BaseScanQuery<T>,
}

/// marks `T` dirty on actors whose base moved since the last scan; a write that leaves the base where it was, including the resolver's own current-value write, costs a compare
fn base_write_scan<T: Attribute>(world: &mut World, id: AttrId, out: &mut Vec<(Entity, AttrId)>) {
    if !world.contains_resource::<BaseScanState<T>>() {
        let actors = world.query_filtered();
        world.insert_resource(BaseScanState::<T> { actors });
    }
    world.resource_scope(|world, mut state: Mut<BaseScanState<T>>| {
        for (entity, attribute) in state.actors.iter(world) {
            if attribute.folded_base() != Some(attribute.base_value()) {
                out.push((entity, id));
            }
        }
    });
}

/// one node for every attribute, so the base-write scan adds no schedule node of its own per attribute
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attributes::Attribute;
    use crate::prelude::{ReflectAccessAttribute, ReflectComponent};
    use bevy::prelude::World;

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

    /// attribute 129 has no index to hand out and the refusal has to name it; a caught panic escapes `catch_unwind` in the cranelift dev build, so `should_panic` matches the message
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
}
