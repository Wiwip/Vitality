//! `Vitality::modify_base`, the base-write entry point: every op lands on the base, the current
//! value follows in one fixed tick, a read of an attribute the actor lacks is zero, and a write to
//! one it lacks is dropped with a warn.

mod gameplay;

#[path = "../benches/support/common.rs"]
mod common;
#[path = "support/logs.rs"]
mod logs;

use bevy::prelude::*;
use gameplay::*;
use vitality::CurrentValueChanged;
use vitality::actors::ActorBuilder;
use vitality::attributes::Attribute;
use vitality::effect::EffectBuilder;
use vitality::effect::modifier::{EffectSubject, ModOp};

/// the event is not `Clone`, so the harness' `record` cannot hold it
#[derive(Resource, Default)]
struct Fires(Vec<(f32, f32)>);

fn watch<T: Attribute<Property = f32>>(app: &mut App) {
    app.init_resource::<Fires>();
    app.add_observer(
        move |ev: On<CurrentValueChanged<T>>, mut fires: ResMut<Fires>| {
            fires.0.push((ev.event().old, ev.event().new));
        },
    );
}

fn fires(app: &App) -> Vec<(f32, f32)> {
    app.world().resource::<Fires>().0.clone()
}

fn write(app: &mut App, actor: Entity, op: ModOp, value: f32) {
    with_vitality(app, move |v| v.modify_base::<Health>(actor, op, value));
    ticks(app, 1);
}

#[test]
fn add_sub_and_set_land_on_the_base_and_the_current_value_follows_in_one_tick() {
    let mut app = app();
    let hero = spawn(&mut app, hero_def());
    watch::<Health>(&mut app);

    write(&mut app, hero, ModOp::Add, 25.0);
    assert_eq!(base::<Health>(&app, hero), 125.0, "Add moves the base");
    assert_eq!(
        current::<Health>(&app, hero),
        125.0,
        "the current value follows in the one tick"
    );

    write(&mut app, hero, ModOp::Sub, 15.0);
    assert_eq!(
        base::<Health>(&app, hero),
        110.0,
        "Sub subtracts from the base"
    );
    assert_eq!(current::<Health>(&app, hero), 110.0);

    write(&mut app, hero, ModOp::Set, 40.0);
    assert_eq!(base::<Health>(&app, hero), 40.0, "Set replaces the base");
    assert_eq!(current::<Health>(&app, hero), 40.0);

    assert_eq!(
        fires(&app),
        [(100.0, 125.0), (125.0, 110.0), (110.0, 40.0)],
        "one CurrentValueChanged per write, from the value the previous write left"
    );

    // writing the value the base already holds: the base does not move, and HEAD stays quiet
    write(&mut app, hero, ModOp::Set, 40.0);
    assert_eq!(base::<Health>(&app, hero), 40.0);
    assert_eq!(current::<Health>(&app, hero), 40.0);
    assert_eq!(
        fires(&app).len(),
        3,
        "a Set that moves nothing triggers nothing"
    );
}

#[test]
fn modify_base_on_an_attribute_the_actor_does_not_carry_changes_nothing() {
    let mut app = app();
    let mute = spawn(
        &mut app,
        ActorBuilder::new().name("Mute").with::<Health>(10.0),
    );
    watch::<Health>(&mut app);
    watch::<Armor>(&mut app);

    with_vitality(&mut app, move |v| {
        v.modify_base::<Armor>(mute, ModOp::Add, 5.0);
    });
    ticks(&mut app, 1);

    assert_eq!(
        base::<Health>(&app, mute),
        10.0,
        "nothing else moves either"
    );
    assert_eq!(current::<Health>(&app, mute), 10.0);
    assert!(
        fires(&app).is_empty(),
        "the dropped write says nothing: {:?}",
        fires(&app)
    );
}

/// A NaN base write is a write like any other: it lands, and the dependents recomputed from it land
/// on NaN too while an attribute that does not read the source stays finite.
#[test]
fn a_nan_base_write_lands_and_reaches_the_attributes_that_read_it() {
    let mut app = app();
    let hero = spawn(&mut app, hero_def());
    let scaling = effect(
        &mut app,
        EffectBuilder::permanent()
            .modify::<Armor>(Power::src(), ModOp::Add, EffectSubject::Target)
            .build(),
    );
    apply(&mut app, hero, hero, &scaling);
    ticks(&mut app, 1);
    assert!(
        current::<Armor>(&app, hero).is_finite(),
        "Armor is finite before the write, so what follows is about the cascade"
    );

    with_vitality(&mut app, move |v| {
        v.modify_base::<Power>(hero, ModOp::Set, f32::NAN);
    });
    ticks(&mut app, 1);

    assert!(
        current::<Power>(&app, hero).is_nan(),
        "the NaN has to land on the attribute it was written to"
    );
    assert!(
        current::<Armor>(&app, hero).is_nan(),
        "Armor is recomputed from Power, so the NaN has to ride the cascade"
    );
    assert!(
        current::<Health>(&app, hero).is_finite(),
        "Health does not read Power, so the NaN must not reach it"
    );
}

