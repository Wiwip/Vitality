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

/// The harness app on the default resolver, plus a consumer system ordered after the resolve.
fn probe_app() -> App {
    // this pin is about the Dag path, so it ignores VITALITY_RESOLVER; mode() is only the banner
    let _ = mode();
    let mut app = app_in(vitality::ResolverMode::Dag);
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
