//! D-34: the one thing the three dirty files leave to the flip list — that a resolve which yields
//! *the same value* is not a change, so it must not re-mark its actor and must not fan out to a
//! dependent.
//!
//! `AbsDiff::are_different` for floats (`crates/vitality_core/src/math.rs:33-35`) is `self != rhs`, the
//! approved D-34 fix (`.auto/decisions.md` D-34, answered by D-38): a resolve that yields the value it
//! already holds reports no change, and a NaN resolve reports one because `NaN != NaN`. The formula it
//! replaced — `self.abs_diff(rhs) > Self::Output::MIN`, where `MIN` is `f32::MIN` = `-3.4028235e38`, the
//! most negative finite float and not `MIN_POSITIVE` — was true for every pair of floats that carried no
//! NaN operand, so a pair of equal values counted as a change and nothing finite ever resolved quietly.
//! The one exception was a NaN operand, which read as unchanged: that is what `test_dirty_nan.rs` is
//! built on.
//!
//! Two things ride on that one bool out of `update_attribute`
//! (`crates/vitality_core/src/attributes.rs:322-331`, read at
//! `crates/vitality_effects/src/systems.rs:196-226`):
//!
//! * the write-back to `current_value`, which `on_change_notify_attribute_parents::<T>`
//!   (`crates/vitality_effects/src/systems.rs:274-283`) reads as `Changed<T>` and answers with a fresh
//!   `MarkNodeDirty<T>` — the actor's own mark, raised again by any resolve that really did move the
//!   value, which since D-34 is the only kind of write-back there is;
//! * `CurrentValueChanged<T>` plus one `RecalculateExpression` per entry of `AttributeDependents<T>`,
//!   and `update_modifier_when_dependencies_changed`
//!   (`crates/vitality_effects/src/modifier.rs:203-228`) answers *every* one of those with
//!   `MarkNodeDirty` on the effect node — unconditionally, whatever its expression re-evaluates to —
//!   which `mark_node_dirty_observer` (`vitality_effects/src/systems.rs:31-42`) then walks up
//!   `EffectTarget` onto the reader actor.
//!
//! So the shape this file needs is a real dependency edge, not a bare actor: [`cross_actor`]'s source
//! plus one reader with one `EffectSubject::Source` modifier. A bare-actor shape cannot tell "the
//! fan-out stopped" from "there was never anything to fan out to", and it is the reader's marks and the
//! trigger counts that say which of the two the world did.
//!
//! Both halves are pinned, and the second one is the reason the first means anything:
//!
//! * a bare `MarkNodeDirty<Attr0>` on the source — no write behind it, so its resolve recomputes the
//!   value it already has — leaves the whole world clean one tick later: no mark on the source, none on
//!   the reader, none on the effect node, and no `CurrentValueChanged<Attr0>` or `RecalculateExpression`
//!   fired. Green on this tree, and every one of those reads is a pin against the old formula coming
//!   back;
//! * a write that *does* move the source still crosses the edge: the reader picks up `Dirty<Attr4>` and
//!   its `Attr4` moves by [`cross_actor::CROSS_STEP`]. Green on both sides of the fix, and without it
//!   the clean read above would only say the cascade is broken.
//!
//! The steady-state half of B-4/B-4b/B-4c (`test_dirty_lifetime.rs`, `test_dirty_spread.rs`,
//! `test_dirty_nan.rs`) is the flip list; this file is the pin that says *why* those marks decay.

#[path = "../benches/support/common.rs"]
mod common;
#[path = "../benches/support/cross_actor.rs"]
mod cross_actor;
#[path = "../benches/support/floor.rs"]
mod floor;
#[path = "../benches/support/identity.rs"]
mod identity;
#[path = "../benches/support/recompute.rs"]
mod recompute;

use crate::common::{Attr0, Attr4, AttrSpec, HarnessWorld, current_values, run_fixed_tick};
use crate::cross_actor::{CROSS_STEP, cross_shape, cross_spec, move_source};
use crate::floor::{dirty_entity_count, dirty_split, prepare_floor};
use crate::recompute::dirty_names;
use bevy::prelude::*;
use vitality::CurrentValueChanged;
use vitality::effect::modifier::RecalculateExpression;

