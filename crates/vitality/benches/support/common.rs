#![allow(dead_code)]

//! World generator shared by the bench target and by `tests/test_bench_harness.rs`. Kept free of
//! criterion so the gate can include it.
//!
//! [`build_app_with`] inserts `TimeUpdateStrategy::FixedTimesteps(1)`, so one [`tick`] is exactly
//! one fixed tick and [`run_fixed_tick`] is the same tick without the rest of the app. Every random
//! choice comes from [`seed_rng`], so a [`WorldSpec`] always rebuilds the same world, and
//! [`WorldSpec::shape_id`] is the label any number taken from it has to carry.
//!
//! The default pool is 24 attributes deep. A bench that wants more declares its own types with
//! `vitality::attribute!`, builds its own `&'static [AttrSpec]` of [`AttrSpec::new`] entries, and
//! hands that to [`build_app_with`].
//!
//! Timed loops take [`build_app_with`] as their `iter_batched_ref` setup and
//! `criterion::BatchSize::PerIteration` as their batch size: criterion 0.8 has no `LargeSample`, and
//! an `App` is not a value to hold several of at once.

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};
use std::marker::PhantomData;
use vitality::actors::{Actor, ActorBuilder};
use vitality::attributes::Attribute;
use vitality::context::Vitality;
use vitality::effect::EffectBuilder;
use vitality::effect::definition::EffectDef;
use vitality::effect::modifier::{EffectSubject, ModOp};
use vitality::effect::systems::{Dirty, MarkNodeDirty};
use vitality::prelude::*;
use vitality::{AttributesPlugin, init_attribute};

/// Every bench draws from the same seed (BENCH-PLAN §1.6).
pub const SEED: u64 = 7;

/// Attribute property the harness supports. `attribute!` defaults to this.
type Value = f32;

/// One attribute type erased behind `fn` pointers, so a spec can pick attributes by index.
pub struct AttrSpec {
    pub name: &'static str,
    /// `init_attribute::<T>` registration, run once per app.
    pub register: fn(&mut App),
    add_attribute: fn(ActorBuilder, Value) -> ActorBuilder,
    /// Adds one `ModOp::Add` modifier for `T` with `EffectSubject::Target`.
    add_modifier: fn(EffectBuilder, Value, EffectSubject) -> EffectBuilder,
    base_of: fn(&World, Entity) -> Option<f64>,
    current_of: fn(&World, Entity) -> Option<f64>,
    set_base_of: fn(&mut World, Entity, f64),
    mark_dirty_of: fn(&mut World, Entity),
    is_dirty_of: fn(&World, Entity) -> bool,
}

