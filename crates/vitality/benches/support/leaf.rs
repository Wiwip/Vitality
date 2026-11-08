#![allow(dead_code)]

//! Family C (`leaf/access_paths`, BENCH-PLAN §2C): four ways to read one attribute value out of
//! one entity, all against the same [`WorldSpec`] world. Kept free of criterion so
//! `tests/test_bench_harness.rs` can include it and check the four agree.
//!
//! Setup is [`leaf_fixture`] — the app, one tick so the resolver has resolved, the entity to read,
//! the `ComponentId`, the `QueryState`, and the slot blocks filled *after* that tick. Measured is
//! one call to one `read_*`, plus, for path (i) only, the [`leaf_view`] fetch the library makes once
//! per actor before it touches any leaf. The number is ns/read.

use crate::common::{AttrSpec, HarnessWorld, WorldSpec, actor_entities, current_values, tick};
use bevy::ecs::bundle::Bundle;
use bevy::ecs::component::ComponentId;
use bevy::ecs::query::QueryState;
use bevy::ecs::world::EntityRefExcept;
use bevy::prelude::*;
use std::any::TypeId;
use vitality::attributes::Attribute;
use vitality::effect::modifier::ModifierOf;
use vitality::effect::{
    AppliedEffects, Effect, EffectDuration, EffectSource, EffectSources, EffectTarget,
    EffectTicker, EffectsRef,
};

/// The leaf all four paths read: slot 0 of the default pool.
pub type LeafAttr = crate::common::Attr0;

/// Where [`LeafAttr`] sits in a [`SlotBlock`].
pub const LEAF_SLOT: u16 = 0;

/// The exclusion bundle a leaf read really carries: `vitality_effects`' own attribute view, the one
/// the modifier expressions hand `Attribute::at()`. Named instead of spelled out at each use so
/// `leaf/exclusion_length` can sweep the bundle width without touching the read helpers.
pub type LeafBundle = (
    ChildOf,
    ModifierOf,
    (
        Effect,
        EffectDuration,
        EffectTicker,
        EffectSource,
        EffectTarget,
        AppliedEffects,
        EffectSources,
    ),
);

/// Compile-time pin, never called: `EffectsRef` *is* `EntityRefExcept<LeafBundle>`, so the bundle
/// above cannot drift from the library's alias unnoticed.
pub fn pin_leaf_bundle_to_effects_view(
    view: EntityRefExcept<'static, 'static, LeafBundle>,
) -> EffectsRef<'static, 'static> {
    view
}

/// The slot block a slot-block resolver would read: one `Vec<f64>` per actor, values in pool order.
#[derive(Component)]
pub struct SlotBlock(pub Vec<f64>);

/// Everything the paths need that is not the read itself, resolved once. The bench's
/// `iter_batched_ref` setup and `all_access_paths_agree` both go through here, so the test guards
/// the code the bench actually times.
pub struct LeafFixture {
    pub app: App,
    pub actors: Vec<Entity>,
    /// The entity the benches read.
    pub actor: Entity,
    /// [`LeafAttr`]'s id, resolved outside the loop for path (iii).
    pub id: ComponentId,
    /// How many components [`LeafBundle`] excludes, so a bench id can carry it.
    pub excl: usize,
    /// Hands out the [`EntityRefExcept`] view path (i) reads through.
    pub view: QueryState<EntityRefExcept<'static, 'static, LeafBundle>>,
}

pub fn leaf_fixture(spec: &WorldSpec) -> LeafFixture {
    let mut app = spec.build_app();
    let attrs = app.world().resource::<HarnessWorld>().attrs();
    assert_eq!(
        attrs[LEAF_SLOT as usize].name,
        stringify!(Attr0),
        "slot {LEAF_SLOT} of the pool is no longer {}, so the four paths would read different attributes",
        stringify!(Attr0),
    );

    // `build_app` only runs Startup, so the permanent Add modifiers are still pending and every
    // current_value still equals its base_value. Read a settled world or all four paths agree on
    // an unresolved number.
    tick(&mut app);

    let actors = actor_entities(app.world_mut());
    assert!(
        !actors.is_empty(),
        "shape {} spawned no actor to read",
        spec.shape_id()
    );
    // After the tick: a SlotBlock filled before it would freeze the pending value.
    fill_slot_blocks(app.world_mut(), attrs, &actors);

    let id = leaf_component_id(app.world());
    let excl = bundle_width::<LeafBundle>(app.world());
    let view = leaf_view_query(app.world_mut());
    let actor = actors[0];
    LeafFixture {
        app,
        actors,
        actor,
        id,
        excl,
        view,
    }
}