/// A live self-targeted modifier (`source == target == owner`) does not follow a base write to the
/// attribute it reads: what decides it is the write's cadence and the registration order, not density.
mod self_buff_base_write {

    use crate::common::{
        ATTRIBUTE_POOL, Attr0, Attr1, WorldSpec, actor_entities, build_app_with, tick,
    };
    use bevy::prelude::*;
    use vitality::attributes::{Attribute, AttributeDependents};
    use vitality::effect::definition::EffectDef;
    use vitality::effect::modifier::{AttributeModifier, EffectSubject, ModOp, OwnedModifiers};
    use vitality::effect::{AppliedEffects, ApplyEffectEvent, EffectBuilder, EffectTargeting};

    const WRITE: f64 = 100.0;

    const SETTLE: usize = 3;

    #[test]
    fn a_self_buff_written_the_tick_after_it_lands_follows_the_source_move() {
        let spec = WorldSpec {
            attributes_per_actor: 2,
            effects_per_actor: 0,
            modifiers_per_effect: 1,
            actors: 1,
            dirty_fraction: 0.0,
            ticking_fraction: 0.0,
        };
        let mut app = build_app_with(&spec, ATTRIBUTE_POOL);
        let actor = actor_entities(app.world_mut())[0];
        let handle = app.world_mut().resource_mut::<Assets<EffectDef>>().add(
            EffectBuilder::permanent()
                .modify::<Attr1>(Attr0::src(), ModOp::Add, EffectSubject::Target)
                .build(),
        );

        // the apply cadence `scenario::queue_apply` uses: queued, so it lands inside the next tick
        {
            let world = app.world_mut();
            let mut commands = world.commands();
            commands.trigger(ApplyEffectEvent {
                entity: actor,
                targeting: EffectTargeting::new(actor, actor),
                handle,
            });
        }
        app.world_mut().flush();
        tick(&mut app);

        let (effect, modifier) = {
            let world = app.world();
            let effects: Vec<Entity> = world.get::<AppliedEffects>(actor).unwrap().iter().collect();
            let effect = *effects.last().expect("the self-buff landed on the actor");
            let modifiers: Vec<Entity> = world
                .get::<OwnedModifiers>(effect)
                .unwrap()
                .iter()
                .collect();
            (effect, modifiers[0])
        };
        let source = ATTRIBUTE_POOL[0].current(app.world(), actor).unwrap() as f32;
        let before = ATTRIBUTE_POOL[1].current(app.world(), actor).unwrap() as f32;
        let base = ATTRIBUTE_POOL[1].base(app.world(), actor).unwrap() as f32;
        assert!(
            app.world()
                .get::<AttributeDependents<Attr0>>(actor)
                .is_some(),
            "the dependency edge has to be on the actor or this test is not about the notify step"
        );
        assert!(
            (before - (base + source)).abs() < 1e-3,
            "the apply is not folded yet: Attr1 {before}, want base {base} + Attr0 {source}"
        );

        ATTRIBUTE_POOL[0].set_base(app.world_mut(), actor, source as f64 + WRITE);
        for _ in 0..SETTLE {
            tick(&mut app);
        }

        // the cascade ran: the modifier knows the new value, so the loss is the refresh after it
        let value = app
            .world()
            .get::<AttributeModifier<Attr1>>(modifier)
            .unwrap()
            .value;
        assert!(
            (value - (source + WRITE as f32)).abs() < 1e-3,
            "the dependent modifier was not even re-evaluated: {value}, want {}",
            source + WRITE as f32
        );
        let after = ATTRIBUTE_POOL[1].current(app.world(), actor).unwrap() as f32;
        assert!(
            (after - (before + WRITE as f32)).abs() < 1e-3,
            "self-buff on {actor:?} (effect {effect:?}): Attr0 moved +{WRITE} and the modifier followed, \
         but Attr1 stayed at {after}, want {}",
            before + WRITE as f32
        );
    }
}

/// The missing-attribute fallback: a `T::src()` read of an attribute the actor lacks reads zero.
/// `warn_once!` is per call site, so one warning covers both actors and every tick of the run.
mod missing_attribute {

    use crate::gameplay::*;
    use bevy::log::tracing::subscriber::with_default;
    use bevy::log::tracing_subscriber::{Registry, fmt, layer::SubscriberExt};
    use std::io::{self, Write};
    use std::sync::{Arc, Mutex};
    use vitality::actors::ActorBuilder;
    use vitality::effect::modifier::{EffectSubject, ModOp};
    use vitality::prelude::*;

    /// The resolver settles a dependent hop in the same tick, so 4 leaves room.
    const SETTLE: usize = 4;

    fn drain() -> EffectBuilder {
        EffectBuilder::permanent().modify::<Health>(Power::src(), ModOp::Sub, EffectSubject::Target)
    }

