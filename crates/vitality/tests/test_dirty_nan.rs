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
//! (`crates/vitality_effects/src/modifier.rs:248-283`), which triggers `MarkNodeDirty` on the effect
//! entity from the `UpdateAttributeSignal` → `RecalculateExpression` observer chain that the resolve
//! itself queues — so it lands *after* `update_current_value_system` has cleaned the nodes of that tick.
//! (Since L-4 that chain runs inside the resolve rather than off `EffectsSet::Notify`, which changes the
//! set name and nothing else about this: measured here, tick 1 of a live round still ends with
//! `90v0[Dirty<NanDep>]`, kind `effect`, and ticks 2 onward with none.) A per-tick "no node is dirty"
//! assert would therefore be red on a world that is working, before and after L-4 alike.
//!
//! What is pinned instead is the claim the walk actually makes. Only `update_effect_tree_attributes`
//! takes a node marker off, and it reaches a node in the tick after the actor above it was dirty. So:
//! **no node marker survives two consecutive reads**, and **no node marker is present on a tick where
//! no actor is marked at all** (nothing ran to raise one and nothing ran to remove one). Both hold at
//! every tick of all three tests here, at both library versions. A red is a library regression — the
//! graph walk stopped cleaning the nodes it walked. D-34 changes `are_different`, which decides
//! whether a *value* counts as changed and therefore whether a resolve notifies off itself; measured
//! after it landed, the node marker still appears on tick 1 of every live round (a NaN round raises one
//! on every tick, because its resolve notifies on every tick) and is gone by tick 2 of a round that
//! goes quiet, so both halves of the guarantee hold.
//!
//! ## Half two: NaN is the one value whose mark never decays
//!
//! `AbsDiff::are_different` for floats is `self != rhs` (`crates/vitality_core/src/math.rs`, D-34), with
//! no epsilon, so it is false for equal values and **true** for any pair with a NaN operand — including
//! `NaN` against itself. A resolve that evaluates to NaN therefore always reads as "changed"
//! (`vitality_core/src/attributes.rs:377-389`), which means three things, all probed on this shape (one
//! source actor, one reader actor, one permanent `EffectSubject::Source` modifier so the reader's
//! `NanDep` value comes from the source's `NanAttr`):
//!
//! * **the mark is re-raised forever.** A marked actor with a NaN-valued resolve is told again on every
//!   tick of the round, while the *finite* attribute of the same actor, marked by the same call, is told
//!   once and then left alone: the finite resolve yields the value it already holds, so it writes nothing,
//!   notifies nothing and re-marks nothing. Since L-4 neither half is readable as a `Dirty<T>` at a tick
//!   boundary — the tick that raises a mark resolves it — so this pair is read off [`MarkFires`], the
//!   file's `MarkNodeDirty` counters: measured on the round below, the source's `NanAttr` earns one mark
//!   on every tick of `NAN_HORIZON` and its `NanDep` earns none. NaN is the one attribute here that never
//!   stops asking;
//! * **the write lands.** The base goes to NaN and `current_value` follows it from tick 1, and a fresh
//!   `MarkNodeDirty` re-writes it. NaN can still reach `current_value` unguarded in two other ways —
//!   `set_current_value` is public and `attribute_impl!`'s `new()`
//!   (`crates/vitality_core/src/attributes.rs:242-250`) writes base *and* current with no comparison at
//!   all — but neither one is sticky any more: the next resolve compares against it and counts it as a
//!   change;
//! * **the cascade rides it.** `should_notify_observers` is true, so `CurrentValueChanged` and
//!   `RecalculateExpression` fire off a NaN resolve: the reader is marked with `MarkNodeDirty<NanDep>`,
//!   reads NaN from tick 2 and is told again on every tick after (one fire on tick 1, none on tick 2 —
//!   the tick that resolves the mark it already carried, while the node marker still short-circuits the
//!   propagation — two on every tick from tick 3). The round just before the NaN one, on the same edge,
//!   is the file's positive control: one `+3` write to the source moved the reader's value by 3 inside
//!   three ticks and put a `Dirty<NanDep>` on the reader's effect node — on its first tick, which is when
//!   the node marker is legal.
//!
//! Read the two NaN doors apart, because they send NaN to opposite places. When the **resolve evaluates
//! to NaN** (the base is NaN, so `NanAttr::src()`-style expressions fold to NaN), the NaN is written
//! into `current_value` and rides the edge: the reader's `NanDep` reads NaN from tick 2 and keeps being
//! re-marked. When a **NaN merely sits in `current_value`** (the public `set_current_value`, which is
//! [`a_nan_left_in_current_value_leaves_its_dependent_dirty_and_stale`]) the finite base is still in the
//! other slot, so the first resolve writes the base back over it, notifies once, and the dependent is
//! marked on tick 1, clean from tick 2, and never sees the NaN in its own value at all. Before D-34 both
//! doors were the other way round — the evaluating NaN was swallowed and its dependents left never
//! marked and permanently stale, the sitting NaN was never overwritten and its dependent left marked
//! forever. Nothing panicked and nothing wedged in either probe: every read reaches every tick it was
//! asked for.
//!
//! ## What D-34 changed in this file
//!
//! `.auto/decisions.md` D-34 (approved, D-38) made `are_different` `!=` with **no** epsilon, so NaN
//! counts as changed. Thirteen asserts went red and every one of them was inverted in place, in the
//! three groups below; nothing else in the file went red — not the node guarantee of half one, not the
//! split-vs-count agreement, not either `prepare_floor` control, not the two live-edge controls, not the
//! three asserts that only hold while `mark_dirty_fraction` flushes, not the settled read between the two
//! rounds of the NaN test, and not the frozen dependent value of the third test. (The reset between those
//! two rounds deliberately reports the marker count instead of asserting it: the finite round leaves
//! nothing dirty at all, so `prepare_floor` there removes 0 and that is not a failure.)
//!
//! * *the marked actors are still dirty at the horizon* — 3 asserts, all in half one: the actor half
//!   read two ticks after the mark, the actor count at tick 22 and the world count at tick 22. Measured
//!   here now: the two marked actors are clean on **tick 1** and the walk reports 0 dirty entities from
//!   there on, while the composition the test is named for is still pinned at the mark itself;
//! * *the NaN write is swallowed* — 3 asserts, each one a `current_value`: the source's in the base
//!   round, its twin after the bare mark, and the third test's NaN sitting in `current_value`. The first
//!   two read NaN from tick 1; the third reads the finite base back, written over the injected NaN by
//!   the first resolve;
//! * *the cascade stops at the NaN* — 3 asserts, each one the reader: its value in the base round, its
//!   marks in the same round, and its marks after the bare mark. The reader reads NaN from tick 2; what
//!   those two marks-reads asserted moved with L-4, below;
//! * *the NaN mark decays and nothing else decays* — 4 asserts: the `NanAttr`-absent one and the
//!   `NanDep`-present control beside it (base round, tick >= 2), the bare-mark `marks` read (which
//!   asserts both halves of that pair in one), and the third test's "the dependent stays dirty". The pair
//!   is exactly inverted, and L-4 moved where it is readable: at a tick boundary neither half carries a
//!   marker any more, and the inversion lives in the marks each one raises.
//!
//! The last group retires the phrase "a NaN mark then lives as long as a finite one's", which the first
//! pass of this file used and which was wrong even then: `NaN != NaN` is true, so a NaN resolve reports
//! a change every tick it runs. A NaN keeps asking and a finite attribute stops; before L-4 that
//! difference was readable as markers that never left versus a marker gone by tick 1, and since L-4 both
//! boundaries read empty and only the re-mark separates them. That permanently-re-marking world is what
//! D-34 hands to a
//! dependent of an attribute whose **resolve evaluates to** NaN, and to no other *NaN* shape: where the
//! NaN only sits in `current_value`, the third test below falsifies the broad claim — the first resolve
//! writes the finite base over it and that dependent is clean from tick 2. NaN is not the only way to
//! be permanently dirty either: an attribute whose resolve oscillates between two finite values differs
//! from its current value on every tick too, and keeps re-marking a finite-valued dependent forever —
//! no test in this file builds that shape. The assert beside the third test's flip keeps its "the
//! dependent never reads NaN" wording on purpose: here the NaN is erased before the edge can carry it,
//! which is the difference the two paragraphs above are about.
//!
//! ## What L-4 changed in this file
//!
//! L-4 moved the mark/notify pair ahead of the resolve (`vitality_effects/src/lib.rs`,
//! `EffectsSet::UpdateBaseValues`), so a tick raises a `Dirty<T>` and clears it again before its own
//! boundary. Every assert that went red was a boundary read of a marker this file used to see: the NaN
//! round's "source carries `NanAttr`" / "reader carries `NanDep`" pair and their bare-mark twins (four),
//! plus `nan_world`'s `prepare_floor` control, which is `floor.rs`'s story rather than this file's. What
//! held unchanged is the point: the value pins, the node-marker guarantee of half one and the third
//! test's "told once, then quiet". The reorder moves *when a mark is readable*, not what the cascade
//! does. [`MarkFires`] is how the file keeps its teeth: counting `MarkNodeDirty` triggers says whether an
//! attribute is still being told, which an empty boundary read no longer can.
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
use std::collections::HashMap;
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

