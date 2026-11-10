#![allow(dead_code)]

//! Family B (`recompute/cross_actor`, BENCH-PLAN §2B): the cascade one dirty source starts when the
//! modifiers that read it live on *other* actors.
//!
//! The shape is K reader actors, each carrying one permanent effect whose single modifier is
//! `EffectSubject::Source` and reads the source's attribute — `modify::<Dep>(Src::src(), Add, Source)`
//! applied with `ApplyEffectEvent { entity: target, targeting: EffectTargeting::new(source, target),
//! handle }`, which is exactly what `Vitality::apply_effect_to_target` triggers
//! (`crates/vitality/src/vitality.rs:41-52`). So the effect lands on the reader, the value it adds
//! comes from the source, and the work is the event cascade rather than a fold:
//!
//! ```text
//! Src base write -> Dirty<Src> on the source -> update_attribute<Src> walks AttributeDependents<Src>
//!   (vitality_effects/src/systems.rs:201-226)
//! -> RecalculateExpression per dependent modifier -> update_modifier_when_dependencies_changed
//!   (vitality_effects/src/modifier.rs:203-229) re-evaluates Src::src() and marks the effect node
//! -> MarkNodeDirty<Dep> propagates up EffectTarget -> the reader resolves Dep on a later tick
//! ```
//!
//! Kept criterion-free so `tests/test_bench_cross_actor.rs` can include it (rule 3 of
//! `tests/test_bench_wiring.rs`).
//!
//! ## What the timed tick is, and why it is a write
//!
//! [`cascade_tick`] — one base write that moves the source, then one [`run_fixed_tick`] — is the unit
//! the group times, and a mark alone is not. That is now the plain reading: `are_different` for floats is
//! `self != rhs` (`crates/vitality_core/src/math.rs`, D-34), so a resolve that yields the value the
//! attribute already holds writes nothing, `should_notify_observers`
//! (`vitality_core/src/attributes.rs:322-331`) is false, and nothing reaches
//! `AttributeDependents<Src>` (`systems.rs:209-225`) — a bare mark on an unmoving source is one local
//! resolve and then an idle world (`tests/test_dirty_noop_resolve.rs` pins it). Before the fix the float
//! arm was `self.abs_diff(rhs) > Self::Output::MIN` with `MIN` = `f32::MIN`/`f64::MIN`, which is true for
//! equal values, so a dirty-but-unchanged source fanned the whole K-wide cascade out on every tick it was
//! resolved and a "dirty source" column built on a bare mark priced a value that never moves.
//!
//! Every sample therefore moves the source, to keep the loop in the steady state the row is about: the
//! fan-out a sample triggers is resolved by a later sample, so µs/tick is the per-tick cost of a source
//! that moves every tick with K readers hanging off it, not the cost of one lucky tick. Writing is also
//! what keeps the marks in the shape at all — with `!=` a shape nobody writes drains, which is the same
//! reason [`floor::prepare_floor`] can leave a world at zero and stay there.
//!
//! `WorldSpec` has no field for any of this — the source actor, the K cross effects and the
//! write-inside-the-timed-unit — and adding one is not allowed (`test_bench_harness.rs` writes full
//! `WorldSpec` literals), so [`cross_id`] carries it as an id suffix the way `leaf.rs` carries `excl=`.
//!
//! ## Passes to converge, measured at this tip
//!
//! [`passes_to_converge`] counts the `run_fixed_tick` calls between one source move and the tick on
//! which every reader carries it. Measured on the gate shape `(a=5/readers=1)` at this tip, `mt`/`dyn`:
//!
//! ```text
//! write (the bench's unit)          2 passes
//! write + MarkNodeDirty             2 passes
//! ```
//!
//! Both are > 1, which is §2B's staleness finding: a reader's dependent attribute lags the source it
//! reads by whole resolver ticks, and the reason is the schedule, not the cascade. Before L-4 the write
//! was marked in `EffectsSet::Notify` (`systems.rs:292-301`), which ran *after* `UpdateCurrentValues`, so
//! a base write was only resolved on the next tick; that resolve fanned out and marked the reader, and a
//! system's dirty-actor query is fixed when it starts, so the reader resolved on the tick after that —
//! three ticks for one cascade. L-4 moved the mark phase ahead of the resolve
//! (`vitality_effects/src/lib.rs`, `init_effect_attribute`), so the write and the resolve that reads its
//! `Changed<T>` now share a tick and the count is two. Two is the floor a schedule like this one has:
//! the mark, the fan-out onto the reader's effect node and the reader's own resolve are still three
//! separate command batches. `AttrSpec::mark_dirty` — what a spell or aura system that dirties its own
//! source would do — already skipped the first of the old three, so its count was 2 then and 2 now, and
//! the two halves of the pair meeting is what L-4 bought. The µs/tick column is still *not* the latency a
//! player sees: at 60 Hz a 2-pass cascade is one frame of stale derived stats.
//!
//! ## What L-4 costs, measured in one session
//!
//! L-4 is the ordering change alone. L-3's one-pass design — a `DirtyMarkers` resource, `mark_all` and a
//! per-attribute resolve phase — is a parked item and none of it is in this branch, so nothing here
//! reproduces the 1-pass rows that attempt quoted, and the 2 above is not a step towards them. Both
//! columns below are the same nine `recompute/cross_actor` ids, same binary, same machine, one session,
//! `--warm-up-time 0.5 --measurement-time 1 --sample-size 10`, median with the 95% band, µs/tick; `HEAD`
//! is `87e968e`'s `vitality_effects/src/lib.rs` in that tree and `L-4` is the reorder:
//!
//! ```text
//!  a  readers          HEAD (band)              L-4 (band)         delta
//!  1        1     29.26 (28.0 – 31.1)     26.89 (26.2 – 28.0)     −8.1%
//!  5        1     50.37 (48.6 – 51.5)     39.59 (38.8 – 40.3)    −21.4%
//! 20        1     84.49 (82.8 – 85.7)     76.81 (74.5 – 80.3)     −9.1%
//!  1       10     31.22 (30.2 – 32.1)     40.03 (37.2 – 43.5)    +28.2%
//!  5       10     44.62 (42.5 – 47.0)     48.83 (47.4 – 50.1)     +9.4%
//! 20       10     99.96 (96.8 – 104.5)    93.85 (92.1 – 96.6)     −6.1%
//!  1      100   114.61 (106.8 – 123.9)   125.29 (118.8 – 131.7)   +9.3%
//!  5      100   135.75 (131.1 – 142.6)   142.46 (135.0 – 151.7)   +4.9%
//! 20      100   217.71 (209.3 – 226.2)   201.96 (196.7 – 209.1)   −7.2%
//! ```
//!
//! Every point printed `write=3 write_then_mark=2` under HEAD and `write=2 write_then_mark=2` under the
//! reorder — the tick saving belongs to the schedule, not to a width, and it is what [`cross_shape`]
//! prints for each of the nine before timing it. The `delta` column is one run per side and does not
//! survive re-measurement. Re-running `a=1/readers=10` twice per side in the same layout gave L-4
//! medians of 46.86 and 45.58 µs against HEAD's 48.23 and 38.42
//! (`/tmp/l4-review/bench-l4-a1.log`, `/tmp/l4-review/bench-l4-a1-round2.log`,
//! `/tmp/l4-review/bench-head-a1.log`, `/tmp/l4-review/bench-head-a1-round2.log`): +6.7 % on the median
//! pair, bands overlapping in half the pairings, two pairings reading negative, and HEAD disagreeing
//! with itself by 25.5 % on non-overlapping bands — wider than eight of the nine deltas below, short of
//! only the `+28.2 %` cell, and the table's own 31.22 for that HEAD cell sits below both re-runs. So no
//! row here is a measured cost: `+28.2 %` is the one delta the run-to-run spread at `--sample-size 10`
//! cannot clear, and it belongs to the cell whose HEAD re-runs disagree the most. "HEAD-level" is the
//! only honest summary, and settling a difference of this size needs interleaved A/B rounds rather than
//! more runs of this layout. The tick saving shows up as a win nowhere in the table because the timed
//! unit is one tick of an already-settled world — what a game stops paying is the cascade tick it no
//! longer runs, and no id here times a cascade from a cold source.
//!
//! Both counts are measured from a shape that has gone quiet, and that qualifier is load-bearing since
//! D-34: `passes_to_converge` stops on the tick the *values* agree on, which leaves the reader holding
//! the `Dirty<Dep>` its own write just raised, and a second move issued from that state no longer
//! converges at all. `drain_marks` is the step between the two counts, and its bound is the premise
//! guard: one idle tick is the measured drain, so a shape that needs more has the re-dirty storm back. The cascade does run — the modifier value moves and `MarkNodeDirty<Dep>` fires on
//! the effect and on the reader — but the reader's resolve queued `try_remove::<Dirty<Dep>>`
//! (`vitality_effects/src/systems.rs:80`) in the same command batch the cascade's `try_insert`
//! (`systems.rs:37-40`) landed in, and the removal is applied second, so the mark is dropped and nothing
//! ever tells that reader again. The old `are_different` hid this by re-marking every actor every tick.
//! `cross_actor_cascade_converges` pins both halves.
//!
//! ## Measured µs/tick at this tip
//!
//! The table above came from `/tmp/l4-head.log` and `/tmp/l4-after.log`, like the four files the `delta`
//! discussion cites: all six live outside the repo, which is why every number here carries its band. The
//! one line that belongs in this file is the shape of the answer, with the build tag on the number itself
//! (D-22): on that `mt`/`dyn` run µs/tick is a level plus **1.26–1.35 µs per reader per tick** (the
//! `a=20` row, `readers=1` to `readers=100`: HEAD `(217.71 − 84.49)/99`, L-4 `(201.96 − 76.81)/99`), so
//! the row prices a per-reader fan-out slope, and the 2-pass lag above is what that slope buys.
//!
//! ## Two other things this shape shows
//!
//! * A permanent effect applied to a *settled* actor marks nothing: nothing observes the `EffectTarget`
//!   / `AppliedEffects` insert, so the modifier is never folded into `current_value` until something
//!   else dirties the target attribute. [`cross_shape`] therefore asks for the first fold through the
//!   library's own `MarkNodeDirty<Dep>` event; a game that applies a buff to an idle actor has the same
//!   hole. (Spawning via `ActorBuilder::with_effect` hides it, because `on_add_attribute` marks every
//!   inserted attribute of the fresh actor.)
//! * `on_change_notify_attribute_dependencies::<T>` (`systems.rs:275-290`) fires
//!   `AttributeDependencyChanged` for every dependent on every `Changed<T>` — K triggers per source
//!   write — and no observer in the workspace is registered for that event, so a share of the fan-out
//!   the row prices is work nothing reads.