/// How many components the view excludes — the width `leaf/exclusion_length` sweeps, and the reason
/// it counts as a hidden variable in a bench id.
pub fn bundle_width<B: Bundle>(world: &World) -> usize {
    B::get_component_ids(world.components()).flatten().count()
}

/// Snapshots every actor's current values into a [`SlotBlock`]. Setup only: a resolver tick after
/// this leaves the block stale, which is why the test compares it against the live value.
pub fn fill_slot_blocks(world: &mut World, attrs: &[AttrSpec], actors: &[Entity]) {
    for &actor in actors {
        let values = current_values(world, attrs, actor);
        world.entity_mut(actor).insert(SlotBlock(values));
    }
}

/// [`LeafAttr`]'s id. The harness registers every attribute it builds actors from, so this never
/// takes the `RwLock` fallback in `Components::get_id`.
pub fn leaf_component_id(world: &World) -> ComponentId {
    world
        .components()
        .get_id(TypeId::of::<LeafAttr>())
        .expect("the harness registers the attribute it inserts")
}

/// The `QueryState` that hands out an `EntityRefExcept<B>`. Generic in `B` for the same reason as
/// [`LeafBundle`]: the exclusion-length sweep builds other bundles.
pub fn leaf_view_query<B: Bundle>(
    world: &mut World,
) -> QueryState<EntityRefExcept<'static, 'static, B>> {
    world.query::<EntityRefExcept<'static, 'static, B>>()
}

/// Path (i)'s view, fetched outside the read it is timed on. The fetch is the call the library makes
/// once per actor; the `get` below is the one it makes once per expression leaf.
pub fn leaf_view<'w, 's, B: Bundle>(
    world: &'w World,
    query: &'s mut QueryState<EntityRefExcept<'static, 'static, B>>,
    actor: Entity,
) -> EntityRefExcept<'w, 's, B>
where
    'w: 's,
{
    query
        .get(world, actor)
        .unwrap_or_else(|err| panic!("no leaf view on {actor:?}: {err}"))
}

/// (i) `EntityRefExcept::get::<LeafAttr>()` — the path `Attribute::at()` takes, so it pays the
/// `Components` lookup and the linear scan of the exclusion bundle.
pub fn read_except<B: Bundle>(view: &EntityRefExcept<'_, '_, B>) -> f64 {
    view.get::<LeafAttr>()
        .expect("the leaf attribute is in no exclusion bundle")
        .current_value() as f64
}

/// (ii) plain `EntityRef::get::<LeafAttr>()` — the same typed lookup with no bundle to scan.
pub fn read_entity_ref(world: &World, actor: Entity) -> f64 {
    world
        .entity(actor)
        .get::<LeafAttr>()
        .expect("every actor carries the leaf attribute")
        .current_value() as f64
}

/// (iii) `EntityRef::get_by_id` with the id resolved in [`leaf_fixture`] — no `TypeId` lookup left.
///
/// Only sound with an id from [`leaf_component_id`]: the raw pointer is read as a `LeafAttr`
/// whatever it actually points at.
pub fn read_by_id(world: &World, actor: Entity, id: ComponentId) -> f64 {
    let ptr = world
        .entity(actor)
        .get_by_id(id)
        .unwrap_or_else(|err| panic!("no leaf component on {actor:?}: {err}"));
    // SAFETY: `id` came from `leaf_component_id`, so it names a `LeafAttr` on this actor.
    unsafe { ptr.deref::<LeafAttr>() }.current_value() as f64
}

/// (iv) the reference line for a slot-block resolver: one storage lookup and one indexed read of a
/// `u16` slot.
pub fn read_slot(world: &World, actor: Entity) -> f64 {
    world
        .get::<SlotBlock>(actor)
        .unwrap_or_else(|| panic!("no slot block on {actor:?}: build it with leaf_fixture"))
        .0[LEAF_SLOT as usize]
}
