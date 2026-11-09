//! B-4c: the two things B-4 left printed rather than pinned — what a mark does to an **effect node**,
//! and what a **NaN** does to a mark.
//!
//! ## Half one: who is inside `floor::dirty_entity_count`
//!
//! [`floor::dirty_entity_count`] is one number over the whole world, and `test_dirty_lifetime.rs` and
//! `test_dirty_spread.rs` both *print* it beside their per-actor reads (2 on B-4's shape, always). A
//! count that includes the 8 effect entities of that shape is satisfied by the two marked actors, so a
//! resolver fix that left every effect and modifier node dirty forever would keep both files green.
//! [`floor::dirty_split`] decomposes the same walk into actors and non-actor nodes, and
//! [`the_dirty_count_is_actors_and_never_the_effect_nodes`] pins the composition at four moments: at
//! the build, after [`floor::prepare_floor`], at the mark, and at B-4's 22-tick horizon.
//!
//! The build is the positive control and it is not a small one: 4 actors plus 8 effect entities, the
//! nodes dirty on `Dirty<Stacks>` — the attribute `AttributesPlugin` registers into every app, which is
//! also why the walk matches `Dirty` by name rather than over the pool. So a zero node count later is a
//! read, not a blind spot, and the second moment proves those node markers are removable:
//! `prepare_floor` takes 24 markers off the world (16 on the actors — the pool's three plus `Stacks` —
//! and 8 on the effect entities) and leaves nothing.
//!
//! ### What the node half is guaranteed to do, which is not "never dirty"
//!
//! A node marker is raised by `update_modifier_when_dependencies_changed`
//! (`crates/vitality_effects/src/modifier.rs:222-228`), which triggers `MarkNodeDirty` on the effect
//! entity from `EffectsSet::Notify` — *after* `update_current_value_system` has cleaned the nodes of
//! that tick. So the first tick of a live cascade legitimately ends with one node marker on the world,
//! NaN or not: measured on this file's finite control round (a `+3` write to the source, no NaN
//! anywhere) tick 1 ends with `84v0[Dirty<NanDep>]`, kind `effect`, and ticks 2 onward with none. A
//! per-tick "no node is dirty" assert would therefore be red on a world that is working, today and
//! after D-34 alike.
//!
//! What is pinned instead is the claim the walk actually makes. Only `update_effect_tree_attributes`
//! takes a node marker off, and it reaches a node in the tick after the actor above it was dirty. So:
//! **no node marker survives two consecutive reads**, and **no node marker is present on a tick where
//! no actor is marked at all** (nothing ran to raise one and nothing ran to remove one). Both hold at
//! every tick of all three tests here, at both library versions. A red is a library regression — the
//! graph walk stopped cleaning the nodes it walked — and it is not something D-34 or D-38 is expected
//! to cause: those change `are_different`, which decides whether a *value* counts as changed, and marks
//! are raised on actors. Measured on the fix (scratch copy, `self != rhs` at
//! `crates/vitality_core/src/math.rs:33`): the node marker still appears on tick 1 of every live round
//! and is gone on tick 2, so both halves of the guarantee stay green.
//!
//! ## Half two: NaN is the one value where a mark decays
//!
//! `AbsDiff::are_different` for floats is `self.abs_diff(rhs) > Self::Output::MIN`
//! (`crates/vitality_core/src/math.rs:32-34`) and `MIN` is `f32::MIN`, so it is true for every pair of
//! values **except** one with a NaN operand, where the comparison itself is false. A resolve that
//! evaluates to NaN therefore reads as "unchanged" (`vitality_core/src/attributes.rs:322-331`), which
//! means three things, all probed on this shape (one source actor, one reader actor, one permanent
//! `EffectSubject::Source` modifier so the reader's `NanDep` value comes from the source's `NanAttr`):
//!
//! * **the mark decays.** A marked actor with a NaN-valued resolve loses its `Dirty<NanAttr>` — one
//!   tick after a bare mark, two after the base write that also re-marks it through `Changed<T>` — and
//!   stays clean for every tick after that, while the *finite* attribute of the same actor, marked by
//!   the same call, is still dirty at the horizon. This is the only decay today;
//! * **the write is swallowed.** `current_value` keeps the last finite value it had: the base goes to
//!   NaN and the current value sits at 50.4976806640625 for every tick of this file's NaN round,
//!   including across a fresh `MarkNodeDirty`. NaN can reach `current_value` unguarded in two other
//!   ways — `set_current_value` is public and `attribute_impl!`'s `new()`
//!   (`crates/vitality_core/src/attributes.rs:242-250`) writes base *and* current with no comparison at
//!   all — so a NaN attribute is silently stale: nothing ever recomputes it and nothing reports that it
//!   did not;
//! * **the cascade dies with it.** `should_notify_observers` is false, so no `CurrentValueChanged` and
//!   no `RecalculateExpression`: the reader never gains a `Dirty<NanDep>` and keeps its derived value
//!   forever. The round just before the NaN one, on the same edge, is the file's positive control: one
//!   `+3` write to the source moved the reader's value by 3 inside three ticks and put `Dirty<NanDep>`
//!   on the reader and — on its first tick, which is when the node marker is legal — on its effect
//!   node. So the dead cascade is the NaN, not the shape.
//!
//! So on *this* shape — NaN written to `base`, with the resolver's own comparison the only thing in
//! front of `current_value` — no NaN reaches the reader's stored value, because the expression that
//! would carry it (`NanAttr::src()` reads `current_value`,
//! `crates/vitality_core/src/attributes.rs:91-100`) is fed the stale finite value the resolver refused
//! to overwrite. That is a property of how the NaN got in, not of NaN: [`a_nan_left_in_current_value_leaves_its_dependent_dirty_and_stale`]
//! puts one straight into `current_value` (the public `set_current_value`) and the dependent there is
//! dirty on every tick of the horizon *and* stale on every one of them — permanently dirty, because
//! marks never decay, and permanently stale, because a NaN resolve notifies nothing. A NaN spawned in
//! through `Attribute::new(NaN)` is the third case: the source's own mark decays at tick 2 and the
//! reader is never marked at all. Nothing panicked and nothing wedged in any of the three: every probe
//! reaches every tick it was asked for.
//!
//! ## Which asserts D-34 flips
//!
//! `.auto/decisions.md` D-34 (approved, D-38) makes `are_different` `!=` with **no** epsilon, so NaN
//! counts as changed. Measured on the fix in a scratch copy (`self != rhs` at `math.rs:33`, the file
//! otherwise untouched) **thirteen** asserts go red and they are the whole list below — every one of
//! them says *invert*, and nothing else in the file goes red: not the node guarantee of half one, not
//! the split-vs-count agreement, not either `prepare_floor` control, not the two live-edge controls, not
//! the three asserts that only hold while `mark_dirty_fraction` flushes, not the settled read between the
//! two rounds of the NaN test, and not the frozen dependent value of the third test. (The reset between
//! those two rounds deliberately reports the marker count instead of asserting it: under D-34 the finite
//! round leaves nothing dirty at all, so `prepare_floor` there removes 0 and that is not a failure.)
//!
//! * *the marked actors are still dirty at the horizon* — 3 asserts, all in half one: the actor half
//!   read two ticks after the mark, the actor count at tick 22 and the world count at tick 22.
//!   Measured: the two marked actors are clean on **tick 1** and the walk reports 0 dirty entities
//!   there, so invert to "empty at tick 1 / 0 from tick 1";
//! * *the NaN write is swallowed* — 3 asserts, each one a `current_value`: the source's in the base
//!   round (keeps the frozen 50.4976806640625), its twin after the bare mark, and the third test's "the
//!   NaN stays in `current_value`". Measured: the base round writes NaN from tick 1 and the bare mark
//!   leaves NaN in place, while the current-value probe has its NaN **erased** at tick 1 by the
//!   write-back of the finite base — the first two invert to "is NaN", the third to "is the base
//!   again", so read each message rather than flipping them alike;
//! * *the cascade stops at the NaN* — 3 asserts, each one the reader: its value in the base round (kept
//!   at 63.22623825073242), its marks in the same round, and its marks after the bare mark. Measured:
//!   the reader reads NaN from tick 2 and carries `Dirty<NanDep>` on every tick of the round, so invert
//!   all three;
//! * *the NaN mark decays, and nothing else decays* — 4 asserts: the `NanAttr`-absent one and the
//!   `NanDep`-present control beside it (base round, tick ≥ 2), the bare-mark `marks == ["NanDep"]`
//!   (which asserts both halves of that pair in one), and the third test's "the dependent stays
//!   dirty". Measured: the source keeps `Dirty<NanAttr>` on every tick and loses `Dirty<NanDep>` at
//!   tick 1, so the pair is exactly inverted — flip the four together.
//!
//! The last group needs a second look at the phrase "a NaN mark then lives as long as a finite one's",
//! which the first pass of this file used and which is **wrong** after the fix: `NaN != NaN` is true,
//! so a NaN resolve reports a change every tick it runs. Post-fix the source's `Dirty<NanAttr>` and the
//! reader's `Dirty<NanDep>` never leave at all and the reader's `NanDep` reads NaN from tick 2 onward,
//! while a *finite* marked actor is clean at tick 1. A NaN mark outlives the finite one, it does not
//! join it — and that permanently-dirty-permanently-NaN world is the steady state D-34 hands to any
//! dependent of a NaN attribute, which is what the third test pins from the other side today.
//!
//! NaN stays inside `NanAttr`/`NanDep` in a world this file builds and never shares, because a NaN
//! current value can propagate through dependents and no other test may inherit one.

