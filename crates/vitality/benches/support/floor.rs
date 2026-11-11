#![allow(dead_code)]

//! Family A (`floor/idle_tick`, BENCH-PLAN §2A): what one registered attribute costs a tick when
//! there is nothing to do. Kept free of criterion so `tests/test_bench_harness.rs` can include it
//! and prove the shape idle.
//!
//! A registered attribute is not only a component: on v1 `init_attribute` puts `apply_periodic_effect`,
//! `apply_modifier_events`, `update_current_value_system`, the two notify systems and
//! `apply_clamps` into `FixedUpdate` for its type
//! (`vitality_effects/src/lib.rs:155-172`, `vitality/src/lib.rs:137-141`). With no dirty actor every
//! one of them still runs and iterates nothing, and that is the scheduling floor T1 asks about —
//! BENCH-PLAN §3 collapses the topology when its slope reaches 2 µs/attr/tick. That slope is a v1
//! quantity: under `VITALITY_RESOLVER=dag` a registered attribute adds no system at all, only an entry
//! in three resources (`vitality_effects/src/lib.rs:142-152`), and the app carries one
//! `resolve_system`, one `apply_all_modifier_events`, one `scan_base_writes` and one
//! `apply_all_periodic_effects` for every attribute together
//! (`vitality_effects/src/lib.rs:95-124`), so the same sweep measures the collapsed scan's per-attribute
//! query cost rather than a schedule cost, and the two floors are only comparable inside one mode.
//!
//! [`floor_shape`] builds the shape, and it clears the dirty markers instead of ticking them away.
//! A world from `build_app_with` comes back dirty and unresolved: the first resolve has a real fold to
//! write (the modifiers `Startup` applied are not in `current_value` yet). Since D-34 that is the end of
//! it: the next resolve yields what the attribute already holds, `are_different`
//! (`crates/vitality_core/src/math.rs`) says "unchanged", and nothing re-marks. Since L-4 one tick is
//! enough to get there, because `on_change_notify_attribute_parents::<T>` now runs in
//! `EffectsSet::UpdateBaseValues`, *ahead of* the resolve — the write that resolve makes is only read as
//! a `Changed<T>` by the next tick, and by then the marker is gone. Measured on this file's
//! 3-attribute, 5-actor, zero-effect world: 5 dirty entities at the build, 0 after tick 1 and from there
//! on; before L-4 the same world read 5 after tick 1 and 0 from tick 2. Before the D-34 fix it triggered
//! 5 `MarkNodeDirty` and re-inserted 5 `Dirty<T>` on every tick while every value stayed put, so it was
//! dirty forever. [`clear_dirty`] is still what the shapes use: it reaches zero in one step and without
//! spending resolve work that a timed tick would then be paying for.

use crate::common::{
    ATTRIBUTE_POOL, AttrSpec, POOL_LEN, WorldSpec, actor_entities, build_app_with, run_fixed_tick,
    tick,
};
use bevy::ecs::component::ComponentId;
use bevy::prelude::*;
use vitality::actors::Actor;
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
    /// `Dirty<T>` components the world carried when [`prepare_floor`] took it over. Since L-4 that is
    /// the same number as [`FloorShape::dirty_at_build`]: the settle tick consumes the marks, so
    /// [`clear_dirty`] has nothing left to remove. Zero means the build dirtied nothing at all.
    pub markers_at_handoff: usize,
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
    let (markers_at_handoff, cleared) = prepare_floor_tallies(&mut app);
    // Nothing to fold on a floor point, so a mark still standing after the settle tick is one the
    // resolve did not consume, and `clear_dirty` is what hides that.
    assert_eq!(
        cleared,
        0,
        "shape {} left {cleared} `Dirty<T>` markers for `clear_dirty` to remove after its settle tick \
         ({markers_at_handoff} dirty entities at handoff): a shape that folds nothing has nothing \
         standing at the end of a tick unless that tick raised the mark after its own resolve, and the \
         pre-L-4 `EffectsSet::Notify` put `on_change_notify_attribute_parents::<T>` exactly there — \
         behind the resolve, answering the spawn's `Insert<T>` as a `Changed<T>` — and L-4 moved that \
         mark phase in front of it",
        spec.shape_id()
    );
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
        markers_at_handoff,
    }
}