use crate::common::{
    ATTRIBUTE_POOL, AttrSpec, HarnessWorld, WorldSpec, actor_entities, build_app_with,
    run_fixed_tick,
};
use crate::floor;
use crate::identity::stagger_actors;
use bevy::prelude::*;
use std::marker::PhantomData;
use vitality::actors::{ActorBuilder, SpawnActorCommand};
use vitality::attributes::{Attribute, AttributeDependents};
use vitality::effect::definition::EffectDef;
use vitality::effect::modifier::EffectSubject;
use vitality::effect::modifier::{ModOp, OwnedModifiers};
use vitality::effect::systems::MarkNodeDirty;
use vitality::effect::{AppliedEffects, ApplyEffectEvent, EffectBuilder, EffectTargeting};

/// The K axis of §2B: readers hanging off one dirty source. 1 is the smallest cascade there is and 100
/// is the widest `recompute/many_actors` point scaled to one source.
pub const CROSS_READERS: [usize; 3] = [1, 10, 100];

/// The A axis of §2B. Each point registers that many attributes and runs the cascade between the
/// pool's first slot (the source attribute) and its last (the dependent one), so `a=1` is the shape
/// where both are the same type and `a=20` is the widest the default pool serves without a second
/// pool. `tests/test_bench_cross_actor.rs` uses 5, which keeps the two attributes different types.
pub const CROSS_ATTRS: [usize; 3] = [1, 5, 20];