#[path = "../benches/support/common.rs"]
mod common;
#[path = "../benches/support/floor.rs"]
mod floor;
#[path = "../benches/support/recompute.rs"]
mod recompute;

use crate::common::{
    AttrSpec, HarnessWorld, WorldSpec, actor_entities, build_app_with, mark_dirty_fraction,
    seed_rng, tick,
};
use crate::floor::{DirtyNode, DirtySplit, dirty_entity_count, dirty_split, prepare_floor};
use crate::recompute::dirty_names;
use bevy::prelude::*;
use std::marker::PhantomData;
use vitality::attributes::Attribute;
use vitality::effect::Effect;
use vitality::effect::definition::EffectDef;
use vitality::effect::modifier::{EffectSubject, ModOp, ModifierMarker};
use vitality::effect::systems::MarkNodeDirty;
use vitality::effect::{ApplyEffectEvent, EffectBuilder, EffectTargeting};
use vitality::prelude::*;

/// B-4's shape, so this file reads the world `test_dirty_lifetime.rs` already characterises.
const ACTORS: usize = 4;
const ATTRIBUTES: usize = 3;
const EFFECTS: usize = 2;
const MODIFIERS: usize = 1;

/// The owner's "mark 2 of 4", and the number B-4 only printed.
const MARKED: usize = 2;