/// Leaves the app with nothing for a tick to do: one [`tick`] so the modifiers Startup applied are
/// resolved, then [`clear_dirty`] for whatever that tick left, then one [`run_fixed_tick`] — the very
/// call the benches time — to show the zero-dirty shape holds.
///
/// Returns how many entities carried a `Dirty<T>` when the world was handed over, not how many
/// [`clear_dirty`] removed. Only a shape with nothing to fold leaves the settle tick with zero to
/// remove — a shape with effects keeps the marks its first resolve fanned out, which is the second pass
/// of the cascade rather than a mark the schedule left behind — so reading the count after
/// [`clear_dirty`] would report a number the call sites cannot attribute, and the `> 0` guards there
/// would stop proving the walk ever saw a marker. Read before the tick, 0 still means "this shape was
/// never dirty".
pub fn prepare_floor(app: &mut App) -> usize {
    prepare_floor_tallies(app).0
}

/// [`prepare_floor`] with both tallies: `(dirty entities at handoff, Dirty<T> markers clear_dirty
/// removed after the settle tick)`. The two are not in the same unit — one entity carries one marker per
/// attribute — and a caller that needs the second one is pinning the settle tick's own work, not its
/// shape's.
pub fn prepare_floor_tallies(app: &mut App) -> (usize, usize) {
    let marked = dirty_entity_count(app.world());
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
    (marked, cleared)
}

/// Removes every `Dirty<T>` from the world and returns how many components came off. Ticking reaches the
/// same place now (module docs), but this gets there in one step and without the resolve work that tick
/// has to spend.
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

/// A dirty entity split by the kind of entity carrying the marker. [`dirty_entity_count`]'s bare
/// number cannot tell an actor from the effect and modifier nodes under an actor, and the two need
/// different words in a failure: a mark that will not leave an actor is the D-34 re-dirty bug, a mark
/// left on a node means the graph walk never finished cleaning.
pub struct DirtySplit {
    pub actors: Vec<Entity>,
    pub nodes: Vec<DirtyNode>,
}

/// One dirty non-actor entity, with the `Dirty<T>` names on it so a node that stays dirty says which
/// attribute kept it.
pub struct DirtyNode {
    pub entity: Entity,
    pub markers: Vec<String>,
}

impl DirtySplit {
    /// The same world as [`dirty_entity_count`] sees it; a guard asserts the two agree so the split
    /// cannot be quieter than the count it decomposes.
    pub fn total(&self) -> usize {
        self.actors.len() + self.nodes.len()
    }

    pub fn node_labels(&self) -> Vec<String> {
        self.nodes
            .iter()
            .map(|node| format!("{:?}[{}]", node.entity, node.markers.join("+")))
            .collect()
    }
}

/// [`dirty_entity_count`] read per entity, so a guard can say *which* entities carry the markers.
/// Same archetype walk, same `is_dirty_component` oracle, one entry per dirty entity whatever it
/// carries.
pub fn dirty_split(world: &World) -> DirtySplit {
    let mut split = DirtySplit {
        actors: Vec::new(),
        nodes: Vec::new(),
    };
    for archetype in world.archetypes().iter() {
        let markers: Vec<String> = archetype
            .components()
            .iter()
            .copied()
            .filter(|id| is_dirty_component(world, *id))
            .filter_map(|id| world.components().get_name(id).map(|name| name.to_string()))
            .collect();
        if markers.is_empty() {
            continue;
        }
        for slot in archetype.entities() {
            let entity = slot.id();
            if world.get::<Actor>(entity).is_some() {
                split.actors.push(entity);
            } else {
                split.nodes.push(DirtyNode {
                    entity,
                    markers: markers.clone(),
                });
            }
        }
    }
    // Archetype order is not stable across runs, and a guard message has to be comparable.
    split.actors.sort();
    split.nodes.sort_by_key(|node| node.entity.index());
    split
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