/// Long enough to show a NaN mark keeps being raised and a finite one stops, with room to spare. Since
/// L-4 the round's marks are gone by every boundary read, so this horizon is read through [`MarkFires`]
/// rather than as "the tick the mark is measured to be gone on", which is what it named pre-L-4. The
/// stale-dependent test reads the same number of ticks.
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
         effect entity after the walk cleaned it (`vitality_effects/src/modifier.rs:248-283`); the same \
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

/// How many `MarkNodeDirty` triggers each half of the NaN pair has landed on each actor since the app
/// started.
///
/// Since L-4 the mark phase runs ahead of the resolve, so a tick both raises a mark and clears it: a read
/// at a tick boundary is empty even on an attribute that re-marks itself forever. The trigger is what
/// still shows which half of a pair keeps asking. Per entity, because one global count cannot tell the
/// source's finite `NanDep` (marked once, never again) from the reader's NaN one (marked every tick).
#[derive(Resource, Default)]
struct MarkFires {
    attr: HashMap<Entity, u32>,
    dep: HashMap<Entity, u32>,
}

impl MarkFires {
    /// How many marks have landed on `entity` for one half, over the whole run of the app.
    fn total(&self, attr: bool, entity: Entity) -> u32 {
        let map = if attr { &self.attr } else { &self.dep };
        map.get(&entity).copied().unwrap_or_default()
    }