/// How far one [`cascade_tick`] moves the source attribute. An integer step keeps every value a
/// multiple of `1/64` (see `identity.rs`), so the exact-equality asserts this family uses stay exact
/// however many samples run.
pub const CROSS_STEP: f32 = 1.0;

/// Base every attribute of the source actor is spawned with, the floor of the generator's band. The
/// stagger overwrites it, so this only has to be a value the shape can be read at if stagger is ever
/// dropped.
const SOURCE_BASE: f32 = 10.0;

/// A give-up bound for [`passes_to_converge`], far above the 3 the gate test allows: a cascade that
/// needs more than this has stopped converging and the row would be timing a livelock.
pub const MAX_PASSES: u32 = 32;

/// How the source is moved for a pass count. Both variants write, because since D-34 a bare mark on a
/// value that does not move fans nothing out at all (module doc) and would be counting the ticks of a
/// cascade that never starts. The second variant also triggers the library's dirty event, which is what
/// [`common::AttrSpec::mark_dirty`](crate::common::AttrSpec) does and skips the notify round trip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceMove {
    /// A plain `DerefMut` base write — the unit the bench times.
    Write,
    /// The same write plus `MarkNodeDirty<Src>`, the harness's own dirty path.
    WriteThenMark,
}

/// A built, settled cross-actor shape and the entities the cascade runs between. Held whole: the bench
/// times [`run_fixed_tick`] on `app` for hundreds of samples.
pub struct CrossShape {
    pub app: App,
    pub spec: WorldSpec,
    /// The one actor every reader's modifier reads.
    pub source: Entity,
    /// The K reader actors, in [`common::actor_entities`](crate::common::actor_entities) order.
    pub readers: Vec<Entity>,
    /// The K effect entities, one per reader, in reader order.
    pub effects: Vec<Entity>,
    /// The K modifier entities, in reader order.
    pub modifiers: Vec<Entity>,
    /// `Dirty<T>` components the world carried when [`floor::prepare_floor`] took it over on the way to
    /// settled. Zero would mean the walk that reports a settled world never saw a marker.
    pub markers_at_handoff: usize,
}