impl AttrSpec {
    pub const fn new<T>(name: &'static str) -> Self
    where
        T: Attribute<Property = Value>,
    {
        Self {
            name,
            register: init_attribute::<T>,
            add_attribute: insert_attr::<T>,
            add_modifier: add_modifier::<T>,
            base_of: base_of::<T>,
            current_of: current_of::<T>,
            set_base_of: set_base_of::<T>,
            mark_dirty_of: mark_dirty_of::<T>,
            is_dirty_of: is_dirty_of::<T>,
        }
    }

    pub fn with_attribute(&self, actor: ActorBuilder, value: Value) -> ActorBuilder {
        (self.add_attribute)(actor, value)
    }

    pub fn with_modifier(
        &self,
        effect: EffectBuilder,
        amount: Value,
        who: EffectSubject,
    ) -> EffectBuilder {
        (self.add_modifier)(effect, amount, who)
    }

    pub fn base(&self, world: &World, actor: Entity) -> Option<f64> {
        (self.base_of)(world, actor)
    }

    pub fn current(&self, world: &World, actor: Entity) -> Option<f64> {
        (self.current_of)(world, actor)
    }

    /// A `DerefMut` write, so `Changed<T>` fires even when the value is the same (BENCH-PLAN §2D).
    pub fn set_base(&self, world: &mut World, actor: Entity, value: f64) {
        (self.set_base_of)(world, actor, value)
    }

    /// Triggers `MarkNodeDirty<T>`, the library's own way to dirty an attribute.
    pub fn mark_dirty(&self, world: &mut World, actor: Entity) {
        (self.mark_dirty_of)(world, actor)
    }

    pub fn is_dirty(&self, world: &World, actor: Entity) -> bool {
        (self.is_dirty_of)(world, actor)
    }
}

fn insert_attr<T>(builder: ActorBuilder, value: Value) -> ActorBuilder
where
    T: Attribute<Property = Value>,
{
    builder.with::<T>(value)
}

fn add_modifier<T>(builder: EffectBuilder, amount: Value, who: EffectSubject) -> EffectBuilder
where
    T: Attribute<Property = Value>,
{
    builder.modify::<T>(amount, ModOp::Add, who)
}

fn base_of<T>(world: &World, actor: Entity) -> Option<f64>
where
    T: Attribute<Property = Value>,
{
    world.get::<T>(actor).map(|attr| attr.base_value() as f64)
}

fn current_of<T>(world: &World, actor: Entity) -> Option<f64>
where
    T: Attribute<Property = Value>,
{
    world
        .get::<T>(actor)
        .map(|attr| attr.current_value() as f64)
}

fn set_base_of<T>(world: &mut World, actor: Entity, value: f64)
where
    T: Attribute<Property = Value>,
{
    if let Some(mut attr) = world.get_mut::<T>(actor) {
        attr.set_base_value(value as Value);
    }
}

fn mark_dirty_of<T>(world: &mut World, actor: Entity)
where
    T: Attribute<Property = Value>,
{
    world.trigger(MarkNodeDirty::<T> {
        entity: actor,
        phantom_data: PhantomData,
    });
}

fn is_dirty_of<T>(world: &World, actor: Entity) -> bool
where
    T: Attribute<Property = Value>,
{
    world.get::<Dirty<T>>(actor).is_some()
}

macro_rules! attr_pool {
    ($( $name:ident ),+ $(,)?) => {
        $( ::vitality::attribute!($name); )+

        /// The pool a [`WorldSpec`] indexes into.
        pub static ATTRIBUTE_POOL: &[AttrSpec] = &[$(AttrSpec::new::<$name>(stringify!($name))),+];

        /// Highest `attributes_per_actor` the default pool can serve.
        pub const POOL_LEN: usize = ATTRIBUTE_POOL.len();
    };
}

attr_pool!(
    Attr0, Attr1, Attr2, Attr3, Attr4, Attr5, Attr6, Attr7, Attr8, Attr9, Attr10, Attr11, Attr12,
    Attr13, Attr14, Attr15, Attr16, Attr17, Attr18, Attr19, Attr20, Attr21, Attr22, Attr23,
);

/// The world shape a bench measures. All six fields land in [`WorldSpec::shape_id`].
#[derive(Clone, Debug)]
pub struct WorldSpec {
    pub attributes_per_actor: usize,
    /// Permanent or ticking effects applied to every actor.
    pub effects_per_actor: usize,
    pub modifiers_per_effect: usize,
    pub actors: usize,
    /// Share of the actors `mark_dirty_fraction` marks — *not* the share the built world is dirty on,
    /// which is always all of it (see [`build_app_with`]), and not a per-sample thing either: nothing
    /// clears the marks the spawn observers put there. A shape whose id has to mean what it says
    /// settles the world first, which is what `floor::prepare_floor` and `recompute::recompute_shape`
    /// are for.
    pub dirty_fraction: f32,
    /// Share of the effects built as periodic instead of permanent.
    pub ticking_fraction: f32,
}

impl Default for WorldSpec {
    fn default() -> Self {
        Self {
            attributes_per_actor: 4,
            effects_per_actor: 2,
            modifiers_per_effect: 1,
            actors: 100,
            dirty_fraction: 0.0,
            ticking_fraction: 0.0,
        }
    }
}

impl WorldSpec {
    /// Bench id carrying the shape, e.g. `a=20/e=20/m=1/actors=1000/dirty=10/tick=0`.
    pub fn shape_id(&self) -> String {
        format!(
            "a={}/e={}/m={}/actors={}/dirty={}/tick={}",
            self.attributes_per_actor,
            self.effects_per_actor,
            self.modifiers_per_effect,
            self.actors,
            percent(self.dirty_fraction),
            percent(self.ticking_fraction),
        )
    }