    #[derive(Clone)]
    struct Sink(Arc<Mutex<String>>);

    impl Write for Sink {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0
                .lock()
                .unwrap()
                .push_str(&String::from_utf8_lossy(buf));
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn reading_an_attribute_the_actor_lacks_is_zero() {
        let logs = Arc::new(Mutex::new(String::new()));
        let writer = logs.clone();
        let subscriber = Registry::default().with(
            fmt::layer()
                .with_ansi(false)
                .without_time()
                .with_writer(move || Sink(writer.clone())),
        );

        // thread-local layer: the global one LogPlugin::build installs would race the other tests here
        with_default(subscriber, || {
            let mut app = app();
            let full = spawn(
                &mut app,
                ActorBuilder::new()
                    .name("full")
                    .with::<Health>(100.0)
                    .with::<Power>(10.0),
            );
            let bare = spawn(
                &mut app,
                ActorBuilder::new().name("bare").with::<Health>(100.0),
            );
            assert!(
                !has::<Power>(&app, bare),
                "the bare actor has to lack the attribute the shared definition reads"
            );

            let drain = effect(&mut app, drain().build());
            apply(&mut app, full, full, &drain);
            apply(&mut app, bare, bare, &drain);
            ticks(&mut app, SETTLE);

            // equality is the NaN guard too: NaN compares false against every number
            assert_eq!(current::<Health>(&app, full), 90.0);
            assert_eq!(
                current::<Health>(&app, bare),
                100.0,
                "the missing Power has to read as zero, not NaN"
            );

            ticks(&mut app, SETTLE);
            assert_eq!(current::<Health>(&app, full), 90.0);
            assert_eq!(base::<Health>(&app, bare), 100.0);
            assert_eq!(
                current::<Health>(&app, bare),
                100.0,
                "the fallback has to be stable across ticks, not creep"
            );
        });

        let logs = logs.lock().unwrap();
        let warned = logs.matches("reading it as zero").count();
        assert_eq!(
            warned,
            1,
            "one missing-attribute read across two actors and {} ticks has to warn once, saw {warned}: {logs:?}",
            2 * SETTLE
        );
        assert!(
            logs.contains("put the component on the actor"),
            "the warning has to name the fix, not just the zero: {logs:?}"
        );
    }
}

/// The other end of the deferral: an instant write lands a tick after the apply, so an actor that
/// died in the frame of the apply can be gone by then. The projectile case — hit, apply, despawn.
mod despawned_actor {

    use crate::gameplay::*;
    use crate::logs::Logs;
    use bevy::ecs::schedule::IntoScheduleConfigs;
    use bevy::log::tracing::subscriber::with_default;
    use bevy::log::tracing_subscriber::{Registry, layer::SubscriberExt};
    use bevy::prelude::{Commands, Entity, Local, Update};
    use vitality::effect::modifier::{EffectSubject, ModOp};
    use vitality::prelude::*;

    #[test]
    fn an_instant_write_whose_source_died_on_the_apply_tick_is_dropped_with_a_warn() {
        let logs = Logs::default();
        let subscriber = Registry::default().with(logs.layer());

        let mut projectile = Entity::PLACEHOLDER;
        with_default(subscriber, || {
            let mut app = app();
            let hero = spawn(&mut app, hero_def());
            projectile = app.world_mut().spawn(()).id();
            let hit = effect(
                &mut app,
                EffectBuilder::instant()
                    .modify::<Health>(5.0, ModOp::Sub, EffectSubject::Target)
                    .build(),
            );
            // the collision applies the damage and a later system of the same frame kills the
            // projectile, still ahead of the deferred write in the next FixedUpdate
            app.add_systems(
                Update,
                (
                    move |mut fired: Local<bool>, mut effects: Effects| {
                        if *fired {
                            return;
                        }
                        *fired = true;
                        effects.apply(projectile, hero, &hit);
                    },
                    move |mut fired: Local<bool>, mut commands: Commands| {
                        if *fired {
                            return;
                        }
                        *fired = true;
                        commands.entity(projectile).despawn();
                    },
                )
                    .chain(),
            );
            ticks(&mut app, 2);

            assert!(
                app.world().get_entity(projectile).is_err(),
                "the source is gone"
            );
            assert_eq!(
                base::<Health>(&app, hero),
                100.0,
                "the write stays dropped, which is what the semantics say"
            );
            assert_eq!(current::<Health>(&app, hero), 100.0);
        });

        let warns = logs.warns();
        let logs = logs.all();
        assert!(
            logs.contains("Mod<Health>") && logs.contains("dropped"),
            "the dropped instant write has to name the modifier at warn level, got: {logs}"
        );
        assert!(
            logs.contains(&format!("source {projectile}")),
            "and it has to name the actor that went missing, got: {logs}"
        );
        assert!(
            warns.contains(&format!("source {projectile}")),
            "the same words in a debug record are not a dropped write, so the level has to be \
             WARN, got: {warns}"
        );
    }
}