/// B-4's horizon: its two ticks plus twenty more. The node half has to hold for as long as the marks
/// do, not only at the tick they were made.
const HORIZON: usize = 22;

// The NaN world's attribute types. Named for what they are so no other file can read a NaN off a
// shared pool slot.
vitality::attribute!(NanAttr);
vitality::attribute!(NanDep);

static NAN_POOL: &[AttrSpec] = &[
    AttrSpec::new::<NanAttr>("NanAttr"),
    AttrSpec::new::<NanDep>("NanDep"),
];

/// The reader's `NanDep` is `base + NanAttr::src()`, so this write has to show up on the reader for
/// the NaN half to be measuring a live cascade.
const NAN_STEP: f64 = 3.0;

/// Past the tick the NaN mark is measured to be gone on (tick 2 after a base write), with room to show
/// it does not come back. The stale-dependent test reads the same number of ticks.
const NAN_HORIZON: usize = 6;

/// The reader's value is an `f32` sum read back as `f64`; the assert is about a 3.0 move on a value in
/// the 60s, so this only absorbs the last bits of the sum.
const VALUE_EPS: f64 = 1e-3;

/// What kind of entity a dirty non-actor node is, so a failure says `effect` or `modifier` rather
/// than a bare count.
fn node_kind(world: &World, entity: Entity) -> &'static str {
    if world.get::<Effect>(entity).is_some() {
        "effect"
    } else if world.get::<ModifierMarker>(entity).is_some() {
        "modifier"
    } else {
        "other"
    }
}

/// What the non-actor half of a [`DirtySplit`] is made of.
fn node_kinds(world: &World, split: &DirtySplit) -> Vec<&'static str> {
    let mut kinds: Vec<&'static str> = split
        .nodes
        .iter()
        .map(|node| node_kind(world, node.entity))
        .collect();
    kinds.sort();
    kinds
}

/// The split and the count are two reads of one walk; every assert below starts by saying they agree,
/// so a decomposition can never be quieter than the number it decomposes — which is the number the
/// §3 allocations row uses as its denominator.
fn assert_split_matches_count(world: &World, split: &DirtySplit, when: &str) {
    assert_eq!(
        split.total(),
        dirty_entity_count(world),
        "{when}: `dirty_split` sees {} dirty entities ({} actors, {} nodes) and `dirty_entity_count` \
         sees {}; the two walk the same archetypes, so one of them has stopped looking at a marker \
         type and every count taken from it is wrong",
        split.total(),
        split.actors.len(),
        split.nodes.len(),
        dirty_entity_count(world),
    );
}

/// The node half of the walk, read at a tick boundary, against the same read at the previous one.
///
/// The guarantee is the narrow one the module doc derives: one node marker per live round is legal
/// (`EffectsSet::Notify` marks the effect entity after the walk has cleaned the nodes), so what is
/// pinned is that a marker never survives a second boundary and never sits on a world with no marked
/// actor. Takes the previous boundary's split and replaces it with this one.
fn assert_node_markers_are_being_cleaned(world: &World, previous: &mut DirtySplit, when: &str) {
    let split = dirty_split(world);
    assert_split_matches_count(world, &split, when);

    if split.actors.is_empty() {
        assert!(
            split.nodes.is_empty(),
            "B-4c effect nodes: no actor carries a `Dirty<T>` {when}, so no cascade ran to raise one \
             and none is running to keep one, yet {} non-actor entities still do — {:?}, kinds {:?}. \
             Only `update_effect_tree_attributes` takes a node marker off and it does that while the \
             actor above the node is dirty, so a node marker with nothing marked above it is one that \
             walk will never reach again. `are_different` is not on that path: no approved fix \
             (`.auto/decisions.md` D-34, D-38) is expected to cause this, so it is a library regression",
            split.nodes.len(),
            split.node_labels(),
            node_kinds(world, &split),
        );
    }

    let carried: Vec<&DirtyNode> = split
        .nodes
        .iter()
        .filter(|node| {
            previous
                .nodes
                .iter()
                .any(|before| before.entity == node.entity)
        })
        .collect();
    if carried.is_empty() {
        *previous = split;
        return;
    }
    let labels: Vec<String> = carried
        .iter()
        .map(|node| format!("{:?}[{}]", node.entity, node.markers.join("+")))
        .collect();
    let kinds: Vec<&'static str> = carried
        .iter()
        .map(|node| node_kind(world, node.entity))
        .collect();
    panic!(
        "B-4c effect nodes: {} non-actor entities carried a `Dirty<T>` at the previous read and still \
         do {when} — {labels:?}, kinds {kinds:?}. One marker per round is the notify path marking the \
         effect entity after the walk cleaned it (`vitality_effects/src/modifier.rs:222-228`); the same \
         node dirty on two consecutive boundaries means the walk stopped cleaning, which no approved \
         fix (D-34, D-38) is on the path to cause. Regression, not a flip",
        carried.len(),
    );
}

