mod gameplay;

use bevy::prelude::*;
use gameplay::*;
use vitality::effect::modifier::{EffectSubject, ModOp};
use vitality::prelude::*;
use vitality_core::schedule::EffectsSet;

/// One entry per fixed tick: how many entities a `Changed<Health>` query saw on that tick.
#[derive(Resource, Default)]
struct Fires(Vec<usize>);

fn count_fires(changed: Query<&Health, Changed<Health>>, mut fires: ResMut<Fires>) {
    fires.0.push(changed.iter().count());
}

/// The harness app, plus a consumer system ordered after the resolve.
fn probe_app() -> App {
    let mut app = app();
    app.init_resource::<Fires>();
    app.add_systems(
        FixedUpdate,
        count_fires.after(EffectsSet::UpdateCurrentValues),
    );
    app
}

fn reset(app: &mut App) {
    app.world_mut().resource_mut::<Fires>().0.clear();
}

fn seen(app: &App) -> Vec<usize> {
    app.world().resource::<Fires>().0.clone()
}

fn write_base(app: &mut App, actor: Entity, value: f32) {
    with_vitality(app, move |v| {
        v.modify_base::<Health>(actor, ModOp::Set, value);
    });
    ticks(app, 1);
}

#[test]
fn a_live_modifier_marking_health_changed_fires_once_and_stops() {
    let mut app = probe_app();
    let hero = spawn(&mut app, hero_def());
    let drain = effect(
        &mut app,
        EffectBuilder::permanent()
            .modify::<Health>(-25.0, ModOp::Add, EffectSubject::Target)
            .build(),
    );

    reset(&mut app);
    apply(&mut app, hero, hero, &drain);
    ticks(&mut app, 4);

    assert_eq!(current::<Health>(&app, hero), 75.0);
    assert_eq!(
        seen(&app),
        [1, 0, 0, 0],
        "Changed<Health> must fire on the tick the modifier moves Health and on no idle tick after"
    );
}

/// `modify_base` writes the base, so the write marks `Health`; a no-op write resolves to nothing
#[test]
fn a_base_write_that_changes_nothing_marks_nothing_after_the_tick_it_lands() {
    let mut app = probe_app();
    let hero = spawn(&mut app, hero_def());

    reset(&mut app);
    write_base(&mut app, hero, 125.0);
    assert_eq!(
        current::<Health>(&app, hero),
        125.0,
        "a write that moves the base lands on the next fixed tick"
    );
    assert_eq!(seen(&app), [1], "the tick that moved it");

    reset(&mut app);
    write_base(&mut app, hero, 125.0);
    ticks(&mut app, 2);
    assert_eq!(
        current::<Health>(&app, hero),
        125.0,
        "writing the value the base already holds leaves the current value where it was"
    );
    assert_eq!(
        seen(&app),
        [1, 0, 0],
        "only the write marks Health: the resolve that gets the same value back adds no mark on any \
         later tick"
    );
}