/// The §2B gate shape, for the same reason `test_dirty_spread.rs` uses it: five attributes keeps the
/// source attribute (`Attr0`) and the dependent one (`Attr4`) different types, and one reader is the
/// smallest cascade that crosses an actor at all.
const ATTRIBUTES: usize = 5;
const READERS: usize = 1;

const SOURCE_ATTR: &str = stringify!(Attr0);
const DEPENDENT_ATTR: &str = stringify!(Attr4);

/// Beyond the tick the fan-out arrives on: one more tick says the clean read is a settled world rather
/// than a cascade caught between two of its own marks.
const SETTLE_TICKS: usize = 2;

/// Fan-out triggers, counted by the two observers [`count_fanout`] registers. Counted because marks
/// alone cannot say whether the resolve decided not to notify or notified and the mark got lost on the
/// way.
#[derive(Resource, Default, Clone, Copy)]
struct Fanout {
    current_value_changed: usize,
    recalculate_expression: usize,
}

impl Fanout {
    fn total(&self) -> usize {
        self.current_value_changed + self.recalculate_expression
    }
}

/// Counts the two triggers `update_attribute` fires when it decides a value changed. bevy dispatches an
/// observer the moment it is spawned, so no schedule has to run first and the [`run_fixed_tick`]-only
/// ticks below are enough to see them.
fn count_fanout(app: &mut App) {
    app.insert_resource(Fanout::default());
    app.add_observer(
        |_: On<CurrentValueChanged<Attr0>>, mut counts: ResMut<Fanout>| {
            counts.current_value_changed += 1;
        },
    );
    app.add_observer(|_: On<RecalculateExpression>, mut counts: ResMut<Fanout>| {
        counts.recalculate_expression += 1;
    });
}

fn read(app: &App) -> Fanout {
    *app.world().resource::<Fanout>()
}

fn reset(app: &mut App) {
    *app.world_mut().resource_mut::<Fanout>() = Fanout::default();
}

/// The same world read three ways, plus the two trigger counters, so a red line names the read that
/// disagreed and the print shows what the resolve fired even when the marks already told the story.
fn report(
    label: &str,
    world: &World,
    pool: &[AttrSpec],
    source: Entity,
    reader: Entity,
    counts: Fanout,
) {
    let split = dirty_split(world);
    println!(
        "D-34 {label}: source {SOURCE_ATTR} marks {:?}, reader {DEPENDENT_ATTR} marks {:?}; world \
         walk sees {} dirty entities ({} actors, {} nodes {:?}); triggers \
         CurrentValueChanged={}/RecalculateExpression={}",
        dirty_names(world, pool, source),
        dirty_names(world, pool, reader),
        dirty_entity_count(world),
        split.actors.len(),
        split.nodes.len(),
        split.node_labels(),
        counts.current_value_changed,
        counts.recalculate_expression,
    );
}

