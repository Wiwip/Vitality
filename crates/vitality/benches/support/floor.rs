#![allow(dead_code)]

//! Family A (`floor/idle_tick`, BENCH-PLAN §2A): what one registered attribute costs a tick when
//! there is nothing to do. Kept free of criterion so `tests/test_bench_harness.rs` can include it
//! and prove the shape idle.
//!
//! A registered attribute is not only a component: `init_attribute` puts `apply_periodic_effect`,
//! `apply_modifier_events`, `update_current_value_system`, the two notify systems and `apply_clamps`
//! into `FixedUpdate` for its type. With no dirty actor every one of them still runs and iterates
//! nothing, and that is the scheduling floor T1 asks about — BENCH-PLAN §3 collapses the topology
//! when its slope reaches 2 µs/attr/tick.
//!
//! [`floor_shape`] builds the shape, and it has to clear the dirty markers rather than tick them
//! away. A world from `build_app_with` comes back dirty and unresolved, and ticking it does not
//! settle it into zero-dirty: a resolve takes `AttributeQueryData<T>` (a `&mut T`), so `T` counts as
//! changed for the rest of the tick, and `on_change_notify_attribute_parents::<T>` — which runs in
//! `EffectsSet::Notify`, after the resolve — turns that into a fresh `MarkNodeDirty`. Measured on a
//! 3-attribute, 5-actor, zero-effect world: every tick triggers 5 `MarkNodeDirty` and inserts 5
//! `Dirty<T>` while every value stays put, so it is dirty forever. [`clear_dirty`] removes the
//! markers, and a world with none stays at zero because nothing resolves and so nothing is marked.

use crate::common::{
    ATTRIBUTE_POOL, AttrSpec, POOL_LEN, WorldSpec, actor_entities, build_app_with, run_fixed_tick,
    tick,
};
use bevy::ecs::component::ComponentId;
use bevy::prelude::*;
use vitality::prelude::*;

/// Actors every floor shape spawns. An idle tick touches no actor at all, so this only has to be big
/// enough that the tick is not measuring an empty world.
pub const FLOOR_ACTORS: usize = 1000;

/// Attribute counts the group times, so the guard test and the bench cannot drift apart on the
/// x-axis. BENCH-PLAN §2A lists `{1,5,10,20,40,80}`; the sprint item takes five points and drops 5,
/// which draws from the same pool as 10 and adds nothing the slope needs.
pub const FLOOR_COUNTS: [usize; 5] = [1, 10, 20, 40, 80];

macro_rules! floor_attr_pool {
    ($( $name:ident ),+ $(,)?) => {
        $( ::vitality::attribute!($name); )+

        /// Past the 24 attributes `common::ATTRIBUTE_POOL` holds, for the 40 and 80 points. The 40
        /// point registers its prefix: `build_app_with` slices the pool to the shape, so it pays for
        /// 40 registrations and 40 component sets, not 80, and `HarnessWorld::attrs().len()` is what
        /// a guard reads to make sure of that.
        pub static FLOOR_POOL: &[AttrSpec] = &[$(AttrSpec::new::<$name>(stringify!($name))),+];
    };
}

floor_attr_pool!(
    Flr0, Flr1, Flr2, Flr3, Flr4, Flr5, Flr6, Flr7, Flr8, Flr9, Flr10, Flr11, Flr12, Flr13, Flr14,
    Flr15, Flr16, Flr17, Flr18, Flr19, Flr20, Flr21, Flr22, Flr23, Flr24, Flr25, Flr26, Flr27,
    Flr28, Flr29, Flr30, Flr31, Flr32, Flr33, Flr34, Flr35, Flr36, Flr37, Flr38, Flr39, Flr40,
    Flr41, Flr42, Flr43, Flr44, Flr45, Flr46, Flr47, Flr48, Flr49, Flr50, Flr51, Flr52, Flr53,
    Flr54, Flr55, Flr56, Flr57, Flr58, Flr59, Flr60, Flr61, Flr62, Flr63, Flr64, Flr65, Flr66,
    Flr67, Flr68, Flr69, Flr70, Flr71, Flr72, Flr73, Flr74, Flr75, Flr76, Flr77, Flr78, Flr79,
);

/// The deepest shape [`floor_pool`] can serve.
pub const FLOOR_POOL_LEN: usize = FLOOR_POOL.len();

/// The pool a floor point registers. Up to [`POOL_LEN`] the points use the harness's default pool —
/// same types as every other family, so their ns/tick is comparable with the leaf numbers — and above
/// it they use [`FLOOR_POOL`], whose types belong to this family alone.
pub fn floor_pool(attributes: usize) -> &'static [AttrSpec] {
    assert!(
        attributes <= FLOOR_POOL_LEN,
        "no floor pool {} attributes deep: FLOOR_POOL holds {FLOOR_POOL_LEN}, the default pool {POOL_LEN}",
        attributes
    );
    if attributes <= POOL_LEN {
        ATTRIBUTE_POOL
    } else {
        &FLOOR_POOL[..attributes]
    }
}

