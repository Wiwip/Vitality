//! P-15 (BENCH-PLAN §2B): the cross-actor cascade `recompute/cross_actor` times.
//!
//! One source actor, K reader actors, and one permanent effect per reader whose single modifier is
//! `EffectSubject::Source` and reads the *source's* attribute — applied with the same
//! `ApplyEffectEvent { entity: target, targeting: EffectTargeting::new(source, target), handle }`
//! that `Vitality::apply_effect_to_target` triggers (`crates/vitality/src/vitality.rs:41-52`). So the
//! workload is the cascade, not a fold: the source's attribute changes, `update_attribute` walks its
//! `AttributeDependents` (`vitality_effects/src/systems.rs:201-226`), each dependent modifier
//! recalculates (`vitality_effects/src/modifier.rs:203-229`) and re-marks its effect node, and the
//! mark propagates up `EffectTarget` to the reader, which resolves on a later tick.
//!
//! The three things this pins, in the order the bench could silently break them:
//!
//! * the cascade is really *cross-actor* — the effect's `EffectSource` is the source actor, the
//!   `AttributeDependents<Src>` that drives the fan-out sits on the source and not on the reader, so
//!   a shape that quietly degenerated into a self-effect would fail here rather than print a flat row;
//! * the dependent attribute moves by exactly the modifier amount, with `==`: every base is a
//!   multiple of `1/64` (`benches/support/identity.rs`) and every source move is `+1.0`, so every
//!   partial sum is exactly representable and an inexact resolver shows up as a red assert — the same
//!   exact-equality rule the rest of this bench family uses;
//! * `passes_to_converge` is small. §2B reads "passes > 1" as per-tick staleness, so the count is the
//!   finding, not an implementation detail: it is printed, pinned to the count this tip measured, and
//!   bounded, and the same number is written into `support/cross_actor.rs`.

#[path = "../benches/support/common.rs"]
mod common;
#[path = "../benches/support/cross_actor.rs"]
mod cross_actor;
#[path = "../benches/support/floor.rs"]
mod floor;
#[path = "../benches/support/identity.rs"]
mod identity;

use crate::common::{Attr0, Attr4, HarnessWorld};
use crate::cross_actor::{
    CROSS_ATTRS, CROSS_READERS, SourceMove, cross_shape, cross_spec, dependent_count,
    passes_to_converge, settled_on,
};
use bevy::prelude::*;
use vitality::attributes::{Attribute, AttributeDependency, AttributeDependents};
use vitality::effect::modifier::{
    AttributeModifier, EffectSubject, ModOp, ModifierOf, OwnedModifiers,
};
use vitality::effect::{AppliedEffects, EffectSource, EffectTarget};

/// The §2B point this guard runs: `(a=5, readers=1)` is one of the nine grid points the bench times,
/// and five attributes keeps the source attribute (`Attr0`) and the dependent attribute (`Attr4`)
/// different types, so "the value that moved" and "the value that was read" cannot be the same thing.
const ATTRIBUTES: usize = 5;

/// §2B's smallest K: one source, one reader, i.e. the item's two actors.
const READERS: usize = 1;

/// Ceiling on either pass count, kept as a hang guard next to the pins below: a cascade that needs more
/// ticks than this has stopped converging, and the row would be timing a livelock rather than a shape.
/// The number itself is pinned by `assert_eq!`, because a bound alone stays green while a library fix
/// moves the count and every doc that quotes it goes stale.
const MAX_PASSES: u32 = 3;

/// Finds the effect and modifier entities a reader carries, through the library's own relationship
/// components, so the test reads what the cascade reads rather than what the builder remembered.
fn effect_and_modifier(world: &World, reader: Entity) -> (Entity, Entity) {
    let applied = world
        .get::<AppliedEffects>(reader)
        .expect("a reader with a permanent cross-actor effect carries AppliedEffects");
    let effects: Vec<Entity> = applied.iter().collect();
    assert_eq!(
        effects.len(),
        1,
        "reader {reader} carries {} effects, so the shape is not one permanent effect per reader",
        effects.len()
    );
    let effect = effects[0];
    let owned = world
        .get::<OwnedModifiers>(effect)
        .expect("the effect carries its modifiers as OwnedModifiers");
    let modifiers: Vec<Entity> = owned.iter().collect();
    assert_eq!(
        modifiers.len(),
        1,
        "effect {effect} carries {} modifiers, so the row is not m=1",
        modifiers.len()
    );
    (effect, modifiers[0])
}

