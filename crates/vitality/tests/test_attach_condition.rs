mod gameplay;

use bevy::prelude::{App, Entity};
use gameplay::*;
use vitality::effect::EffectStackingPolicy;
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

    // the gate is read at apply, so a late-but-alive attach must not land: Immune arrives after the miss
    app.world_mut().entity_mut(hero).insert(Immune);
    ticks(&mut app, ticks_for(DURATION - PERIOD));
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
}

#[test]
fn a_failed_attach_if_adds_no_stack_to_an_effect_already_carried() {
    let mut app = app();
    let hero = spawn(&mut app, hero_def().insert(Immune));
    let buff = effect(
        &mut app,
        EffectBuilder::permanent()
            .modify::<Power>(5.0, ModOp::Add, EffectSubject::Target)
            .attach_if(HasComponent::<Immune>::target())
            .with_stacking_policy(EffectStackingPolicy::Add {
                count: 1,
                max_stack: 5,
            })
            .build(),
    );
    record::<EffectApplied>(&mut app);

    apply(&mut app, hero, hero, &buff);
    ticks(&mut app, 1);
    let held = with_effects(&mut app, move |e| e.on(hero));
    assert_eq!(
        held.len(),
        1,
        "the satisfied gate lets the first apply attach"
    );
    let held = held[0];
    assert_eq!(with_effects(&mut app, move |e| e.stacks(held)), Some(1));

    // the second apply takes the stacking branch, and the gate has to hold there too
    app.world_mut().entity_mut(hero).remove::<Immune>();
    apply(&mut app, hero, hero, &buff);
    ticks(&mut app, 1);

    assert_eq!(
        with_effects(&mut app, move |e| e.stacks(held)),
        Some(1),
        "a failed attach_if adds no stack to the effect already carried"
    );
    assert_eq!(
        current::<Power>(&app, hero),
        15.0,
        "one stack, one modifier"
    );
    assert_eq!(
        recorded::<EffectApplied>(&app).len(),
        1,
        "the missed re-apply is not an apply"
    );
}

#[test]
fn an_effect_subject_of_attach_if_reads_the_source_because_apply_has_no_effect_entity() {
    let buff = || {
        EffectBuilder::permanent()
            .attach_if(HasComponent::<Immune>::effect())
            .modify::<Power>(5.0, ModOp::Add, EffectSubject::Target)
            .build()
    };

    {
        let mut app = app();
        let hero = spawn(&mut app, hero_def().insert(Immune));
        let carried = effect(&mut app, buff());

        apply(&mut app, hero, hero, &carried);
        ticks(&mut app, 2);

        let held = with_effects(&mut app, move |e| e.on(hero));
        assert_eq!(
            held.len(),
            1,
            "at apply the Effect subject reads the source, the only actor in reach"
        );
        assert!(
            !has::<Immune>(&app, held[0]),
            "the effect entity never carried the tag the attach read through the source"
        );
        assert_eq!(
            current::<Power>(&app, hero),
            15.0,
            "the attach survives the walk"
        );
    }

    {
        let mut app = app();
        let hero = spawn(&mut app, hero_def());
        let missed = effect(&mut app, buff());

        apply(&mut app, hero, hero, &missed);
        ticks(&mut app, 2);

        assert_eq!(
            effects_on(&mut app, hero),
            0,
            "a source without the tag misses the same gate"
        );
        assert_eq!(current::<Power>(&app, hero), 10.0);
    }
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

    // same definition, active_while: the instant branch reads activate_conditions, a miss returns early
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