/// The actors a per-attribute `Dirty<T>` read finds, in `actor_entities` order — the read the world
/// walk above has to agree with, and the one that goes blind if a marking helper stops flushing.
fn actors_with_marks(world: &World, pool: &[AttrSpec], actors: &[Entity]) -> Vec<Entity> {
    actors
        .iter()
        .copied()
        .filter(|actor| !dirty_names(world, pool, *actor).is_empty())
        .collect()
}

/// The per-actor and world-level read, printed at each moment the asserts below read. Reported rather
/// than asserted, the way the two dirty files do it: the asserts say who is dirty, and this is the line
/// a report quotes.
fn report(label: &str, world: &World, pool: &[AttrSpec], actors: &[Entity]) {
    let per_actor: Vec<String> = actors
        .iter()
        .map(|actor| format!("{actor:?}={:?}", dirty_names(world, pool, *actor)))
        .collect();
    let split = dirty_split(world);
    println!(
        "B-4c {label}: {} of {} actors dirty {per_actor:?}; world walk sees {} dirty entities \
         ({} actors, {} nodes {:?})",
        actors_with_marks(world, pool, actors).len(),
        actors.len(),
        dirty_entity_count(world),
        split.actors.len(),
        split.nodes.len(),
        node_kinds(world, &split),
    );
}

/// The reader's one permanent effect: `NanDep += NanAttr::src()`, applied to the reader with the source
/// in the targeting pair, the way `cross_actor::apply_cascade` applies its cascade. `MarkNodeDirty` on
/// the reader asks for the first fold, without which the modifier value stays whatever the spawn
/// evaluated and the resolver never touches it (`benches/support/cross_actor.rs`, "Two other things
/// this shape shows").
fn apply_reader_effect(world: &mut World, source: Entity, reader: Entity) {
    let handle = world.resource_mut::<Assets<EffectDef>>().add(
        EffectBuilder::permanent()
            .modify::<NanDep>(NanAttr::src(), ModOp::Add, EffectSubject::Source)
            .name("b4c.nan.reader".to_string())
            .build(),
    );
    world.trigger(ApplyEffectEvent {
        entity: reader,
        targeting: EffectTargeting::new(source, reader),
        handle,
    });
    world.trigger(MarkNodeDirty::<NanDep> {
        entity: reader,
        phantom_data: PhantomData,
    });
    world.flush();
}

/// The two NaN shapes build the same world: two actors, the reader reading the source through one
/// permanent modifier, nothing periodic and nothing else registered.
fn nan_world() -> (App, Entity, Entity) {
    let spec = WorldSpec {
        attributes_per_actor: NAN_POOL.len(),
        effects_per_actor: 0,
        modifiers_per_effect: 0,
        actors: 2,
        dirty_fraction: 0.0,
        ticking_fraction: 0.0,
    };
    let mut app = build_app_with(&spec, NAN_POOL);
    let actors = actor_entities(app.world_mut());
    assert_eq!(
        actors.len(),
        2,
        "the NaN shape needs one source and one reader and built {}",
        actors.len(),
    );
    let (source, reader) = (actors[0], actors[1]);
    let cleared = prepare_floor(&mut app);
    assert!(
        cleared > 0,
        "the NaN shape left no `Dirty<T>` for `prepare_floor` to remove, so the clean reads below are \
         a walk that never saw a marker",
    );
    apply_reader_effect(app.world_mut(), source, reader);
    let folded = prepare_floor(&mut app);
    assert!(
        folded > 0,
        "applying the reader's effect and asking for the first fold marked nothing, so the resolver \
         never looked at the edge this file reads NaN through",
    );
    (app, source, reader)
}

/// One door a NaN has into `current_value` that does not go through the resolver's comparison at all.
fn put_nan_in_current_value(world: &mut World, actor: Entity) {
    let mut attribute = *world.get::<NanAttr>(actor).expect("NanAttr on source");
    attribute.set_current_value(f32::NAN);
    world.entity_mut(actor).insert(attribute);
    world.flush();
}

