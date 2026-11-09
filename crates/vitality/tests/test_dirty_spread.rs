//! B-4b: the half of `test_dirty_lifetime.rs` that its shape cannot see — a `Dirty<T>` that crosses
//! an actor.
//!
//! B-4's four actors carry their effects on themselves, so `mark_node_dirty_observer` walks an
//! `EffectTarget` chain back onto the same actor and "never spreads" there cannot fail for the wrong
//! reason. On `cross_actor`'s shape one `EffectSubject::Source` modifier turns the same question into a
//! real one: the effect lives on the reader, its value comes from the source, so a mark on the source
//! *has* to turn into a `Dirty<Attr4>` on the reader or the cascade is broken — and an actor joined to
//! neither side must still be clean. Both halves are read per actor and per attribute
//! ([`recompute::dirty_names`]), never as a world-level count, because a count is satisfied by the
//! marked actor itself.
//!
//! Which half is which, in the D-34 sense (`.auto/decisions.md`, same split as B-4's module doc):
//!
//! * *still marked at the horizon* is the bug and is expected to **flip**. Its assert says to invert
//!   it rather than delete it, the way `test_dirty_lifetime.rs` words its own;
//! * *the mark arrives across the edge* and *no edge, no mark* are the guarantees. If the arrival
//!   assert goes red the cascade stopped crossing actors — a library regression, not a flip — and if
//!   the bystander assert goes red something re-marked an actor that has no dependency edge to walk.
//!
//! The mark rides with a write that moves the source ([`cross_actor::move_source`]) rather than a bare
//! `MarkNodeDirty`: `update_attribute` only notifies dependents when the recomputed value differs
//! (`vitality_core/src/attributes.rs:322-331`), and today `are_different` for floats is true for every
//! pair of values (`vitality_core/src/math.rs:32-34`), so a bare mark fans the cascade out anyway. Once
//! D-34 lands a bare mark would stop the fan-out at tick 1 and the assert that goes red would be the
//! arrival one — the wrong half. Write-then-mark is `cross_actor::SourceMove::WriteThenMark`, the
//! harness's own dirty path, and it is also why the reader carries the mark on tick 1 instead of
//! tick 2 ("Passes to converge" in `benches/support/cross_actor.rs`).
//!
//! The world is [`cross_actor::cross_shape`]'s plus [`spawn_bystander`], the one actor that shape has
//! no field for, and it is settled twice over — once by the shape, once after the spawn — because
//! `build_app_with` hands back a world the spawn observers already marked and a clean read only means
//! something on a world this file proved clean first.

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

use crate::common::{Attr0, Attr4, AttrSpec, HarnessWorld, run_fixed_tick};
use crate::cross_actor::{cross_shape, cross_spec, move_source};
use crate::floor::{dirty_entity_count, prepare_floor};
use crate::recompute::dirty_names;
use bevy::prelude::*;
use std::collections::HashSet;
use vitality::actors::{ActorBuilder, ActorDef, SpawnActorCommand};
use vitality::attributes::AttributeDependents;
use vitality::effect::AppliedEffects;

/// The §2B gate shape: five attributes keeps the source attribute (`Attr0`) and the dependent one
/// (`Attr4`) different types, so "the attribute marked" and "the attribute that arrived" cannot be the
/// same thing, and one reader is the smallest cascade that crosses an actor at all.
const ATTRIBUTES: usize = 5;
const READERS: usize = 1;

/// What `cross_shape` asserts its pool slots against: slot 0 is the attribute the cascade reads, the
/// last slot the one the reader's modifier writes.
const SOURCE_ATTR: &str = stringify!(Attr0);
const DEPENDENT_ATTR: &str = stringify!(Attr4);

/// The tick the mark has to be readable on the reader: `mark_dirty` is the dirty path that skips the
/// notify round trip, so the source resolves on tick 1 and marks the reader inside it.
const ARRIVAL_TICK: usize = 1;

/// B-4's "still dirty" horizon, at a quarter of its 22 ticks because every tick here runs a full
/// cascade instead of an idle resolve.
const HORIZON: usize = 4;

/// Base for the bystander's attributes: the same floor `cross_actor::spawn_source` spawns its source
/// with, so nothing about the bystander's values is special.
const BYSTANDER_BASE: f32 = 10.0;