#[test]
fn cross_actor_cascade_converges() {
    assert!(
        CROSS_ATTRS.contains(&ATTRIBUTES) && CROSS_READERS.contains(&READERS),
        "the guard shape (a={ATTRIBUTES}, readers={READERS}) is no longer a point of the grid the \
         bench times ({CROSS_ATTRS:?} x {CROSS_READERS:?}), so this file would be guarding a shape no \
         row prints",
    );

    let spec = cross_spec(ATTRIBUTES, READERS);
    let mut shape = cross_shape::<Attr0, Attr4>(&spec, stringify!(Attr0), stringify!(Attr4));
    let world = shape.app.world();
    assert_eq!(
        world.resource::<HarnessWorld>().attrs().len(),
        ATTRIBUTES,
        "shape {} registered a different attribute width than the row's a= field",
        spec.shape_id()
    );
    assert_eq!(
        shape.readers.len(),
        READERS,
        "the row names {} readers and the world carries {}",
        READERS,
        shape.readers.len()
    );
    assert!(
        shape.markers_cleared > 0,
        "settling shape {} cleared no Dirty<T> at all, so the walk that reports a settled world \
         never saw one and the asserts below prove nothing",
        spec.shape_id()
    );

    let reader = shape.readers[0];
    let source = shape.source;
    assert_ne!(
        reader, source,
        "the shape's reader and its source are one entity, so every assert below would also hold \
         for a self-effect"
    );
    let (effect, modifier) = effect_and_modifier(world, reader);

    // The effect really points from the source at this reader: the cascade's whole reason to exist.
    assert_eq!(
        world.entity(effect).get::<EffectTarget>().map(|t| t.0),
        Some(reader),
        "effect {effect} is not applied to the reader, so nothing marks the reader dirty"
    );
    assert_eq!(
        world.entity(effect).get::<EffectSource>().map(|s| s.0),
        Some(source),
        "effect {effect} does not come from the source actor, so the row times a self-effect and \
         `recompute/cross_actor` would be a second copy of `recompute/one_attr`"
    );

    // One permanent modifier, on the dependent attribute, reading the source.
    let modifier_of = world
        .get::<ModifierOf>(modifier)
        .expect("the modifier entity names its effect");
    assert_eq!(
        modifier_of.0, effect,
        "modifier {modifier} belongs to {} and the reader's effect is {effect}",
        modifier_of.0
    );
    let card = world
        .get::<AttributeModifier<Attr4>>(modifier)
        .expect("the modifier carries AttributeModifier<Attr4>");
    assert!(
        matches!(card.who, EffectSubject::Source),
        "the modifier's subject is {:?}, not Source, so this is not the Source-subject shape \
         `recompute/cross_actor` prices",
        card.who
    );
    assert!(
        matches!(card.operation, ModOp::Add),
        "the modifier's op is {:?}, not Add, so the expected dependent value below is wrong",
        card.operation
    );

    // The fan-out component: `update_attribute` walks `AttributeDependents<Src>` on the entity whose
    // attribute changed, so it has to be the *source* that carries it. `AttributeDependency<Src>` is
    // the other half of the same relationship, on the modifier.
    let dependency = world
        .get::<AttributeDependency<Attr0>>(modifier)
        .expect("a modifier whose expression reads Attr0 spawns with AttributeDependency<Attr0>");
    assert_eq!(
        dependency.source, source,
        "modifier {modifier} depends on {} for Attr0, not on the shape's source {source}",
        dependency.source
    );
    let dependents = world
        .get::<AttributeDependents<Attr0>>(source)
        .expect("the source actor carries AttributeDependents<Attr0>, which is the fan-out list");
    let listed: Vec<Entity> = dependents.iter().collect();
    assert_eq!(
        listed,
        vec![modifier],
        "the source's Attr0 fan-out list is {listed:?}, not the reader's one modifier {modifier}, \
         so the tick that changes Attr0 would not recalculate it",
    );
    assert!(
        world.get::<AttributeDependents<Attr0>>(reader).is_none(),
        "the reader {reader} also carries AttributeDependents<Attr0>}}: the fan-out would run for the \
         reader too and the timed tick would price more cross-actor work than the shape names"
    );
    assert_eq!(
        dependent_count::<Attr0>(world, source),
        shape.readers.len(),
        "the source fans out to {} modifiers while the row's readers= field names {}",
        dependent_count::<Attr0>(world, source),
        shape.readers.len(),
    );

    let src_attr = world
        .get::<Attr0>(source)
        .expect("the source carries the attribute the cascade reads");
    let dep_before = world
        .get::<Attr4>(reader)
        .expect("the reader carries the dependent attribute");
    assert!(
        dep_before.current_value() != dep_before.base_value(),
        "reader {reader} has Attr4 current == base ({}), so the permanent effect is not folded into \
         it and there is no cascade to converge",
        dep_before.current_value()
    );
    assert!(
        settled_on::<Attr0, Attr4>(world, &shape.readers, source),
        "shape {} is not settled before the source moves: every reader's dependent value is not \
         its own base plus the source's current value, so the pass count below would be counting a \
         fix-up the shape never finished",
        spec.shape_id()
    );
    assert_eq!(
        card.value,
        src_attr.current_value(),
        "modifier {modifier} holds {}, the source's Attr0 current value is {}, so the value the \
         cascade reads and the value the effect was spawned with are different things",
        card.value,
        src_attr.current_value()
    );
    // `settled_on` aims at the source's *base*, because its current lags its own write by a resolver
    // tick and a pass count read against the lagging number would say the write had already arrived.
    // That target is only right while nothing else moves the source's current.
    assert!(
        world.get::<AppliedEffects>(source).is_none(),
        "the source {source} carries effects, so its current value is not its base and the pass \
         count below would be chasing a target the shape never settles on"
    );

    // One write to the source attribute, then the ticks it takes the reader to carry it. Both moves
    // are measured: the bench times the plain write, and the marked variant is the harness's own dirty
    // path (`AttrSpec::mark_dirty`), which skips the notify round trip.
    let base_before = src_attr.base_value();
    let value_before = dep_before.current_value();
    let write = passes_to_converge::<Attr0, Attr4>(&mut shape, SourceMove::Write);
    let marked = passes_to_converge::<Attr0, Attr4>(&mut shape, SourceMove::WriteThenMark);
    println!(
        "cross_actor {} passes_to_converge: write={write} write_then_mark={marked} (bound {MAX_PASSES})",
        spec.shape_id()
    );
    assert_eq!(
        write, 3,
        "a plain source write now converges in {write} resolver ticks, but 3 is the number the docs \
         quote: update the `write (the bench's unit) 3 passes` / `write + MarkNodeDirty 2 passes` \
         block, the `3 → 2` sentence and the `printed write=3 write_then_mark=2` line in \
         benches/support/cross_actor.rs, and the `takes 3 resolver ticks` and `arrive 3 ticks late` \
         lines of the `recompute/cross_actor` doc in benches/bench_systems.rs",
    );
    assert_eq!(
        marked, 2,
        "a source write marked by hand now converges in {marked} resolver ticks, but 2 is the number \
         the docs quote: update the `write + MarkNodeDirty 2 passes` line and the `3 → 2` sentence in \
         benches/support/cross_actor.rs, and the `2 once MarkNodeDirty is triggered by hand` line of \
         the `recompute/cross_actor` doc in benches/bench_systems.rs",
    );
    assert!(
        write <= MAX_PASSES,
        "a plain source write takes {write} resolver ticks to reach the reader (bound \
         {MAX_PASSES}): §2B's staleness finding would be bigger than the row can carry",
    );
    assert!(
        marked <= MAX_PASSES,
        "a marked source write takes {marked} resolver ticks to reach the reader (bound \
         {MAX_PASSES})",
    );

    let world = shape.app.world();
    let src_after = world.get::<Attr0>(source).expect("source attr");
    let dep_after = world.get::<Attr4>(reader).expect("reader attr");
    assert!(
        settled_on::<Attr0, Attr4>(world, &shape.readers, source),
        "shape {} did not converge: no tick left the reader's Attr4 equal to its base plus the \
         source's Attr0 current value",
        spec.shape_id()
    );
    assert_eq!(
        src_after.base_value(),
        base_before + 2.0,
        "the two source moves left the source's Attr0 base at {}, not two steps above {base_before}",
        src_after.base_value()
    );
    assert_eq!(
        dep_after.current_value(),
        value_before + 2.0,
        "the reader's Attr4 moved by {} and not by exactly the two source steps: the modifier value \
         and the source attribute are not the same number",
        dep_after.current_value() - value_before
    );
}