    /// A ready app built from the default attribute pool.
    pub fn build_app(&self) -> App {
        build_app_with(self, ATTRIBUTE_POOL)
    }
}

fn percent(fraction: f32) -> u32 {
    (fraction.clamp(0.0, 1.0) * 100.0).round() as u32
}

/// What the world was built from, kept as a resource so a test can check the resolution against
/// the plan rather than against itself.
#[derive(Resource)]
pub struct HarnessWorld {
    pub spec: WorldSpec,
    attrs: &'static [AttrSpec],
    /// One entry per effect asset the actors got.
    pub plan: Vec<EffectPlan>,
}

impl HarnessWorld {
    /// The attribute pool this world was built with.
    pub fn attrs(&self) -> &'static [AttrSpec] {
        self.attrs
    }
}

#[derive(Clone, Debug)]
pub struct EffectPlan {
    pub handle: Handle<EffectDef>,
    /// Periodic effects do not feed `current_value`; they fire on their own timer.
    pub ticking: bool,
    pub modifiers: Vec<ModifierPlan>,
}

#[derive(Clone, Debug)]
pub struct ModifierPlan {
    /// Index into [`HarnessWorld::attrs`].
    pub attribute: usize,
    pub amount: f32,
}

/// Seeded RNG so two runs of the same spec build the same world.
pub fn seed_rng() -> StdRng {
    StdRng::seed_from_u64(SEED)
}

/// Builds the world and runs `Startup`, so the returned app holds live actors carrying their
/// attributes and their effect instances. `attrs` must be at least `spec.attributes_per_actor` long.
///
/// What it does not hand back is the shape's `dirty_fraction`. `on_add_attribute` observes `Insert<T>`
/// and triggers `MarkNodeDirty` for every attribute the bundle inserts, and `spawn` flushes its
/// commands inline, so the returned world is dirty on every attribute of every actor and nothing has
/// resolved yet — measured: a 3-attribute, 4-actor build carries 24 `Dirty<T>` markers when
/// [`floor::clear_dirty`] walks it, actors and effect nodes alike. The [`mark_dirty_fraction`] call
/// below therefore adds marks to a world that already has all of them, and no amount of ticking fixes
/// it afterwards, because a resolve re-marks what it resolved. A shape that needs its `dirty=` field
/// to be true has to clear markers itself — see `floor::prepare_floor` and
/// `recompute::recompute_shape`; `test_recompute_ratio` pins the all-dirty build.
pub fn build_app_with(spec: &WorldSpec, attrs: &'static [AttrSpec]) -> App {
    assert!(
        spec.attributes_per_actor <= attrs.len(),
        "shape {} needs a pool of {}, only {} attributes declared",
        spec.shape_id(),
        spec.attributes_per_actor,
        attrs.len()
    );
    let attrs = &attrs[..spec.attributes_per_actor];

    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default(), AttributesPlugin));
    for attr in attrs {
        // Called, not `add_plugins`: every `fn(&mut App)` pointer shares one TypeId, so bevy would
        // reject the second attribute as a duplicate plugin.
        (attr.register)(&mut app);
    }
    // A headless update lasts microseconds, so with the real clock FixedUpdate never runs.
    app.insert_resource(TimeUpdateStrategy::FixedTimesteps(1));
    app.insert_resource(HarnessWorld {
        spec: spec.clone(),
        attrs,
        plan: Vec::new(),
    });
    app.add_systems(Startup, prepare_world);
    app.update();

    // Marks the shape's share on top of a world the spawn observers already made 100 % dirty, so this
    // changes nothing observable for a fraction below 1.0 and is kept only so a shape that clears
    // markers later can re-mark through the same seeded pick the id is quoted with.
    let mut rng = seed_rng();
    let actors = actor_entities(app.world_mut());
    mark_dirty_fraction(
        app.world_mut(),
        attrs,
        &actors,
        spec.dirty_fraction,
        &mut rng,
    );
    app
}