/// One more actor of the shape's attribute width, with no effect and nothing pointing at it — the
/// actor `cross_shape` cannot build, because every actor that shape spawns is a reader. Spawned the
/// way `cross_actor::spawn_source` spawns its source (the library's own `SpawnActorCommand`, not a
/// hand-assembled bundle) so its attributes reach the world through the same `on_add_attribute`
/// observer and have to be settled away like every other mark here.
fn spawn_bystander(world: &mut World, pool: &[AttrSpec]) -> Entity {
    let mut builder = ActorBuilder::new().name("b4b.bystander");
    for attr in pool {
        builder = attr.with_attribute(builder, BYSTANDER_BASE);
    }
    let handle = world
        .resource_mut::<Assets<ActorDef>>()
        .add(builder.build());
    let bystander = world.spawn_empty().id();
    world
        .commands()
        .entity(bystander)
        .queue(SpawnActorCommand { handle });
    // The command queues its own builder actions, so two drains to a live actor.
    world.flush();
    world.flush();
    bystander
}

/// Fails unless `actor` carries every attribute of the pool. Without it a clean read cannot be told
/// from an actor that never had the component to be dirty on: `AttrSpec::is_dirty` is false for both.
fn assert_carries(world: &World, pool: &[AttrSpec], actor: Entity, who: &str) {
    let missing: Vec<&'static str> = pool
        .iter()
        .filter(|attr| attr.base(world, actor).is_none())
        .map(|attr| attr.name)
        .collect();
    assert!(
        missing.is_empty(),
        "{who} {actor:?} carries none of {missing:?}, so every clean read on it would be a walk \
         past a component this world never inserted",
    );
}

/// The no-edge half, re-read at every moment this file reports on. Driven off one actor's per-attribute
/// marks, so the actor this file marked cannot pay for it.
fn assert_no_leak(world: &World, pool: &[AttrSpec], bystander: Entity, when: &str) {
    let leaked = dirty_names(world, pool, bystander);
    assert!(
        leaked.is_empty(),
        "B-4b never spreads: the bystander {bystander:?} carries {leaked:?} {when}. It has no effect, \
         no modifier and no fan-out entry, so those marks arrived without a dependency edge — the \
         sideways leak B-4's shape had nowhere to show, and the reason this file exists",
    );
}