/// The shape a `floor/idle_tick` point times: a built, settled, empty-of-work app and the actors it
/// spawned.
pub struct FloorShape {
    pub app: App,
    pub actors: Vec<Entity>,
    /// Dirty entities in the unprepared world. Kept so a guard can show the walk in
    /// [`dirty_entity_count`] really does see a `Dirty<T>` — a walk that sees none also reports 0 for
    /// a world full of them, and then "no `Dirty<T>` survives" proves nothing.
    pub dirty_at_build: usize,
    /// `Dirty<T>` components [`clear_dirty`] removed.
    pub markers_cleared: usize,
}

/// The idle shape for `attributes` attributes on `actors` actors: no effects, nothing dirty, so the
/// modifiers/dirty/ticking fields of the id are all zero by construction.
pub fn floor_spec(attributes: usize, actors: usize) -> WorldSpec {
    WorldSpec {
        attributes_per_actor: attributes,
        effects_per_actor: 0,
        modifiers_per_effect: 0,
        actors,
        dirty_fraction: 0.0,
        ticking_fraction: 0.0,
    }
}

/// Builds and settles one floor point. `pool` has to be at least `spec.attributes_per_actor` deep;
/// `build_app_with` registers the prefix only, so a deep pool is safe to hand to a shallow shape, and
/// `HarnessWorld::attrs()` is how a guard checks that the shape registered what it declared.
pub fn floor_shape(spec: &WorldSpec, pool: &'static [AttrSpec]) -> FloorShape {
    let mut app = build_app_with(spec, pool);
    let dirty_at_build = dirty_entity_count(app.world());
    let markers_cleared = prepare_floor(&mut app);
    let actors = actor_entities(app.world_mut());
    assert!(
        !actors.is_empty(),
        "shape {} spawned no actor to leave idle",
        spec.shape_id()
    );
    FloorShape {
        app,
        actors,
        dirty_at_build,
        markers_cleared,
    }
}

/// Leaves the app with nothing for a tick to do: one [`tick`] so the modifiers Startup applied are
/// resolved, then [`clear_dirty`], then one [`run_fixed_tick`] — the very call the benches time — to
/// show the zero-dirty shape holds. Returns how many markers were cleared.
pub fn prepare_floor(app: &mut App) -> usize {
    tick(app);
    let cleared = clear_dirty(app.world_mut());
    run_fixed_tick(app.world_mut());
    let left = dirty_entity_count(app.world());
    assert_eq!(
        left, 0,
        "the floor shape re-dirtied itself ({} entities) after {} markers were cleared and an idle \
         tick ran: a timed tick would resolve, so its ns/tick is not a floor",
        left, cleared
    );
    cleared
}

/// Removes every `Dirty<T>` from the world and returns how many components came off. This is the
/// only way the module knows to a genuinely idle tick — see the module docs for why ticking does not
/// get there.
pub fn clear_dirty(world: &mut World) -> usize {
    // Collected first: removing a component moves its entity to another archetype.
    let mut marked: Vec<(Entity, ComponentId)> = Vec::new();
    for archetype in world.archetypes().iter() {
        for id in archetype.components().iter().copied() {
            if !is_dirty_component(world, id) {
                continue;
            }
            marked.extend(archetype.entities().iter().map(|slot| (slot.id(), id)));
        }
    }

    for (entity, id) in &marked {
        world.entity_mut(*entity).remove_by_id(*id);
    }
    marked.len()
}

/// Entities anywhere in this world carrying a `Dirty<T>`, whatever `T`.
///
/// Walked over the archetypes rather than over the harness's pool, because the claim is about the
/// world: archetypes partition the entities, so summing the length of every archetype that stores
/// some `Dirty<T>` counts a dirty entity once however many attributes it carries.
pub fn dirty_entity_count(world: &World) -> usize {
    world
        .archetypes()
        .iter()
        .filter(|archetype| {
            archetype
                .components()
                .iter()
                .any(|id| is_dirty_component(world, *id))
        })
        .map(|archetype| archetype.entities().len())
        .sum()
}

/// The walk sees `Dirty<T>` by name, because a [`WorldSpec`] picks its attributes at run time and no
/// `T` is in scope here. `Dirty` is a last segment, not a substring, so the `MarkNodeDirty<T>` event
/// cannot match it.
fn is_dirty_component(world: &World, id: ComponentId) -> bool {
    // `DebugName` only carries a type name when bevy's `debug` feature is on; dev-dependencies turn
    // it on, and `FloorShape::dirty_at_build` fails loudly if that ever stops being true.
    let Some(name) = world.components().get_name(id) else {
        return false;
    };
    let name = name.to_string();
    let head = name
        .strip_suffix('>')
        .and_then(|head| head.rsplit_once('<'))
        .map_or(name.as_str(), |(head, _tail)| head);
    head.rsplit("::").next() == Some("Dirty")
}
