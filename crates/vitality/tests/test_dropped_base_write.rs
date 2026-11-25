//! The base write of an attribute the actor lacks: dropped, and it says so; own binary for `warn_once!`.

mod gameplay;

#[path = "support/logs.rs"]
mod logs;

use crate::logs::Logs;
use bevy::app::App;
use bevy::asset::{Assets, Handle};
use express_it::plan::Plan;
use gameplay::*;
use vitality::ability::{AbilityBuilder, AbilityDef, TargetData};
use vitality::actors::ActorBuilder;
use vitality::context::Source;
use vitality::effect::modifier::{EffectSubject, ModOp};
use vitality::prelude::{AbilityActivated, AbilityActivationFailed, Attribute, EffectBuilder};

fn ability(app: &mut App, def: AbilityDef) -> Handle<AbilityDef> {
    app.world_mut()
        .resource_mut::<Assets<AbilityDef>>()
        .add(def)
}

#[test]
fn a_base_write_on_an_attribute_the_actor_lacks_is_dropped_with_a_warn() {
    let logs = Logs::scoped();

    let mute = logs.run(|| {
        let mut app = app();
        let mute = spawn(
            &mut app,
            ActorBuilder::new().name("Mute").with::<Health>(10.0),
        );
        with_vitality(&mut app, move |v| v.set_base::<Mana>(mute, 25.0));
        ticks(&mut app, 1);

        assert!(
            !has::<Mana>(&app, mute),
            "the dropped write must not conjure the component"
        );
        assert_eq!(base::<Health>(&app, mute), 10.0, "nothing else moves");
        mute
    });

    let warns = logs.warns();
    let logs = logs.all();
    assert!(
        warns.contains("Mana")
            && warns.contains(".with::<Mana>()")
            && warns.contains("init_attribute::<Mana>()"),
        "the dropped base write has to warn at WARN level and name both remedies, got: {logs}"
    );
    assert!(
        warns.contains(&mute.to_string()),
        "and name the actor the write was aimed at, got: {logs}"
    );
}

#[test]
fn a_clamp_bound_to_an_attribute_the_actor_lacks_is_dropped_with_a_warn() {
    let logs = Logs::scoped();

    let bare = logs.run(|| {
        let mut app = app();
        // the clamp only runs when the attribute it bounds resolves, so the effect has to name it
        let burn = effect(
            &mut app,
            EffectBuilder::permanent()
                .with_tag::<Burning>()
                .modify::<Health>(-1.0, ModOp::Add, EffectSubject::Target)
                .build(),
        );
        let bare = spawn(
            &mut app,
            ActorBuilder::new()
                .name("Bare")
                .with::<MaxHealth>(100.0)
                .with_effect(&burn)
                .clamp::<Health>(0.0, MaxHealth::src()),
        );
        ticks(&mut app, 1);

        assert!(
            !has::<Health>(&app, bare),
            "the dropped clamp must not conjure the component"
        );
        assert_eq!(base::<MaxHealth>(&app, bare), 100.0, "nothing else moves");
        bare
    });

    let warns = logs.warns();
    let logs = logs.all();
    assert!(
        warns.contains("Health") && warns.contains(".with::<Health>()"),
        "the dropped clamp has to warn at WARN level and name the remedy, got: {logs}"
    );
    assert!(
        warns.contains(&bare.to_string()),
        "and name the actor the clamp was put on, got: {logs}"
    );
}

#[test]
fn a_plan_step_writing_an_attribute_the_caster_lacks_is_dropped_with_an_error() {
    let logs = Logs::scoped();

    logs.run(|| {
        let mut app = app();
        let mutators = ability(
            &mut app,
            AbilityBuilder::new()
                .on_execute(
                    Plan::new()
                        .step(Mana::add::<Source, _>(25.0))
                        .step(Health::add::<Source, _>(5.0)),
                )
                .build(),
        );
        let mute = spawn(
            &mut app,
            ActorBuilder::new()
                .name("Mute")
                .with::<Health>(10.0)
                .grant_ability(&mutators),
        );
        record::<AbilityActivated>(&mut app);
        record::<AbilityActivationFailed>(&mut app);

        let handle = mutators.clone();
        let granted = with_abilities(&mut app, move |a| {
            a.try_activate(mute, &handle, TargetData::SelfCast)
        });
        ticks(&mut app, 2);

        assert!(
            granted && recorded::<AbilityActivated>(&app).len() == 1,
            "the dropped step is no PlanError, so the activation is granted and commits rather \
             than refused: what is missing is the write, not the run"
        );
        assert!(
            recorded::<AbilityActivationFailed>(&app).is_empty(),
            "nothing is refused, so nothing reads back as a rollback"
        );
        assert!(
            !has::<Mana>(&app, mute),
            "the dropped step must not conjure the component"
        );
        assert_eq!(
            base::<Health>(&app, mute),
            15.0,
            "the step beside it lands whatever the dropped one did"
        );

        let errors = logs.errors();
        assert_eq!(
            errors.lines().count(),
            1,
            "the dropped write is the only trace the run leaves, and the plan itself reports \
             success, got: {errors}"
        );
        assert!(
            errors.contains("Mana")
                && errors.contains("not found on the actor being written")
                && errors.contains("put the component on the actor"),
            "the error has to name the dropped attribute and give the remedy, got: {errors}"
        );
    });
}