/// The generator half of the shape: K bare reader actors, `attributes` attributes each, no effects —
/// every effect this shape carries is the cross one [`cross_shape`] applies by hand, which is why the
/// id reads `e=0` and the suffix reads `readers=`.
pub fn cross_spec(attributes: usize, readers: usize) -> WorldSpec {
    WorldSpec {
        attributes_per_actor: attributes,
        effects_per_actor: 0,
        modifiers_per_effect: 0,
        actors: readers,
        dirty_fraction: 0.0,
        ticking_fraction: 0.0,
    }
}

/// The id §2B prints: [`WorldSpec::shape_id`] plus the three things `WorldSpec` cannot carry — how many
/// readers hang off the source, and that the timed unit is a source write and a tick.
pub fn cross_id(spec: &WorldSpec) -> String {
    format!(
        "{}/readers={}/cross=src-write",
        spec.shape_id(),
        spec.actors
    )
}

/// Builds the shape: the K readers from the generator, one more actor as the source, a per-actor
/// stagger so a guard that reads reader 3 cannot be reading reader 0, one permanent
/// `EffectSubject::Source` effect applied to each reader, and a settled world.
///
/// `src_name`/`dep_name` are what `stringify!` of `S`/`D` gives. The pool is picked at run time, so
/// nothing in the type system says the attribute the cascade is typed on is one the shape registered;
/// these two asserts say it, and a mismatch means the row would be reading a component no system
/// resolves.
pub fn cross_shape<S, D>(spec: &WorldSpec, src_name: &str, dep_name: &str) -> CrossShape
where
    S: Attribute<Property = f32>,
    D: Attribute<Property = f32>,
{
    let mut app = build_app_with(spec, ATTRIBUTE_POOL);
    let attrs = app.world().resource::<HarnessWorld>().attrs();
    assert_eq!(
        attrs.len(),
        spec.attributes_per_actor,
        "shape {} registered {} attributes, not the {} its a= field names",
        spec.shape_id(),
        attrs.len(),
        spec.attributes_per_actor
    );
    let src_slot = attrs
        .first()
        .unwrap_or_else(|| panic!("shape {} has no attribute to read", spec.shape_id()));
    assert_eq!(
        src_slot.name, src_name,
        "slot 0 of the pool is {}, but the cascade is typed on {src_name}: the source attribute the \
         row prices would be a component this app never registers a resolver for",
        src_slot.name
    );
    let dep_slot = &attrs[attrs.len() - 1];
    assert_eq!(
        dep_slot.name, dep_name,
        "the shape's last slot is {}, but the dependent modifier is typed on {dep_name}",
        dep_slot.name
    );

    let source = spawn_source(app.world_mut(), attrs);
    let mut readers = actor_entities(app.world_mut());
    readers.retain(|entity| *entity != source);
    assert_eq!(
        readers.len(),
        spec.actors,
        "shape {} names {} readers and the world carries {}",
        spec.shape_id(),
        spec.actors,
        readers.len()
    );

    let mut actors = readers.clone();
    actors.push(source);
    stagger_actors(app.world_mut(), attrs, &actors);
    // Settle the stagger before applying: the modifier's value is evaluated once at spawn from the
    // source's *current* value, and a stale current there makes the spawn-time value disagree with the
    // resolver forever (`RecalculateExpression` is the only thing that refreshes it).
    let staggered = floor::prepare_floor(&mut app);
    assert!(
        staggered > 0,
        "shape {} carried no `Dirty<T>` when prepare_floor took it over, so its settle tick \
         resolved nothing, and the zero-dirty assert there is reading a world the walk never \
         saw",
        spec.shape_id()
    );

    apply_cascade::<S, D>(app.world_mut(), &readers, source);
    let markers_at_handoff = floor::prepare_floor(&mut app);
    let (effects, modifiers) = find_cascade(app.world(), &readers);

    CrossShape {
        app,
        spec: spec.clone(),
        source,
        readers,
        effects,
        modifiers,
        markers_at_handoff,
    }
}