fn prepare_world(mut ctx: Vitality, mut harness: ResMut<HarnessWorld>) {
    let spec = harness.spec.clone();
    let attrs = harness.attrs;
    let mut rng = seed_rng();
    let mut plan = Vec::with_capacity(spec.effects_per_actor);

    for index in 0..spec.effects_per_actor {
        let ticking = rng.random_bool(spec.ticking_fraction as f64);
        let mut effect = if ticking {
            // ~1 firing every 1-3 ticks once `Time<Fixed>` is advancing.
            EffectBuilder::every_second_permanently(rng.random_range(0.5..1.5))
        } else {
            EffectBuilder::permanent()
        };

        let mut modifiers = Vec::with_capacity(spec.modifiers_per_effect);
        for _ in 0..spec.modifiers_per_effect {
            let attribute = rng.random_range(0..attrs.len());
            let amount = rng.random_range(1.0..2.0);
            effect = attrs[attribute].with_modifier(effect, amount, EffectSubject::Target);
            modifiers.push(ModifierPlan { attribute, amount });
        }

        let handle = ctx.add_effect(effect.name(format!("bench.effect.{index}")).build());
        plan.push(EffectPlan {
            handle,
            ticking,
            modifiers,
        });
    }

    let mut actor = ActorBuilder::new().name("bench.actor");
    for attr in attrs {
        actor = attr.with_attribute(actor, rng.random_range(10.0..100.0));
    }
    for effect in plan.iter() {
        actor = actor.with_effect(&effect.handle);
    }

    let actor = ctx.add_actor(actor.build());
    for _ in 0..spec.actors {
        ctx.spawn_actor_from_handle(&actor);
    }

    harness.plan = plan;
}

/// One `app.update()` runs exactly one fixed tick under `TimeUpdateStrategy::FixedTimesteps(1)`.
pub fn tick(app: &mut App) {
    app.update();
}

/// Runs the attribute resolver and nothing else. Timers do not advance here, so a periodic effect
/// only fires on a [`tick`]-driven run.
pub fn run_fixed_tick(world: &mut World) {
    world.run_schedule(FixedUpdate);
}

/// The actors this world spawned, excluding the global-effect actor. Sorted so the order is the
/// same whatever order the archetypes happen to be walked in.
pub fn actor_entities(world: &mut World) -> Vec<Entity> {
    let mut query = world.query_filtered::<Entity, With<Actor>>();
    let mut actors: Vec<Entity> = query.iter(world).collect();
    actors.sort();
    actors
}

/// Current value of every attribute of one actor, in pool order.
pub fn current_values(world: &World, attrs: &[AttrSpec], actor: Entity) -> Vec<f64> {
    attrs
        .iter()
        .map(|attr| {
            attr.current(world, actor)
                .unwrap_or_else(|| panic!("{} missing on {actor:?}", attr.name))
        })
        .collect()
}

/// Marks every attribute of `round(fraction * actors.len())` actors dirty and returns how many. The
/// pick is a seeded shuffle, so the count is exact and repeatable.
///
/// The marks are visible when it returns, because it flushes: `AttrSpec::mark_dirty` is a
/// `World::trigger` and the observer's `try_insert(Dirty<T>)`
/// (`vitality_effects/src/systems.rs:31-42`) is a deferred command, so without the flush the caller is
/// handed a count of marks it cannot read back — that is what `mark_dirty_fraction_marks_are_visible`
/// failed on. Flushing costs one queue drain per sample and is not timed.
///
/// What the marks then do is the library's, not this function's: `on_change_notify_attribute_parents`
/// (`vitality_effects/src/systems.rs:274-283`) re-triggers `MarkNodeDirty` off the write the next
/// resolve makes, so a `run_fixed_tick` leaves the same attributes dirty forever. Measured on the
/// tiny shape it does not *spread* — 2 of 4 actors marked stays 2 of 4 across two further ticks — but
/// on a world this module built, where the build already marked everything, "never decays" is the
/// whole story: it is dirty on every attribute of every actor for the rest of its life.
pub fn mark_dirty_fraction(
    world: &mut World,
    attrs: &[AttrSpec],
    actors: &[Entity],
    fraction: f32,
    rng: &mut StdRng,
) -> usize {
    let wanted = (actors.len() as f32 * fraction.clamp(0.0, 1.0)).round() as usize;
    let mut order: Vec<usize> = (0..actors.len()).collect();
    for index in (1..order.len()).rev() {
        let swap = rng.random_range(0..=index);
        order.swap(index, swap);
    }

    for &index in order.iter().take(wanted) {
        for attr in attrs {
            attr.mark_dirty(world, actors[index]);
        }
    }
    // Without this the `Dirty<T>` inserts are still queued and the returned count is fiction.
    world.flush();
    wanted
}