#[test]
fn marks_cross_a_cross_actor_edge_and_stop_there() {
    assert!(
        HORIZON > ARRIVAL_TICK,
        "the horizon ({HORIZON}) has to sit past the arrival tick ({ARRIVAL_TICK}), or the ticks \
         between them are never run and the horizon reads a world nothing ticked",
    );

    let spec = cross_spec(ATTRIBUTES, READERS);
    let mut shape = cross_shape::<Attr0, Attr4>(&spec, SOURCE_ATTR, DEPENDENT_ATTR);
    let pool: &'static [AttrSpec] = shape.app.world().resource::<HarnessWorld>().attrs();
    assert_eq!(
        pool.len(),
        ATTRIBUTES,
        "shape {} registered {} attributes, not the {} the shape's a= field names",
        spec.shape_id(),
        pool.len(),
        ATTRIBUTES,
    );
    assert_eq!(
        pool[0].name, SOURCE_ATTR,
        "slot 0 of the pool is {}, so the mark this file makes would not be the source attribute \
         the cascade reads",
        pool[0].name,
    );

    let source = shape.source;
    assert_eq!(
        shape.readers.len(),
        READERS,
        "shape {} names {} readers and the world carries {}",
        spec.shape_id(),
        READERS,
        shape.readers.len(),
    );
    let reader = shape.readers[0];
    assert_ne!(
        source, reader,
        "the cascade's source and its reader are one actor, so the mark below could arrive without \
         ever crossing an actor",
    );

    let bystander = spawn_bystander(shape.app.world_mut(), pool);
    // `on_add_attribute` marks every attribute the spawn inserts, so the world only counts as settled
    // after the same prepare the shape itself went through.
    let cleared = prepare_floor(&mut shape.app);
    assert!(
        cleared > 0,
        "spawning a bystander with {ATTRIBUTES} attributes left no `Dirty<T>` for `prepare_floor` to \
         clear, so the clean reads below are a walk that never saw a marker",
    );

    let roster: [(&str, Entity); 3] = [
        ("source", source),
        ("reader", reader),
        ("bystander", bystander),
    ];
    assert_eq!(
        roster
            .iter()
            .map(|(_, actor)| *actor)
            .collect::<HashSet<_>>()
            .len(),
        roster.len(),
        "the source, the reader and the bystander are not three distinct actors, so a per-actor read \
         could be satisfied by the marked actor itself",
    );

    let world = shape.app.world();
    assert_eq!(
        dirty_entity_count(world),
        0,
        "shape {} with a bystander added is not clean after {} markers were cleared, so no tick below \
         can attribute a mark to the cascade",
        spec.shape_id(),
        cleared,
    );
    let mut carried = 0;
    for (who, actor) in &roster {
        assert_carries(world, pool, *actor, who);
        carried += 1;
    }
    assert_eq!(
        carried,
        roster.len(),
        "only {carried} of the {} actors were checked for the attributes they carry, so a clean read \
         here could be an actor this loop skipped",
        roster.len(),
    );
    // The bystander's half of the bargain: nothing joins it to either side of the edge.
    assert!(
        world.get::<AppliedEffects>(bystander).is_none(),
        "the bystander {bystander:?} carries effects, so a mark landing on it would be a cascade \
         doing its job rather than a leak",
    );
    assert!(
        world.get::<AttributeDependents<Attr0>>(bystander).is_none(),
        "the bystander {bystander:?} carries the {SOURCE_ATTR} fan-out list, so it is joined to the \
         very attribute this file marks and its clean reads would prove nothing",
    );

    // One source move, one harness mark: one attribute of one actor.
    let moved = move_source::<Attr0>(shape.app.world_mut(), source);
    pool[0].mark_dirty(shape.app.world_mut(), source);
    shape.app.world_mut().flush();

    let world = shape.app.world();
    report("at the mark (flushed)", world, pool, &roster);
    assert_eq!(
        dirty_names(world, pool, source),
        [SOURCE_ATTR],
        "this file moved {SOURCE_ATTR} to {moved} and marked it on {source:?} alone, but that actor \
         reads {:?}: with no mark of its own the source has nothing to spread",
        dirty_names(world, pool, source),
    );
    let at_mark = dirty_names(world, pool, reader);
    assert!(
        at_mark.is_empty(),
        "the reader {reader:?} reads {at_mark:?} before a single tick has run, so whatever arrives \
         on it later was put there by the build, not carried across the edge",
    );
    let at_mark = dirty_names(world, pool, bystander);
    assert!(
        at_mark.is_empty(),
        "the bystander {bystander:?} reads {at_mark:?} before a single tick has run, so the world \
         this file marks is not the clean one the assert above said it was",
    );

    for number in ARRIVAL_TICK..=HORIZON {
        run_fixed_tick(shape.app.world_mut());
        let world = shape.app.world();
        report(&format!("tick {number}"), world, pool, &roster);
        assert_no_leak(world, pool, bystander, &format!("at tick {number}"));

        if number == ARRIVAL_TICK {
            let crossed = dirty_names(world, pool, reader);
            assert_eq!(
                crossed,
                [DEPENDENT_ATTR],
                "B-4b spread: one tick after a mark on {SOURCE_ATTR} of {source:?} the reader \
                 {reader:?} reads {crossed:?}. {DEPENDENT_ATTR} is the attribute the reader's \
                 `EffectSubject::Source` modifier writes, so a set without it means the cascade \
                 stopped crossing actors — the guarantee this file is named for, and the assert a \
                 shape that quietly lost its cross-actor edge has to fail",
            );
        }
    }

    let world = shape.app.world();
    let still = dirty_names(world, pool, reader);
    assert!(
        still.contains(&DEPENDENT_ATTR),
        "B-4b never decays: {HORIZON} ticks after one marked source move the reader {reader:?} reads \
         {still:?} and not {DEPENDENT_ATTR}. This is the half D-34 is approved to flip \
         (`.auto/decisions.md`, and the same split `test_dirty_lifetime.rs` documents): when marks \
         finally decay, invert this assert — the reader should be clean by the horizon — rather than \
         delete it, and leave the arrival assert above green, because a mark that stops arriving is a \
         broken cascade rather than a fixed one",
    );
    assert_eq!(
        still,
        [DEPENDENT_ATTR],
        "B-4b never spreads: at tick {HORIZON} the reader {reader:?} carries {still:?}, and \
         {DEPENDENT_ATTR} is the only attribute of it that has an edge to {SOURCE_ATTR}",
    );
}

/// The per-actor marks, printed at every point the test reads them. Reported rather than asserted: the
/// asserts name who is dirty, and the world walk beside them shows whether an effect or modifier node
/// is dirty in a way that would explain a reader's mark better than the edge does.
fn report(label: &str, world: &World, pool: &[AttrSpec], roster: &[(&str, Entity)]) {
    let per_actor: Vec<String> = roster
        .iter()
        .map(|(who, actor)| format!("{who}={:?}", dirty_names(world, pool, *actor)))
        .collect();
    println!(
        "B-4b {label}: {} of {} actors dirty {per_actor:?}; world walk sees {} dirty entities",
        roster
            .iter()
            .filter(|(_, actor)| !dirty_names(world, pool, *actor).is_empty())
            .count(),
        roster.len(),
        dirty_entity_count(world),
    );
}