/// One more actor with the shape's attribute width and no effects of its own: the entity every reader's
/// modifier reads. Spawned through `SpawnActorCommand` — the same command `Vitality::spawn_actor`
/// queues — because this runs on a `World`, not inside a system.
fn spawn_source(world: &mut World, attrs: &[AttrSpec]) -> Entity {
    let mut builder = ActorBuilder::new().name("cross.source");
    for attr in attrs {
        builder = attr.with_attribute(builder, SOURCE_BASE);
    }
    let handle = world
        .resource_mut::<Assets<vitality::actors::ActorDef>>()
        .add(builder.build());
    let source = world.spawn_empty().id();
    world
        .commands()
        .entity(source)
        .queue(SpawnActorCommand { handle });
    // The command's own builder_actions are queued inside it, so two drains to a live actor.
    world.flush();
    world.flush();
    source
}

/// Adds the one modifier effect and applies it to every reader with `targeting: (source, reader)`, then
/// asks for the first fold — see the module doc for why that ask is not optional.
fn apply_cascade<S, D>(world: &mut World, readers: &[Entity], source: Entity)
where
    S: Attribute<Property = f32>,
    D: Attribute<Property = f32>,
{
    let handle = world.resource_mut::<Assets<EffectDef>>().add(
        EffectBuilder::permanent()
            .modify::<D>(S::src(), ModOp::Add, EffectSubject::Source)
            .name("cross.reader".to_string())
            .build(),
    );

    for &reader in readers {
        world.trigger(ApplyEffectEvent {
            entity: reader,
            targeting: EffectTargeting::new(source, reader),
            handle: handle.clone(),
        });
    }
    for &reader in readers {
        world.trigger(MarkNodeDirty::<D> {
            entity: reader,
            phantom_data: PhantomData,
        });
    }
    world.flush();
}