/// The finite `+NAN_STEP` round that proves the edge is live, and that leaves the marks a settled
/// world cannot have raised. Returns the reader's value afterwards.
fn finite_control_round(
    app: &mut App,
    source: Entity,
    reader: Entity,
    previous: &mut DirtySplit,
) -> f64 {
    let pool: &'static [AttrSpec] = app.world().resource::<HarnessWorld>().attrs();
    let (nan, dep) = (&pool[0], &pool[1]);
    let reader_before = dep.current(app.world(), reader).unwrap();
    let source_base = nan.base(app.world(), source).unwrap();
    nan.set_base(app.world_mut(), source, source_base + NAN_STEP);
    let mut rng = seed_rng();
    let marked = mark_dirty_fraction(app.world_mut(), pool, &[source], 1.0, &mut rng);
    assert_eq!(
        marked, 1,
        "marking one actor through the helper marked {marked}"
    );
    // Read before any tick: this is the helper's flush and nothing else, and it says the same thing on
    // both sides of D-34 because no resolver has run yet.
    let marked_now = dirty_names(app.world(), pool, source);
    assert_eq!(
        marked_now,
        ["NanAttr", "NanDep"],
        "the source reads {marked_now:?} the moment `mark_dirty_fraction` returned {marked}: with the \
         helper's `world.flush()` gone the `try_insert(Dirty<T>)` its observer queued is still a \
         buffered command, so the rounds below start from a world that looks unmarked until the first \
         tick applies it",
    );

    for number in 1..=3 {
        tick(app);
        let world = app.world();
        assert_node_markers_are_being_cleaned(
            world,
            previous,
            &format!("finite round, tick {number}"),
        );
    }
    let reader_moved = dep.current(app.world(), reader).unwrap();
    assert!(
        reader_moved.is_finite() && (reader_moved - reader_before - NAN_STEP).abs() < VALUE_EPS,
        "the edge is not live: a +{NAN_STEP} write to the source left the reader's NanDep at \
         {reader_moved} (was {reader_before}), so the NaN half below would be measuring a dead \
         cascade rather than a NaN one",
    );
    reader_moved
}

#[test]
fn the_dirty_count_is_actors_and_never_the_effect_nodes() {
    let spec = WorldSpec {
        attributes_per_actor: ATTRIBUTES,
        effects_per_actor: EFFECTS,
        modifiers_per_effect: MODIFIERS,
        actors: ACTORS,
        dirty_fraction: 0.0,
        ticking_fraction: 0.0,
    };
    let mut app = build_app_with(&spec, common::ATTRIBUTE_POOL);

    // Positive control, and the only reason a node count of 0 below means anything: at the build the
    // walk sees every actor *and* every effect entity of the shape carrying a marker (`Dirty<Stacks>`
    // on the nodes, which `clear_dirty` and `update_current_value_system` are the only ways off).
    let world = app.world();
    let built = dirty_split(world);
    assert_split_matches_count(world, &built, "at the build");
    assert_eq!(
        built.actors.len(),
        ACTORS,
        "at the build the walk finds {} dirty actors of {ACTORS}: `build_app_with` returns after \
         Startup, so `on_add_attribute` has marked every attribute of every actor and a walk that sees \
         fewer cannot report a clean world later either",
        built.actors.len(),
    );
    assert_eq!(
        node_kinds(world, &built),
        vec!["effect"; EFFECTS * ACTORS],
        "at the build the walk's non-actor half is not the {} effect entities of shape {}: a \
         `dirty_entity_count` that cannot see a node marker here would also report 0 nodes after a \
         fix that leaves every node dirty forever, which is the failure this file exists to catch",
        EFFECTS * ACTORS,
        spec.shape_id(),
    );

    // The node markers are removable, so the zeros after this are a settled world, not a walk that
    // lost the archetype.
    let cleared = prepare_floor(&mut app);
    let world = app.world();
    let settled = dirty_split(world);
    assert_split_matches_count(world, &settled, "after `prepare_floor`");
    assert!(
        cleared >= built.total(),
        "`prepare_floor` removed {cleared} markers from a world the walk had just counted at {} dirty \
         actors and {} dirty nodes: every one of those entities carries at least one marker, so a \
         smaller number means the prepare missed a marker type and the zeros below are its, not the \
         world's",
        built.actors.len(),
        built.nodes.len(),
    );
    assert_eq!(
        settled.total(),
        0,
        "shape {} is not settled: {} dirty entities survive `prepare_floor` ({:?} actors, {:?} nodes), \
         so nothing below can attribute a mark to this file's own call",
        spec.shape_id(),
        settled.total(),
        settled.actors,
        settled.node_labels(),
    );

    let pool: &'static [AttrSpec] = app.world().resource::<HarnessWorld>().attrs();
    let actors = actor_entities(app.world_mut());
    let mut rng = seed_rng();
    let marked = mark_dirty_fraction(
        app.world_mut(),
        pool,
        &actors,
        MARKED as f32 / ACTORS as f32,
        &mut rng,
    );
    assert_eq!(
        marked, MARKED,
        "a {MARKED}/{ACTORS} fraction over {ACTORS} actors marked {marked}",
    );

    let world = app.world();
    report("at the mark (flushed)", world, &pool, &actors);
    let marked_actors = actors_with_marks(world, &pool, &actors);
    let split = dirty_split(world);
    assert_split_matches_count(world, &split, "at the mark");
    assert_eq!(
        marked_actors.len(),
        MARKED,
        "the per-attribute `Dirty<T>` read finds {} marked actors after `mark_dirty_fraction` returned \
         {MARKED}: with the helper's `world.flush()` gone the `try_insert(Dirty<T>)` its observer queued \
         is still in the command buffer, and both this read and the walk below see an empty world",
        marked_actors.len(),
    );
    assert_eq!(
        split.actors, marked_actors,
        "at the mark the walk's actor half is {:?} and a per-attribute `Dirty<T>` read finds {:?}",
        split.actors, marked_actors,
    );
    assert!(
        split.nodes.is_empty(),
        "marking actors put a `Dirty<T>` on {} non-actor entities — {:?}: a mark rides the \
         `EffectTarget` chain, but the entities that carry one here carry their effects on themselves, \
         so nothing should have been handed a node marker",
        split.nodes.len(),
        split.node_labels(),
    );

    // B-4's two ticks, then its horizon. The node half is read every tick; the actor half at the two
    // points the count is quoted at.
    let mut previous = dirty_split(app.world());
    for number in 1..=2 {
        tick(&mut app);
        let world = app.world();
        report(&format!("tick {number}"), world, &pool, &actors);
        assert_node_markers_are_being_cleaned(
            world,
            &mut previous,
            &format!("{number} ticks after the mark"),
        );
    }
    let world = app.world();
    assert_eq!(
        dirty_split(world).actors,
        marked_actors,
        "B-4c never decays: two ticks after the mark the walk's actor half is {:?} and not the marked \
         {marked_actors:?}. This is the half D-34 is approved to flip (`.auto/decisions.md`, same split \
         as `test_dirty_lifetime.rs`): measured on the fix (scratch, `!=` at `math.rs:33`) a marked \
         actor's own `Dirty<T>` is already gone at **tick 1** and the world walk reports 0 dirty \
         entities there, so invert this to `actors is empty at tick 1` — do not delete it, or the count \
         loses the only thing that says whose marks it was",
        dirty_split(world).actors,
    );

    for number in 3..=HORIZON {
        tick(&mut app);
        let world = app.world();
        assert_node_markers_are_being_cleaned(world, &mut previous, &format!("tick {number}"));
    }

    let world = app.world();
    report(&format!("tick {HORIZON}"), world, &pool, &actors);
    // The number B-4 printed and nobody asserted, now pinned as a composition: all of it actors, none
    // of it the shape's 8 effect entities or their modifiers.
    let split = dirty_split(world);
    assert_split_matches_count(world, &split, &format!("tick {HORIZON}"));
    assert_eq!(
        split.actors.len(),
        MARKED,
        "the count B-4 quotes as 2 is {} actors and {} nodes at tick {HORIZON}: {} (D-34 flips this to \
         0 actors, inverting rather than deleting: the point is that the count is all actors and none \
         of it nodes, whichever way the marks go)",
        split.actors.len(),
        split.nodes.len(),
        split.node_labels().join(", "),
    );
    assert_eq!(
        dirty_entity_count(world),
        MARKED,
        "at tick {HORIZON} the world walk reports {} dirty entities and not {MARKED}; its actor half \
         is {:?} and its node half {:?}. If the extra ones are nodes, that is the regression the \
         per-tick node assert above is for; if the list has shrunk to nothing, D-34 landed and this is \
         the assert to invert (expect 0) rather than delete",
        dirty_entity_count(world),
        split.actors,
        split.node_labels(),
    );
}