    /// The four reads the rounds below compare across one tick: marks raised on the source's `NanAttr`,
    /// its `NanDep`, the reader's `NanAttr` and the reader's `NanDep`, in that order.
    fn snapshot(&self, source: Entity, reader: Entity) -> [u32; 4] {
        [
            self.total(true, source),
            self.total(false, source),
            self.total(true, reader),
            self.total(false, reader),
        ]
    }
}

fn count_nan_attr_marks(trigger: On<MarkNodeDirty<NanAttr>>, mut fires: ResMut<MarkFires>) {
    *fires.attr.entry(trigger.entity).or_default() += 1;
}

fn count_nan_dep_marks(trigger: On<MarkNodeDirty<NanDep>>, mut fires: ResMut<MarkFires>) {
    *fires.dep.entry(trigger.entity).or_default() += 1;
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
    app.init_resource::<MarkFires>();
    app.add_observer(count_nan_attr_marks);
    app.add_observer(count_nan_dep_marks);
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
    report("at the mark (flushed)", world, pool, &actors);
    let marked_actors = actors_with_marks(world, pool, &actors);
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
        report(&format!("tick {number}"), world, pool, &actors);
        assert_node_markers_are_being_cleaned(
            world,
            &mut previous,
            &format!("{number} ticks after the mark"),
        );
    }
    let world = app.world();
    assert!(
        dirty_split(world).actors.is_empty(),
        "B-4c the marks did not decay: two ticks after the mark the walk's actor half is {:?} and not \
         empty. Since D-34 (`!=` at `math.rs:33`) a bare mark resolves once, writes nothing because the \
         value it yields is the value it already held, and the mark is gone at **tick 1** — measured on \
         this shape. Keep the read: an actor half that never empties means the re-dirty storm is back, \
         and the two asserts above still pin that the marks at the mark itself are all actors and none \
         of them nodes",
        dirty_split(world).actors,
    );

    for number in 3..=HORIZON {
        tick(&mut app);
        let world = app.world();
        assert_node_markers_are_being_cleaned(world, &mut previous, &format!("tick {number}"));
    }

    let world = app.world();
    report(&format!("tick {HORIZON}"), world, pool, &actors);
    // The number B-4 printed and nobody asserted, now pinned as a composition: all of it actors, none
    // of it the shape's 8 effect entities or their modifiers.
    let split = dirty_split(world);
    assert_split_matches_count(world, &split, &format!("tick {HORIZON}"));
    assert!(
        split.actors.is_empty() && split.nodes.is_empty(),
        "the count B-4 quoted as 2 forever is {} actors and {} nodes at tick {HORIZON}, and a settled \
         world should report neither: a bare mark drains at tick 1 and nothing re-raises it. The \
         composition this test is named for is still pinned at the mark itself ({MARKED} actors, 0 \
         nodes), which is the half that does not move with D-34",
        split.actors.len(),
        split.nodes.len(),
    );
    assert_eq!(
        dirty_entity_count(world),
        0,
        "at tick {HORIZON} the world walk reports {} dirty entities and not 0; its actor half is {:?} \
         and its node half {:?}. Before D-34 this number was {MARKED} and it never fell",
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
        dirty_names(world, pool, source),
        ["NanAttr", "NanDep"],
        "marking the source through `mark_dirty_fraction` left it carrying {:?}; with the helper's \
         `world.flush()` gone those inserts are still queued commands and this file's whole NaN half \
         reads a world nothing was marked on",
        dirty_names(world, pool, source),
    );
    let reader_at_nan = dep.current(world, reader).unwrap();
    assert!(
        dirty_names(world, pool, reader).is_empty(),
        "the reader {reader:?} carries {:?} the moment the NaN is written, before a single tick has \
         resolved anything",
        dirty_names(world, pool, reader),
    );
    assert_eq!(
        reader_at_nan, reader_moved,
        "the reader's NanDep reads {reader_at_nan} at the NaN write but {reader_moved} at the end of \
         the finite round, so the world the NaN round starts on is not the settled one the assert above \
         said it was",
    );

    for number in 1..=NAN_HORIZON {
        let before = app.world().resource::<MarkFires>().snapshot(source, reader);
        tick(&mut app);
        let after = app.world().resource::<MarkFires>().snapshot(source, reader);
        let attr_source = after[0] - before[0];
        let dep_source = after[1] - before[1];
        let attr_reader = after[2] - before[2];
        let dep_reader = after[3] - before[3];
        let world = app.world();
        let marks = dirty_names(world, pool, source);
        println!(
            "B-4c nan tick {number}: source marks {marks:?} NanAttr base={} current={} \
             (frozen {frozen}); reader marks {:?} NanDep current={}; marks raised src-attr={attr_source} \
             src-dep={dep_source} reader-attr={attr_reader} reader-dep={dep_reader}",
            nan.base(world, source).unwrap(),
            nan.current(world, source).unwrap(),
            dirty_names(world, pool, reader),
            dep.current(world, reader).unwrap(),
        );
        assert_node_markers_are_being_cleaned(
            world,
            &mut previous,
            &format!("the NaN round, tick {number}"),
        );

        let current = nan.current(world, source).unwrap();
        assert!(
            current.is_nan(),
            "B-4c swallowed write: at tick {number} of the NaN round the source's NanAttr reads \
             {current} and not the NaN its base holds (it was {frozen} before the round). Since D-34 \
             the float arm of `are_different` is `self != rhs`, so a NaN resolve counts as changed and \
             `current_value` follows the base from tick 1 — a NaN that silently stops updating is a bug \
             either way, so this stays red if the write is ever swallowed again",
        );

        let reader_now = dep.current(world, reader).unwrap();
        if number == 1 {
            assert_eq!(
                reader_now, reader_at_nan,
                "B-4c stopped cascade: the reader's NanDep already reads {reader_now} on tick 1 of the \
                 NaN round, before the mark from the NaN resolve has had a tick to arrive at \
                 {reader_at_nan}",
            );
        } else {
            assert!(
                reader_now.is_nan(),
                "B-4c stopped cascade: at tick {number} of the NaN round the reader's NanDep still \
                 reads {reader_now} instead of the NaN its source now carries. Since D-34 a NaN resolve \
                 notifies, so `RecalculateExpression` reaches this reader, the NaN rides \
                 `NanAttr::src()` into the fold and the derived value goes NaN on the tick after the \
                 mark lands; a dependent that quietly absorbs a NaN is what the fix is meant to make \
                 loud",
            );
        }

        if number >= 2 {
            // The pair this file used to read at a boundary — `NanAttr` present on the NaN half, absent
            // on the finite half — is now absent on both: since L-4 the tick that raises a mark resolves
            // it. What still separates the two halves is what gets raised next, so that is what is pinned.
            assert!(
                marks.is_empty(),
                "B-4c a marker outlived its tick: at tick {number} the source reads {marks:?} at the \
                 boundary. Since L-4 the re-mark is raised in `EffectsSet::UpdateBaseValues` and removed \
                 by the resolve later in that same tick, so nothing should be left here; a survivor is \
                 either the pre-L-4 ordering back or a mark this resolve could not clear",
            );
            assert!(
                attr_source > 0,
                "B-4c the NaN mark stopped outliving the finite one: tick {number} of the NaN round \
                 raised no `MarkNodeDirty<NanAttr>` on the source. `NaN != NaN` is true, so the NaN \
                 resolve writes, notifies and re-marks itself every tick — the NaN is the one attribute \
                 on this world that never stops asking, which is the flip side of D-34: a NaN attribute \
                 never leaves the resolve set, whatever the ordering around it is",
            );
            assert!(
                dep_source == 0,
                "the control broke: tick {number} of the NaN round raised {dep_source} more \
                 `MarkNodeDirty<NanDep>` on the source. Its NanDep is finite, so its resolve yields the \
                 value it already holds, writes nothing and re-marks nothing — if it is being told again, \
                 the re-dirty storm is back and the NaN assert above shows nothing about NaN either",
            );
            // Tick 2 is the one tick the reader is not told: it is the tick that resolves the mark the
            // previous resolve left on it, and the cascade's `MarkNodeDirty` lands on the still-dirty
            // effect node, whose observer stops the propagation (`systems.rs:36-39`). From the next tick
            // the node marker is gone and the edge fires again on every tick of the round.
            assert!(
                number < 3 || dep_reader > 0,
                "the cascade stopped at the edge: tick {number} of the NaN round raised {dep_reader} \
                 `MarkNodeDirty<NanDep>` on the reader {reader:?} while it reads {} and re-marks itself \
                 on every tick. Since D-34 a NaN resolve notifies; a reader that stops being told while \
                 its value stays NaN is a stale dependent",
                dep.current(world, reader).unwrap(),
            );
        }
    }

    // A bare mark, with no write behind it, so the decay above cannot be blamed on the round's base
    // write (which re-marks the actor through `Changed<T>` and buys the mark one extra tick).
    let before = app.world().resource::<MarkFires>().snapshot(source, reader);
    nan.mark_dirty(app.world_mut(), source);
    app.world_mut().flush();
    tick(&mut app);
    let after = app.world().resource::<MarkFires>().snapshot(source, reader);
    let attr_source = after[0] - before[0];
    let dep_reader = after[3] - before[3];
    let world = app.world();
    let marks = dirty_names(world, pool, source);
    let reader_marks = dirty_names(world, pool, reader);
    println!(
        "B-4c nan bare mark: source marks {marks:?} reader marks {reader_marks:?}; marks raised \
         src-attr={attr_source} reader-dep={dep_reader}"
    );
    assert_node_markers_are_being_cleaned(world, &mut previous, "after the bare NaN mark");
    assert!(
        marks.is_empty(),
        "B-4c the bare mark left a marker behind: one tick after a bare `MarkNodeDirty<NanAttr>` the \
         source reads {marks:?}. Since L-4 the tick that raises a mark resolves it, so what a boundary \
         read finds is nothing on both halves of the pair — pre-L-4 this read `[\"NanAttr\"]`, because \
         the notify behind the resolve re-raised the mark after the walk had cleaned it. A marker here \
         now is one the NaN resolve could not clear",
    );
    assert!(
        attr_source > 0,
        "B-4c the bare mark did not keep the NaN storm alive: no `MarkNodeDirty<NanAttr>` fired across \
         a tick that started from a hand mark on the NaN attribute. `NaN != NaN` is true, so the resolve \
         writes, notifies and earns another mark — that permanent re-mark is the flip side of D-34, and \
         no bench id prices it: `recompute/*` marks actors through `mark_dirty_fraction` on finite \
         attributes, never through a NaN resolve. Pre-L-4 this was pinned as a marker that survives the \
         tick; since L-4 the marker is gone by the boundary and only the mark being raised again is \
         visible",
    );
    assert!(
        nan.current(world, source).unwrap().is_nan()
            && dep.current(world, reader).unwrap().is_nan(),
        "the bare NaN mark did not reach the value it drives ({} / {}): since D-34 a bare mark on the \
         NaN attribute resolves, writes, notifies and leaves both this current value and the reader's \
         NanDep on NaN — keep the pair, because a value that stops updating in silence is the bug on \
         both sides of the fix",
        nan.current(world, source).unwrap(),
        dep.current(world, reader).unwrap(),
    );
    assert!(
        dep_reader > 0,
        "a bare mark on the NaN attribute fired no `MarkNodeDirty<NanDep>`: since D-34 a NaN resolve \
         notifies, so the cascade has to start from a NaN even when nothing wrote a finite value",
    );
    assert!(
        reader_marks.is_empty(),
        "a bare mark on the NaN attribute left the reader {reader_marks:?} at the tick boundary: the \
         mark crosses the edge and the tick that raises it resolves it (L-4), so what is readable here \
         is the reader's NaN value and no marker",
    );
}