/// The effect and modifier entity behind each reader, read back through `AppliedEffects` and
/// `OwnedModifiers` rather than remembered from the spawn, so a shape whose effect never landed fails
/// here instead of timing an empty cascade.
fn find_cascade(world: &World, readers: &[Entity]) -> (Vec<Entity>, Vec<Entity>) {
    readers
        .iter()
        .map(|&reader| {
            let applied = world.get::<AppliedEffects>(reader).unwrap_or_else(|| {
                panic!("reader {reader:?} carries no AppliedEffects, so no effect was applied to it")
            });
            let effects: Vec<Entity> = applied.iter().collect();
            assert_eq!(
                effects.len(),
                1,
                "reader {reader:?} carries {} effects; the shape is one permanent effect per reader",
                effects.len()
            );
            let effect = effects[0];
            let owned = world.get::<OwnedModifiers>(effect).unwrap_or_else(|| {
                panic!("effect {effect:?} carries no OwnedModifiers, so its modifier never spawned")
            });
            let modifiers: Vec<Entity> = owned.iter().collect();
            assert_eq!(
                modifiers.len(),
                1,
                "effect {effect:?} carries {} modifiers; the shape is m=1",
                modifiers.len()
            );
            (effect, modifiers[0])
        })
        .unzip()
}

/// Moves the source's attribute by [`CROSS_STEP`] with a `DerefMut` write, the way any game code
/// changing a stat does, and returns the new base.
pub fn move_source<S>(world: &mut World, source: Entity) -> f32
where
    S: Attribute<Property = f32>,
{
    let next = world
        .get::<S>(source)
        .unwrap_or_else(|| {
            panic!(
                "the source carries no {} to move",
                std::any::type_name::<S>()
            )
        })
        .base_value()
        + CROSS_STEP;
    if let Some(mut attr) = world.get_mut::<S>(source) {
        attr.set_base_value(next);
    }
    next
}

/// The unit `recompute/cross_actor` times: one write that moves the source, and the [`run_fixed_tick`]
/// that has to carry the cascade. Public so the gate test runs the closure the bench runs.
pub fn cascade_tick<S>(world: &mut World, source: Entity) -> f32
where
    S: Attribute<Property = f32>,
{
    let value = move_source::<S>(world, source);
    run_fixed_tick(world);
    value
}

/// Does every reader's dependent attribute equal *its own* base plus the value the source attribute
/// will settle at — its base, since the source carries no effect of its own?
///
/// The source's *current* value is the one thing that must not be the target: it lags its own base by a
/// resolver tick, so a pass count read against it reports the write as already arrived and the §2B
/// staleness number disappears. The shape guarantees base is the right target (no effect lands on the
/// source, so its resolver computes `current = base`), and `cross_actor_cascade_converges` pins it.
///
/// Exact `f64` equality, on purpose: every base is a multiple of `1/64` (the stagger) and every source
/// move is `+1.0`, so each partial sum is exactly representable and a resolver that loses or double
/// counts a modifier shows up here instead of hiding inside a tolerance.
pub fn settled_on<S, D>(world: &World, readers: &[Entity], source: Entity) -> bool
where
    S: Attribute<Property = f32>,
    D: Attribute<Property = f32>,
{
    let src = world
        .get::<S>(source)
        .unwrap_or_else(|| panic!("the source carries no {}", std::any::type_name::<S>()))
        .base_value() as f64;
    readers.iter().all(|&reader| {
        let attr = world.get::<D>(reader).unwrap_or_else(|| {
            panic!(
                "reader {reader:?} carries no {}",
                std::any::type_name::<D>()
            )
        });
        attr.current_value() as f64 == attr.base_value() as f64 + src
    })
}