#[test]
fn a_nan_resolve_decays_its_mark_and_freezes_what_it_would_notify() {
    let (mut app, source, reader) = nan_world();
    let mut previous = dirty_split(app.world());
    assert_node_markers_are_being_cleaned(app.world(), &mut previous, "after the fold settled");

    let pool: &'static [AttrSpec] = app.world().resource::<HarnessWorld>().attrs();
    let (nan, dep) = (&pool[0], &pool[1]);
    assert_eq!(
        pool[0].name, "NanAttr",
        "slot 0 of the NaN pool is {}, and the write below would not be the attribute the reader's \
         modifier reads",
        pool[0].name,
    );

    // Positive control: the edge is live, so the NaN step below cannot pass by measuring a cascade
    // that never worked.
    let reader_moved = finite_control_round(&mut app, source, reader, &mut previous);

    // Back to settled, and then the same write with NaN in it. The count the prepare removed is
    // reported, not asserted: under D-34 the round above leaves nothing dirty at all, so a nonzero
    // count here is a today-only fact and the settled read below is the thing that matters.
    let reset = prepare_floor(&mut app);
    let world = app.world();
    let settled = dirty_split(world);
    assert_split_matches_count(world, &settled, "between the rounds");
    assert_eq!(
        settled.total(),
        0,
        "the world is not settled between the two rounds: {} dirty entities survive `prepare_floor` \
         (which removed {reset} markers, {:?} actors and {:?} nodes left), so the NaN round below \
         cannot attribute a mark to its own write",
        settled.total(),
        settled.actors,
        settled.node_labels(),
    );
    let mut previous = settled;

    nan.set_base(app.world_mut(), source, f64::NAN);
    let mut rng = seed_rng();
    let marked = mark_dirty_fraction(app.world_mut(), pool, &[source], 1.0, &mut rng);
    assert_eq!(
        marked, 1,
        "marking the source for the NaN round marked {marked} actors"
    );
    let world = app.world();
    let frozen = nan.current(world, source).unwrap();
    assert!(
        nan.base(world, source).unwrap().is_nan(),
        "the source's NanAttr base reads {}, not NaN: the write this file makes is not the NaN the \
         asserts below are about",
        nan.base(world, source).unwrap(),
    );
    assert!(
        !frozen.is_nan(),
        "the NaN is already in current_value ({frozen}) before a single tick has resolved anything, so \
         nothing here can show whether a NaN resolve writes it",
    );
    assert_eq!(
        dirty_names(world, &pool, source),
        ["NanAttr", "NanDep"],
        "marking the source through `mark_dirty_fraction` left it carrying {:?}; with the helper's \
         `world.flush()` gone those inserts are still queued commands and this file's whole NaN half \
         reads a world nothing was marked on",
        dirty_names(world, &pool, source),
    );
    let reader_at_nan = dep.current(world, reader).unwrap();
    assert!(
        dirty_names(world, &pool, reader).is_empty(),
        "the reader {reader:?} carries {:?} the moment the NaN is written, before a single tick has \
         resolved anything",
        dirty_names(world, &pool, reader),
    );
    assert_eq!(
        reader_at_nan, reader_moved,
        "the reader's NanDep reads {reader_at_nan} at the NaN write but {reader_moved} at the end of \
         the finite round, so the world the NaN round starts on is not the settled one the assert above \
         said it was",
    );

    for number in 1..=NAN_HORIZON {
        tick(&mut app);
        let world = app.world();
        let marks = dirty_names(world, &pool, source);
        println!(
            "B-4c nan tick {number}: source marks {marks:?} NanAttr base={} current={} \
             (frozen {frozen}); reader marks {:?} NanDep current={}",
            nan.base(world, source).unwrap(),
            nan.current(world, source).unwrap(),
            dirty_names(world, &pool, reader),
            dep.current(world, reader).unwrap(),
        );
        assert_node_markers_are_being_cleaned(
            world,
            &mut previous,
            &format!("the NaN round, tick {number}"),
        );

        let current = nan.current(world, source).unwrap();
        assert!(
            !current.is_nan() && current == frozen,
            "B-4c swallowed write: at tick {number} of the NaN round the source's NanAttr reads \
             {current} and not the frozen {frozen}. Today `are_different(old, NaN)` is false, so the \
             resolve writes nothing and `current_value` keeps the last finite value forever; D-34's \
             `!=` counts NaN as changed, so once it lands the NaN **is** written and this assert goes \
             red by design — invert it (expect NaN) rather than delete it, because a NaN that silently \
             stops updating is a bug either way",
        );

        let reader_now = dep.current(world, reader).unwrap();
        assert!(
            reader_now.is_finite() && reader_now == reader_at_nan,
            "B-4c stopped cascade: at tick {number} of the NaN round the reader's NanDep reads \
             {reader_now}, having moved from {reader_at_nan}. A NaN resolve reports no change, so no \
             `RecalculateExpression` reaches the reader and its derived value is silently stale; when \
             D-34 lands NaN counts as changed, the NaN rides `NanAttr::src()` into this value, and this \
             assert flips to \"the reader reads NaN\" — invert, do not delete, since a dependent that \
             quietly absorbs a NaN is exactly what the fix is supposed to make loud",
        );

        if number >= 2 {
            assert!(
                !marks.contains(&"NanAttr"),
                "B-4c the NaN mark decays: at tick {number} the source still carries {marks:?}, \
                 including NanAttr. A NaN resolve is the one case today where `are_different` is false, \
                 so the resolve neither notifies nor re-marks and `update_current_value_system`'s \
                 `try_remove` is the end of the mark — that is the bug D-34 exists to remove. Invert \
                 this (expect the mark to survive every tick) rather than delete it, and read it \
                 together with the two asserts below: once `!=` lands the NaN mark never leaves, \
                 because `NaN != NaN` is true and every resolve re-marks, while the finite attribute \
                 marked by the same hand is clean by tick 1 — NaN outlives the finite mark rather than \
                 joining it",
            );
            assert!(
                marks.contains(&"NanDep"),
                "the control broke: at tick {number} the source's finite NanDep is clean ({marks:?}), \
                 so marks are decaying for finite values too and the NaN assert above is no longer \
                 showing anything about NaN. Either D-34 has landed — then both this and the assert \
                 above want inverting, together, and the reader assert below with them — or something \
                 else stopped the re-dirty loop",
            );
            assert!(
                dirty_names(world, &pool, reader).is_empty(),
                "the reader {reader:?} carries {:?} at tick {number} of the NaN round, but a NaN \
                 resolve notifies nothing, so no mark should ever have reached it. D-34 lands the NaN, \
                 the cascade starts, and the reader is dirty on every tick afterwards (a NaN resolve \
                 re-marks itself) — invert to \"carries NanDep on every tick\", do not delete",
                dirty_names(world, &pool, reader),
            );
        }
    }

    // A bare mark, with no write behind it, so the decay above cannot be blamed on the round's base
    // write (which re-marks the actor through `Changed<T>` and buys the mark one extra tick).
    nan.mark_dirty(app.world_mut(), source);
    app.world_mut().flush();
    tick(&mut app);
    let world = app.world();
    let marks = dirty_names(world, &pool, source);
    println!("B-4c nan bare mark: source marks {marks:?}");
    assert_node_markers_are_being_cleaned(world, &mut previous, "after the bare NaN mark");
    assert!(
        marks == ["NanDep"],
        "B-4c the NaN mark decays on its own: one tick after a bare `MarkNodeDirty<NanAttr>` the \
         source reads {marks:?} and not `[\"NanDep\"]`. The NaN resolve reports no change, so it \
         re-marks nothing and the mark is over — while the finite attribute marked by the same hand \
         survives. D-34's `!=` does not make the two alike, it swaps them: `NaN != NaN` is true so the \
         NaN mark is re-raised by every resolve and never leaves, and the finite one is gone at tick 1. \
         Invert both halves of the pair together, and see the module doc's note on the phrase \"as long \
         as a finite one's\"",
    );
    assert!(
        !nan.current(world, source).unwrap().is_nan()
            && dep.current(world, reader).unwrap() == reader_at_nan,
        "the bare NaN mark changed what a resolve writes or what the reader derives \
         ({} / {}), which today it does neither. Under D-34 the resolve writes the NaN and the reader \
         follows it, so invert — and keep the pair, because a value that stops updating in silence is \
         the bug on both sides of the fix",
        nan.current(world, source).unwrap(),
        dep.current(world, reader).unwrap(),
    );
    assert!(
        dirty_names(world, &pool, reader).is_empty(),
        "a bare mark on the NaN attribute reached the reader ({:?}): a NaN resolve notifies nothing, \
         so the cascade cannot start from a NaN. D-34 starts it — invert with the two asserts above",
        dirty_names(world, &pool, reader),
    );
}

