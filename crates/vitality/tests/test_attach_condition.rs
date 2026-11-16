mod gameplay;

use bevy::prelude::{App, Entity};
use gameplay::*;
use vitality::effect::conditions::HasComponent;
use vitality::effect::definition::EffectDef;
use vitality::effect::modifier::{EffectSubject, ModOp};
use vitality::prelude::*;

/// Interval and duration of the dot in the first two cells: four fires in `ticks_for(DURATION) + 1`.
const PERIOD: f32 = 0.25;
const DURATION: f32 = 1.0;

/// A dot gated on a component rather than on `ChanceCondition`, so every cell is deterministic.
fn gated_dot() -> EffectDef {
    EffectBuilder::every_second_for_duration(PERIOD, DURATION)
        .attach_if(HasComponent::<Immune>::target())
        .modify::<Health>(1.0, ModOp::Add, EffectSubject::Target)
        .build()
}

fn effects_on(app: &mut App, target: Entity) -> usize {
    with_effects(app, move |effects| effects.on(target).len())
}

#[test]
fn a_failed_attach_if_spawns_nothing_and_is_never_re_rolled() {
    let mut app = app();
    let hero = spawn(&mut app, hero_def().with::<Health>(0.0));
    let dot = effect(&mut app, gated_dot());
    record::<EffectApplied>(&mut app);
    record::<EffectBlocked>(&mut app);

    apply(&mut app, hero, hero, &dot);
    ticks(&mut app, 1);

    assert_eq!(
        effects_on(&mut app, hero),
        0,
        "a failed attach_if spawns no effect entity"
    );
    assert_eq!(
        base::<Health>(&app, hero),
        0.0,
        "a failed attach_if moves nothing"
    );
    assert_eq!(
        recorded::<EffectApplied>(&app).len(),
        0,
        "a failed attach_if is not EffectApplied"
    );
    assert_eq!(
        recorded::<EffectBlocked>(&app).len(),
        0,
        "only the tag rules fire EffectBlocked"
    );

    // well past the interval and past the duration: attach_conditions are read once, at apply
    ticks(&mut app, ticks_for(DURATION) + 4);
    assert_eq!(
        effects_on(&mut app, hero),
        0,
        "the missed gate is never re-checked on a later tick"
    );
    assert_eq!(base::<Health>(&app, hero), 0.0);
    assert_eq!(recorded::<EffectApplied>(&app).len(), 0);
    assert_eq!(recorded::<EffectBlocked>(&app).len(), 0);
}

#[test]
fn a_satisfied_attach_if_applies_and_fires() {
    let mut app = app();
    let hero = spawn(&mut app, hero_def().with::<Health>(0.0).insert(Immune));
    let dot = effect(&mut app, gated_dot());
    record::<EffectApplied>(&mut app);

    apply(&mut app, hero, hero, &dot);
    ticks(&mut app, ticks_for(PERIOD));

    assert_eq!(
        effects_on(&mut app, hero),
        1,
        "the satisfied gate lets the effect attach"
    );
    let applied = recorded::<EffectApplied>(&app);
    assert_eq!(applied.len(), 1);
    assert_eq!(applied[0].entity, hero);
    assert!(
        applied[0].effect.is_some(),
        "a periodic effect reports the entity it spawned"
    );
    assert_eq!(base::<Health>(&app, hero), 1.0, "one interval, one fire");

    ticks(&mut app, ticks_for(DURATION - PERIOD) + 1);
    assert_eq!(
        base::<Health>(&app, hero),
        4.0,
        "fires every interval to the end"
    );
}

#[test]
fn attach_if_leaves_an_instant_effect_ungated_where_active_while_gates_it() {
    // the instant branch never reads attach_conditions, so the miss changes nothing
    {
        let mut app = app();
        let hero = spawn(&mut app, hero_def());
        let hit = effect(
            &mut app,
            EffectBuilder::instant()
                .attach_if(HasComponent::<Immune>::target())
                .modify::<Health>(-10.0, ModOp::Add, EffectSubject::Target)
                .build(),
        );
        record::<EffectApplied>(&mut app);

        apply(&mut app, hero, hero, &hit);
        ticks(&mut app, 1);

        assert_eq!(
            base::<Health>(&app, hero),
            90.0,
            "an instant effect applies despite the failed attach_if"
        );
        let applied = recorded::<EffectApplied>(&app);
        assert_eq!(
            applied.len(),
            1,
            "and it reports the apply like any instant"
        );
        assert_eq!(applied[0].effect, None);
    }

    // same definition, active_while: the instant branch does read activate_conditions, and a miss
    // returns before the modifiers and before EffectApplied. No entity exists, so no EffectInactive.
    {
        let mut app = app();
        let hero = spawn(&mut app, hero_def());
        let hit = effect(
            &mut app,
            EffectBuilder::instant()
                .active_while(HasComponent::<Immune>::target())
                .modify::<Health>(-10.0, ModOp::Add, EffectSubject::Target)
                .build(),
        );
        record::<EffectApplied>(&mut app);
        record::<EffectBlocked>(&mut app);

        apply(&mut app, hero, hero, &hit);
        ticks(&mut app, 1);

        assert_eq!(
            base::<Health>(&app, hero),
            100.0,
            "the failed active_while gates the instant effect"
        );
        assert_eq!(effects_on(&mut app, hero), 0);
        assert_eq!(recorded::<EffectApplied>(&app).len(), 0);
        assert_eq!(recorded::<EffectBlocked>(&app).len(), 0);
    }
}