/// The other door NaN has into a resolve: `set_current_value` is public and `attribute_impl!`'s
/// `new()` writes base *and* current unguarded, so a NaN can sit in a current value without ever
/// having been evaluated there. `NanAttr::src()` reads `current_value`, so this is the shape where a
/// dependent *is* being fed a NaN — but only for the tick that erases it: the first resolve writes the
/// finite base over it, so this is *not* the permanently-dirty state the module docs separate the
/// evaluating-to-NaN shape into.
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
        let source_marks = dirty_names(world, pool, source);
        let reader_marks = dirty_names(world, pool, reader);
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
            !nan.current(world, source).unwrap().is_nan(),
            "a NaN that is already in `current_value` is still there at tick {number} ({}): since D-34 \
             `are_different(NaN, base)` is `NaN != base`, which is true, so the first resolve writes the \
             finite base back over it and the dependent downstream is told about it",
            nan.current(world, source).unwrap(),
        );
        if number == 1 {
            assert!(
                reader_marks.contains(&"NanDep"),
                "B-4c the stale dependent never noticed the NaN: on tick 1 the reader {reader:?} reads \
                 {reader_marks:?}. Since D-34 the resolve that writes the finite base over the injected \
                 NaN notifies, so the mark reaches this reader on the very next tick",
            );
        } else {
            assert!(
                !reader_marks.contains(&"NanDep"),
                "B-4c the stale dependent stayed dirty: at tick {number} the reader {reader:?} still \
                 reads {reader_marks:?}. It was told about the NaN once, recomputed to the base again, \
                 and a resolve that yields the value it already holds re-marks nothing — a dependent \
                 that is permanently dirty *and* permanently stale was the worst state the old \
                 `are_different` could leave, and it must not come back",
            );
        }
        assert!(
            reader_now == reader_at_inject && reader_now.is_finite(),
            "the dependent recomputed itself ({reader_now}, was {reader_at_inject}) even though the NaN \
             sat in its source's `current_value` only for the tick that wrote the finite base back over \
             it. The edge never carries the NaN here — `NanAttr::src()` reads `current_value` after the \
             resolve has cleaned it — so this value is the same finite fold on both sides of D-34, and \
             what changed is that the dependent is *told* (the mark above) instead of being left stale \
             and quiet. If this goes red, a NaN reached a dependent's fold: read it together with the \
             NaN round of the test above, where the base itself is NaN and the reader does read NaN",
        );
    }
}