/// The other door NaN has into a resolve: `set_current_value` is public and `attribute_impl!`'s
/// `new()` writes base *and* current unguarded, so a NaN can sit in a current value without ever
/// having been evaluated there. `NanAttr::src()` reads `current_value`, so this is the shape where a
/// dependent *is* being fed a NaN — and the state D-34 leaves every NaN dependent in.
#[test]
fn a_nan_left_in_current_value_leaves_its_dependent_dirty_and_stale() {
    let (mut app, source, reader) = nan_world();
    let mut previous = dirty_split(app.world());
    assert_node_markers_are_being_cleaned(
        app.world(),
        &mut previous,
        "before the NaN was put in place",
    );

    let pool: &'static [AttrSpec] = app.world().resource::<HarnessWorld>().attrs();
    let (nan, dep) = (&pool[0], &pool[1]);

    // Same finite round as the other test, and deliberately *not* settled afterwards: today the marks
    // it leaves never decay, which is half of what is being pinned here.
    let reader_moved = finite_control_round(&mut app, source, reader, &mut previous);

    put_nan_in_current_value(app.world_mut(), source);
    let world = app.world();
    let injected = nan.current(world, source).unwrap();
    let reader_at_inject = dep.current(world, reader).unwrap();
    assert!(
        injected.is_nan(),
        "the source's NanAttr current reads {injected} after `set_current_value(NaN)`, so the probe \
         below is not reading a NaN sitting in a current value",
    );
    assert!(
        !nan.base(world, source).unwrap().is_nan(),
        "the source's base is {} too: with NaN in both slots this is the `Attribute::new(NaN)` shape, \
         not the one the asserts below describe",
        nan.base(world, source).unwrap(),
    );
    assert_eq!(
        reader_at_inject, reader_moved,
        "the reader reads {reader_at_inject} at the injection and {reader_moved} at the end of the \
         finite round, so it has already been recomputed and cannot be shown to be stale",
    );

    for number in 1..=NAN_HORIZON {
        tick(&mut app);
        let world = app.world();
        let source_marks = dirty_names(world, &pool, source);
        let reader_marks = dirty_names(world, &pool, reader);
        let reader_now = dep.current(world, reader).unwrap();
        println!(
            "B-4c stale tick {number}: source marks {source_marks:?} NanAttr base={} current={}; \
             reader marks {reader_marks:?} NanDep current={reader_now}",
            nan.base(world, source).unwrap(),
            nan.current(world, source).unwrap(),
        );
        assert_node_markers_are_being_cleaned(
            world,
            &mut previous,
            &format!("the stale round, tick {number}"),
        );

        assert!(
            nan.current(world, source).unwrap().is_nan(),
            "a NaN that is already in `current_value` was overwritten at tick {number} ({}): today \
             `are_different(NaN, base)` is false, so the resolver leaves it in place forever and the \
             two asserts below are about a dependent that is never told about it. D-34 counts it as \
             changed and writes the finite base back — invert this to \"the current is the base again \
             from tick 1\", together with them",
            nan.current(world, source).unwrap(),
        );
        assert!(
            reader_marks.contains(&"NanDep"),
            "B-4c the stale dependent stops looking stale: at tick {number} the reader {reader:?} \
             reads {reader_marks:?}, so it is no longer carrying a mark it will never lose. Today the \
             pair is *permanently dirty and permanently stale* — the reader is re-marked by the \
             re-dirty loop and never recomputed, because a NaN resolve notifies nothing, which is the \
             worst of both and the state D-34 is approved to end. Once `!=` lands the NaN is written \
             out, the reader is re-marked once and then clean, so invert this to \"dirty at tick 1, \
             clean from tick 2\" rather than deleting it",
        );
        assert!(
            reader_now == reader_at_inject && reader_now.is_finite(),
            "the dependent recomputed itself ({reader_now}, was {reader_at_inject}) while its source's \
             current is NaN. Today nothing notifies off a NaN resolve, so this value is frozen at the \
             last finite thing the edge carried while the attribute it reads is NaN — permanently \
             stale, and quiet about it. D-34 leaves a dependent that reads NaN and is dirty about it \
             (see the NaN round of the test above), so the flip here is \"reads NaN\", not a delete",
        );
    }
}