/// How many [`run_fixed_tick`] calls after one source move it takes before every reader carries it.
///
/// The count is §2B's deliverable beside µs/tick — "passes > 1 means per-tick staleness" — so it is a
/// function the gate can call rather than a number in a log; the module doc carries what it measured.
///
/// Issue the move on a world with no leftover mark ([`drain_marks`]). From a shape still carrying the
/// `Dirty<Dep>` its last resolve raised, the next cascade's mark is dropped in the batch that inserts it
/// (module docs), the reader never resolves again, and this gives up at `MAX_PASSES`.
pub fn passes_to_converge<S, D>(shape: &mut CrossShape, dirty: SourceMove) -> u32
where
    S: Attribute<Property = f32>,
    D: Attribute<Property = f32>,
{
    let source = shape.source;
    let readers = shape.readers.clone();
    let world = shape.app.world_mut();
    let moved = move_source::<S>(world, source);
    if matches!(dirty, SourceMove::WriteThenMark) {
        world.trigger(MarkNodeDirty::<S> {
            entity: source,
            phantom_data: PhantomData,
        });
        world.flush();
    }
    for pass in 1..=MAX_PASSES {
        run_fixed_tick(world);
        if settled_on::<S, D>(world, &readers, source) {
            return pass;
        }
    }
    let left = floor::dirty_split(world);
    panic!(
        "shape {} never settled within {MAX_PASSES} ticks of moving its source to {moved}: the \
         cascade is not converging, so there is no pass count to report. Since D-34 the way to reach \
         this on a healthy cascade is a move issued on a shape that still carried its own mark \
         ({} dirty entities left, {:?} of them nodes): the reader's resolve removes that mark in the \
         same batch the cascade inserts a new one, so nothing tells it again. Call `drain_marks` first.",
        shape.spec.shape_id(),
        left.total(),
        left.node_labels().len(),
    )
}

/// How many ticks [`drain_marks`] spends before it calls the shape stuck. One is what D-34 measures: the
/// reader resolves, has nothing to write, and is clean.
pub const MAX_DRAIN_TICKS: u32 = 4;

/// Ticks until the world carries no `Dirty<T>` at all, returning how many it took.
///
/// [`passes_to_converge`] stops on the tick the *values* agree on, not the tick the world goes quiet, so
/// the reader is left holding the mark its own write raised — and a second move from that state never
/// converges (module docs, pinned by `cross_actor_cascade_converges`, which measures this drain at 1). A
/// point therefore drains between its two pass counts. The bound is the guard: with `are_different` as
/// `!=` an idle resolve always clears its mark, so a mark that survives four idle ticks is the re-dirty
/// storm back, and both counts would be measured on a world that never stops resolving.
pub fn drain_marks(shape: &mut CrossShape) -> u32 {
    for tick in 1..=MAX_DRAIN_TICKS {
        let world = shape.app.world_mut();
        run_fixed_tick(world);
        if floor::dirty_entity_count(world) == 0 {
            return tick;
        }
    }
    let split = floor::dirty_split(shape.app.world());
    panic!(
        "shape {} still carried {} dirty entities (actors {:?}, nodes {:?}) after {MAX_DRAIN_TICKS} \
         idle ticks: since D-34 a mark whose resolve has nothing to write is gone on the next tick, so \
         a survivor means something is re-marking itself and neither pass count of this point is \
         measuring a cascade that ends",
        shape.spec.shape_id(),
        split.total(),
        split.actors,
        split.node_labels(),
    )
}

/// The pass count printed beside the id it belongs to, so a bench run ships the §2B staleness number
/// next to the µs/tick it explains.
pub fn passes_line(shape: &CrossShape, write: u32, marked: u32) -> String {
    format!(
        "{} passes_to_converge write={write} write_then_mark={marked}",
        cross_id(&shape.spec)
    )
}

/// The source-side fan-out width the shape claims: how many modifier entities the source's
/// `AttributeDependents<Src>` names. The id's `readers=` field is a claim about this list, so the gate
/// test compares the two instead of trusting [`CrossShape::modifiers`].
pub fn dependent_count<S>(world: &World, source: Entity) -> usize
where
    S: Attribute<Property = f32>,
{
    world
        .get::<AttributeDependents<S>>(source)
        .unwrap_or_else(|| {
            panic!(
                "the source carries no AttributeDependents<{}>, so no modifier would be \
                 recalculated when it changes",
                std::any::type_name::<S>()
            )
        })
        .iter()
        .count()
}