#[test]
fn a_resolve_that_yields_the_same_value_fans_out_nothing() {
    let spec = cross_spec(ATTRIBUTES, READERS);
    let mut shape = cross_shape::<Attr0, Attr4>(&spec, SOURCE_ATTR, DEPENDENT_ATTR);
    let pool: &'static [AttrSpec] = shape.app.world().resource::<HarnessWorld>().attrs();
    assert_eq!(
        pool.len(),
        ATTRIBUTES,
        "shape {} registered {} attributes, not the {} its id names",
        spec.shape_id(),
        pool.len(),
        ATTRIBUTES,
    );
    assert_eq!(
        pool[0].name, SOURCE_ATTR,
        "slot 0 of the pool is {}, so the mark below would not be the source attribute the edge reads",
        pool[0].name,
    );
    assert_eq!(
        shape.readers.len(),
        READERS,
        "shape {} names {} readers and the world carries {}",
        spec.shape_id(),
        READERS,
        shape.readers.len(),
    );
    let source = shape.source;
    let reader = shape.readers[0];
    assert_ne!(
        source, reader,
        "the source and the reader are one actor, so nothing below ever crossed an actor",
    );
    assert!(
        shape.markers_cleared > 0,
        "the shape was never dirty, so `dirty_entity_count` returning 0 below is a walk that never \
         saw a marker rather than a settled world",
    );

    count_fanout(&mut shape.app);
    // `cross_shape` settles itself; this re-reads it here so the zero below belongs to the world this
    // file is about to mark, and `prepare_floor` keeps its own assert that an idle tick stays idle.
    let cleared = prepare_floor(&mut shape.app);
    let world = shape.app.world();
    report("settled", world, pool, source, reader, read(&shape.app));
    assert_eq!(
        dirty_entity_count(world),
        0,
        "shape {} is not clean ({} markers cleared, {} dirty entities left), so no tick below can \
         attribute a mark to this file's own call",
        spec.shape_id(),
        cleared,
        dirty_entity_count(world),
    );
    assert_eq!(
        read(&shape.app).total(),
        0,
        "a settled shape fired {} fan-out trigger(s) before anything was marked",
        read(&shape.app).total(),
    );

    // Half one: one mark, no write, so the resolve recomputes exactly what the attribute already holds.
    let before = current_values(shape.app.world(), pool, source);
    let reader_before = current_values(shape.app.world(), pool, reader);
    pool[0].mark_dirty(shape.app.world_mut(), source);
    shape.app.world_mut().flush();
    let world = shape.app.world();
    report(
        "at the bare mark",
        world,
        pool,
        source,
        reader,
        read(&shape.app),
    );
    assert_eq!(
        dirty_names(world, pool, source),
        [SOURCE_ATTR],
        "this file marked {SOURCE_ATTR} of {source:?} and that actor now reads {:?}: with no mark of \
         its own the source has nothing to resolve, and the clean reads below would prove nothing",
        dirty_names(world, pool, source),
    );
    assert!(
        dirty_names(world, pool, reader).is_empty(),
        "the reader {reader:?} reads {:?} before a single tick has run, so whatever it carries later \
         was put there by the build",
        dirty_names(world, pool, reader),
    );
    reset(&mut shape.app);

    for number in 1..=SETTLE_TICKS {
        run_fixed_tick(shape.app.world_mut());
        let world = shape.app.world();
        report(
            &format!("no-op resolve, tick {number}"),
            world,
            pool,
            source,
            reader,
            read(&shape.app),
        );

        let source_marks = dirty_names(world, pool, source);
        assert!(
            source_marks.is_empty(),
            "D-34 self re-mark: {number} tick(s) after a bare mark on {SOURCE_ATTR} of {source:?} \
             that actor still reads {source_marks:?}. Its resolve recomputed the value it already had, \
             so `are_different` — `self != rhs` (`crates/vitality_core/src/math.rs:33-35`) — reports no \
             change and `set_current_value` never runs; a mark here means that bool is back to \
             `abs_diff > f32::MIN`, which every finite pair satisfied — equal ones included — so \
             `update_attribute` writes `current_value` back, `on_change_notify_attribute_parents` reads \
             the write as `Changed<T>`, and the actor is marked again by the resolve that cleared it",
        );
        let reader_marks = dirty_names(world, pool, reader);
        assert!(
            reader_marks.is_empty(),
            "D-34 fan-out: {number} tick(s) after a bare mark on {SOURCE_ATTR} of {source:?} the \
             reader {reader:?} reads {reader_marks:?}. Nothing about the value the reader derives \
             from {source:?} changed, so no `RecalculateExpression` should have reached its modifier \
             and no `MarkNodeDirty<{DEPENDENT_ATTR}>` should have followed it up `EffectTarget`",
        );
        let split = dirty_split(world);
        assert!(
            split.nodes.is_empty(),
            "D-34 fan-out left {} node marker(s) — {:?} — on a world whose only mark was an \
             attribute-preserving one: `update_modifier_when_dependencies_changed` marks the effect \
             entity for every `RecalculateExpression` it is sent, whatever its expression re-evaluates \
             to, so a node marker here means the fan-out ran",
            split.nodes.len(),
            split.node_labels(),
        );
        assert_eq!(
            split.total(),
            0,
            "the world walk sees {} dirty entity/entities after a resolve that changed no value \
             (actors {:?}, nodes {:?})",
            split.total(),
            split.actors,
            split.node_labels(),
        );

        let counts = read(&shape.app);
        assert_eq!(
            counts.current_value_changed, 0,
            "the no-op resolve fired {} `CurrentValueChanged<{SOURCE_ATTR}>` trigger(s): \
             `update_attribute` only notifies when `are_different` says the value moved",
            counts.current_value_changed,
        );
        assert_eq!(
            counts.recalculate_expression, 0,
            "the no-op resolve fired {} `RecalculateExpression` trigger(s), one per entry of \
             `AttributeDependents<{SOURCE_ATTR}>`; that walk is the fan-out, and each trigger ends in \
             a `MarkNodeDirty` on an effect node",
            counts.recalculate_expression,
        );

        assert_eq!(
            current_values(world, pool, source),
            before,
            "the no-op resolve moved {source:?}'s values from {before:?} to \
             {:?}: nothing was written to the base, so a value that moved is a resolve that changed \
             the answer between two identical runs",
            current_values(world, pool, source),
        );
        assert_eq!(
            current_values(world, pool, reader),
            reader_before,
            "the no-op resolve moved {reader:?}'s values from {reader_before:?} to \
             {:?}: the reader's derived attribute is the thing the fan-out writes, and it should not \
             have been touched",
            current_values(world, pool, reader),
        );
    }

    // Half two: the same edge, live. Without this, every clean read above is also what a shape whose
    // cascade never worked reports, and the fix could not be told from a broken resolver.
    reset(&mut shape.app);
    let moved = move_source::<Attr0>(shape.app.world_mut(), source);
    pool[0].mark_dirty(shape.app.world_mut(), source);
    shape.app.world_mut().flush();
    run_fixed_tick(shape.app.world_mut());
    let world = shape.app.world();
    report(
        "after a real write",
        world,
        pool,
        source,
        reader,
        read(&shape.app),
    );
    let counts = read(&shape.app);
    assert!(
        counts.current_value_changed >= 1,
        "a write that moved {SOURCE_ATTR} to {moved} fired no `CurrentValueChanged<{SOURCE_ATTR}>`: \
         the edge is dead, so the clean reads above were measuring nothing",
    );
    assert!(
        counts.recalculate_expression >= 1,
        "a write that moved {SOURCE_ATTR} to {moved} fired no `RecalculateExpression`, so \
         `AttributeDependents<{SOURCE_ATTR}>` never reached the reader's modifier and the shape is not \
         the cascade the clean reads above say it is",
    );

    let mut crossed = dirty_names(world, pool, reader);
    for _ in 1..=SETTLE_TICKS {
        if crossed.contains(&DEPENDENT_ATTR) {
            break;
        }
        run_fixed_tick(shape.app.world_mut());
        crossed = dirty_names(shape.app.world(), pool, reader);
    }
    assert!(
        crossed.contains(&DEPENDENT_ATTR),
        "the reader {reader:?} never carried `Dirty<{DEPENDENT_ATTR}>` after a real {SOURCE_ATTR} \
         move to {moved}; it reads {crossed:?}",
    );

    for _ in 0..4 {
        let settled = current_values(shape.app.world(), pool, reader);
        if (settled[ATTRIBUTES - 1] - (reader_before[ATTRIBUTES - 1] + CROSS_STEP as f64)).abs()
            < 1e-3
        {
            break;
        }
        run_fixed_tick(shape.app.world_mut());
    }
    let settled = current_values(shape.app.world(), pool, reader);
    assert!(
        (settled[ATTRIBUTES - 1] - (reader_before[ATTRIBUTES - 1] + CROSS_STEP as f64)).abs()
            < 1e-3,
        "the reader's {DEPENDENT_ATTR} read {} four ticks after a +{CROSS_STEP} move of \
         {SOURCE_ATTR} (was {}): the cascade crossed but never landed, so the live-edge control says \
         nothing about the no-op half above",
        settled[ATTRIBUTES - 1],
        reader_before[ATTRIBUTES - 1],
    );
}
