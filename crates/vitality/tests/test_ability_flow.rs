mod gameplay;

#[path = "support/logs.rs"]
mod logs;

use crate::logs::Logs;
use bevy::prelude::*;
use express_it::nodes::Node;
use express_it::plan::Plan;
use gameplay::*;
use hfsm_bevy::StateTimer;
use vitality::ability::ability_state::AbilityMachine;
use vitality::ability::context::AbilityExprContext;
use vitality::ability::cooldown_remaining;
use vitality::ability::tasks::{Tasks, sequence, wait_task};
use vitality::ability::{AbilityBuilder, AbilityDef, TargetData};
use vitality::actors::ActorBuilder;
use vitality::context::{Caster, Target};
use vitality::effect::conditions::HasComponent;
use vitality::prelude::*;

fn ability(app: &mut App, def: AbilityDef) -> Handle<AbilityDef> {
    app.world_mut()
        .resource_mut::<Assets<AbilityDef>>()
        .add(def)
}

fn caster_with(app: &mut App, abilities: &[&Handle<AbilityDef>]) -> Entity {
    let mut builder = hero_def();
    for handle in abilities {
        builder = builder.grant_ability(handle);
    }
    spawn(app, builder)
}

fn activate(app: &mut App, caster: Entity, handle: &Handle<AbilityDef>) {
    let handle = handle.clone();
    let granted = with_abilities(app, move |a| {
        a.try_activate(caster, &handle, TargetData::SelfCast)
    });
    assert!(granted, "the ability is not granted");
}

fn ability_entity(app: &mut App, caster: Entity, handle: &Handle<AbilityDef>) -> Entity {
    let handle = handle.clone();
    with_abilities(app, move |a| {
        a.get_ability_entity_by_handle(caster, &handle)
    })
    .unwrap()
}

fn failures(app: &App) -> Vec<ActivationError> {
    recorded::<AbilityActivationFailed>(app)
        .iter()
        .map(|failure| failure.reason)
        .collect()
}

fn record_all(app: &mut App) {
    record::<AbilityActivated>(app);
    record::<AbilityActivationFailed>(app);
    record::<AbilityEnded>(app);
}

fn cooldown_running(app: &App, ability: Entity) -> bool {
    let timer = app
        .world()
        .get::<StateTimer<AbilityMachine>>(ability)
        .expect("a granted ability carries its machine timer");
    cooldown_remaining(timer).is_some()
}

#[test]
fn cost_is_paid_once_and_the_cooldown_gates_reuse() {
    let mut app = app();
    let bolt = ability(
        &mut app,
        AbilityBuilder::new()
            .with_cost::<Mana>(10.0)
            .with_cooldown(0.5)
            .build(),
    );
    let caster = caster_with(&mut app, &[&bolt]);
    record_all(&mut app);

    activate(&mut app, caster, &bolt);
    ticks(&mut app, 2);
    assert_eq!(base::<Mana>(&app, caster), 40.0);
    assert_eq!(recorded::<AbilityActivated>(&app).len(), 1);
    assert_eq!(
        recorded::<AbilityEnded>(&app).len(),
        1,
        "no tasks: it ends at once"
    );

    activate(&mut app, caster, &bolt);
    ticks(&mut app, 1);
    assert_eq!(base::<Mana>(&app, caster), 40.0);
    assert_eq!(failures(&app), vec![ActivationError::OnCooldown]);

    ticks(&mut app, ticks_for(0.5));
    activate(&mut app, caster, &bolt);
    ticks(&mut app, 2);
    assert_eq!(base::<Mana>(&app, caster), 30.0);
}

#[test]
fn every_declared_cost_is_paid() {
    let mut app = app();
    let blood_magic = ability(
        &mut app,
        AbilityBuilder::new()
            .with_cost::<Mana>(10.0)
            .with_cost::<Health>(5.0)
            .build(),
    );
    let caster = caster_with(&mut app, &[&blood_magic]);

    activate(&mut app, caster, &blood_magic);
    ticks(&mut app, 2);
    assert_eq!(base::<Mana>(&app, caster), 40.0);
    assert_eq!(base::<Health>(&app, caster), 95.0);
}

#[test]
fn a_cost_is_paid_from_the_base_so_a_modifier_is_never_spent() {
    let mut app = app();
    let blessing = effect(
        &mut app,
        EffectBuilder::permanent()
            .modify::<Mana>(10.0, ModOp::Add, EffectSubject::Target)
            .build(),
    );
    let bolt = ability(
        &mut app,
        AbilityBuilder::new().with_cost::<Mana>(12.0).build(),
    );
    let caster = caster_with(&mut app, &[&bolt]);
    apply(&mut app, caster, caster, &blessing);
    ticks(&mut app, 1);
    assert_eq!(current::<Mana>(&app, caster), 60.0, "the +10 is live");

    activate(&mut app, caster, &bolt);
    ticks(&mut app, 2);
    assert_eq!(
        base::<Mana>(&app, caster),
        38.0,
        "the cost comes off the base"
    );
    assert_eq!(
        current::<Mana>(&app, caster),
        48.0,
        "the modifier is still the same +10 on top"
    );
}

#[test]
fn two_abilities_activated_before_a_resolve_both_pay() {
    let mut app = app();
    let slash = ability(
        &mut app,
        AbilityBuilder::new().with_cost::<Health>(40.0).build(),
    );
    let smash = ability(
        &mut app,
        AbilityBuilder::new().with_cost::<Health>(40.0).build(),
    );
    let caster = caster_with(&mut app, &[&slash, &smash]);

    activate(&mut app, caster, &slash);
    activate(&mut app, caster, &smash);
    ticks(&mut app, 2);
    assert_eq!(
        base::<Health>(&app, caster),
        20.0,
        "the second cast pays from what the first left"
    );
}

#[test]
fn a_second_same_frame_activation_is_refused_when_the_pool_no_longer_pays() {
    let logs = Logs::scoped();

    logs.run(|| {
        let mut app = app();
        let slash = ability(
            &mut app,
            AbilityBuilder::new().with_cost::<Health>(60.0).build(),
        );
        let smash = ability(
            &mut app,
            AbilityBuilder::new().with_cost::<Health>(60.0).build(),
        );
        let caster = caster_with(&mut app, &[&slash, &smash]);

        activate(&mut app, caster, &slash);
        activate(&mut app, caster, &smash);
        ticks(&mut app, 2);

        assert_eq!(
            base::<Health>(&app, caster),
            40.0,
            "the pool pays the first cast and refuses the second instead of draining to zero"
        );
        let warns = logs.warns();
        assert!(
            warns.contains("Health") && warns.contains("no longer covers"),
            "the unpaid cost has to say which pool stopped covering it, got: {warns}"
        );
    });
}

#[test]
fn an_unaffordable_ability_fails_with_a_reason() {
    let mut app = app();
    let nuke = ability(
        &mut app,
        AbilityBuilder::new().with_cost::<Mana>(500.0).build(),
    );
    let caster = caster_with(&mut app, &[&nuke]);
    record_all(&mut app);

    activate(&mut app, caster, &nuke);
    ticks(&mut app, 1);
    assert_eq!(failures(&app), vec![ActivationError::CannotAfford]);
    assert_eq!(base::<Mana>(&app, caster), 50.0);
}

#[test]
fn an_ability_without_a_cooldown_can_be_reused() {
    let mut app = app();
    let jab = ability(
        &mut app,
        AbilityBuilder::new().with_cost::<Mana>(1.0).build(),
    );
    let caster = caster_with(&mut app, &[&jab]);

    for _ in 0..3 {
        activate(&mut app, caster, &jab);
        ticks(&mut app, 2);
    }
    assert_eq!(base::<Mana>(&app, caster), 47.0);
}

#[test]
fn a_stunned_caster_cannot_activate() {
    let mut app = app();
    let jab = ability(
        &mut app,
        AbilityBuilder::new()
            .activation_blocked_by::<Stunned>()
            .build(),
    );
    let caster = caster_with(&mut app, &[&jab]);
    record_all(&mut app);

    with_vitality(&mut app, move |v| v.grant_tag::<Stunned>(caster));
    activate(&mut app, caster, &jab);
    ticks(&mut app, 1);
    assert_eq!(failures(&app), vec![ActivationError::Blocked]);

    let jab_entity = ability_entity(&mut app, caster, &jab);
    let check = with_abilities(&mut app, move |a| {
        a.can_activate(jab_entity, TargetData::SelfCast)
    });
    assert_eq!(check, Err(ActivationError::Blocked));
}

#[test]
fn a_second_activation_while_it_runs_reports_already_active() {
    let mut app = app();
    let hold = ability(
        &mut app,
        AbilityBuilder::new()
            .with_cost::<Mana>(10.0)
            .set_tasks(|| bsn! { Tasks [ @wait_task(10.0) ] })
            .build(),
    );
    let caster = caster_with(&mut app, &[&hold]);
    record_all(&mut app);

    activate(&mut app, caster, &hold);
    ticks(&mut app, 2);
    let hold_entity = ability_entity(&mut app, caster, &hold);
    assert!(with_abilities(&mut app, move |a| a.is_active(hold_entity)));

    activate(&mut app, caster, &hold);
    ticks(&mut app, 1);
    assert_eq!(failures(&app), vec![ActivationError::AlreadyActive]);
    assert_eq!(
        recorded::<AbilityActivated>(&app).len(),
        1,
        "the refused ask never re-enters Active"
    );
    assert_eq!(
        base::<Mana>(&app, caster),
        40.0,
        "the running ability paid once"
    );
    assert!(
        with_abilities(&mut app, move |a| a.is_active(hold_entity)),
        "the refused ask leaves the run alone"
    );

    let check = with_abilities(&mut app, move |a| {
        a.can_activate(hold_entity, TargetData::SelfCast)
    });
    assert_eq!(check, Err(ActivationError::AlreadyActive));
}

#[test]
fn an_ability_the_caster_was_never_granted_is_refused() {
    let mut app = app();
    let jab = ability(&mut app, AbilityBuilder::new().build());
    let caster = caster_with(&mut app, &[]);
    let never_granted = app.world_mut().spawn_empty().id();

    let held = with_abilities(&mut app, move |a| {
        let jab = jab.clone();
        a.get_ability_entity_by_handle(caster, &jab)
    });
    assert_eq!(held, None, "the caster holds no entity for that def");

    let check = with_abilities(&mut app, move |a| {
        a.can_activate(never_granted, TargetData::SelfCast)
    });
    assert_eq!(check, Err(ActivationError::NotGranted));
}

#[test]
fn a_target_that_is_gone_fails_with_a_reason() {
    let mut app = app();
    let spark = ability(&mut app, AbilityBuilder::new().build());
    let caster = caster_with(&mut app, &[&spark]);
    let victim = caster_with(&mut app, &[]);
    app.world_mut().despawn(victim);
    record_all(&mut app);
    let spark_entity = ability_entity(&mut app, caster, &spark);

    let check = with_abilities(&mut app, move |a| {
        a.can_activate(spark_entity, TargetData::Target(victim))
    });
    assert_eq!(check, Err(ActivationError::NoSuchTarget));

    let handle = spark.clone();
    let granted = with_abilities(&mut app, move |a| {
        a.try_activate(caster, &handle, TargetData::Target(victim))
    });
    assert!(granted, "a despawned target is still a granted ability");
    ticks(&mut app, 2);
    assert_eq!(failures(&app), vec![ActivationError::NoSuchTarget]);
    assert!(
        recorded::<AbilityActivated>(&app).is_empty(),
        "a gone target cannot activate it"
    );

    let granted = with_abilities(&mut app, move |a| {
        a.try_activate(caster, &spark, TargetData::SelfCast)
    });
    assert!(granted);
    ticks(&mut app, 2);
    assert_eq!(
        recorded::<AbilityActivated>(&app).len(),
        1,
        "the same actor still activates on itself"
    );
}

// gated on a component: HasComponent is the only condition this schema has, an attribute value cannot gate an ability
#[test]
fn an_unmet_activation_condition_fails_with_a_reason() {
    let mut app = app();
    let shout = ability(
        &mut app,
        AbilityBuilder::new()
            .with_cost::<Mana>(10.0)
            .with_cooldown(0.5)
            .with_activation_condition(Box::new(HasComponent::<Burning>::source()))
            .build(),
    );
    let caster = caster_with(&mut app, &[&shout]);
    record_all(&mut app);

    activate(&mut app, caster, &shout);
    ticks(&mut app, 1);
    assert_eq!(failures(&app), vec![ActivationError::ConditionFailed]);
    assert!(
        recorded::<AbilityActivated>(&app).is_empty(),
        "the condition is what stopped it"
    );
    assert_eq!(
        base::<Mana>(&app, caster),
        50.0,
        "a refused activation pays nothing"
    );
    let shout_entity = ability_entity(&mut app, caster, &shout);
    assert!(
        !cooldown_running(&app, shout_entity),
        "a refused activation leaves no cooldown behind"
    );

    with_vitality(&mut app, move |v| v.grant_tag::<Burning>(caster));
    activate(&mut app, caster, &shout);
    ticks(&mut app, 2);
    assert_eq!(
        recorded::<AbilityActivated>(&app).len(),
        1,
        "the same ability activates once the caster carries the component"
    );
    assert_eq!(
        failures(&app),
        vec![ActivationError::ConditionFailed],
        "the satisfied arm adds no failure"
    );
    assert_eq!(base::<Mana>(&app, caster), 40.0);
}

// activation_requires runs before the activation expressions, so a miss on both is the tag that reports
#[test]
fn a_missing_required_tag_reports_blocked_before_the_condition_fails() {
    let mut app = app();
    let seal = ability(
        &mut app,
        AbilityBuilder::new()
            .with_cost::<Mana>(10.0)
            .activation_requires::<Magic>()
            .with_activation_condition(Box::new(HasComponent::<Burning>::source()))
            .build(),
    );
    let caster = caster_with(&mut app, &[&seal]);
    record_all(&mut app);

    activate(&mut app, caster, &seal);
    ticks(&mut app, 1);
    assert_eq!(failures(&app), vec![ActivationError::Blocked]);
    assert!(
        recorded::<AbilityActivated>(&app).is_empty(),
        "the missing tag is what stopped it"
    );
    assert_eq!(base::<Mana>(&app, caster), 50.0, "a blocked pays nothing");

    with_vitality(&mut app, move |v| v.grant_tag::<Magic>(caster));
    activate(&mut app, caster, &seal);
    ticks(&mut app, 1);
    assert_eq!(
        failures(&app),
        vec![ActivationError::Blocked, ActivationError::ConditionFailed],
        "with the tag granted the still-unsatisfied expression decides"
    );
    assert!(recorded::<AbilityActivated>(&app).is_empty());
    assert_eq!(base::<Mana>(&app, caster), 50.0);
}

fn channel(app: &mut App) -> Handle<AbilityDef> {
    ability(
        app,
        AbilityBuilder::new()
            .with_tag::<Magic>()
            .activation_owned_tag::<Casting>()
            .set_tasks(|| bsn! { Tasks [ @wait_task(10.0) ] })
            .build(),
    )
}

#[test]
fn cancel_ends_an_active_ability_and_returns_its_owned_tag() {
    let mut app = app();
    let channel = channel(&mut app);
    let caster = caster_with(&mut app, &[&channel]);
    record_all(&mut app);

    activate(&mut app, caster, &channel);
    ticks(&mut app, 2);
    let entity = ability_entity(&mut app, caster, &channel);
    assert!(with_abilities(&mut app, move |a| a.is_active(entity)));
    assert!(has::<Casting>(&app, caster));

    with_abilities(&mut app, move |a| a.cancel(entity));
    ticks(&mut app, 2);
    assert!(!has::<Casting>(&app, caster));
    let ended = recorded::<AbilityEnded>(&app);
    assert_eq!(ended.len(), 1);
    assert!(ended[0].cancelled);
}

#[test]
fn an_ability_that_runs_to_its_end_is_not_reported_cancelled() {
    let mut app = app();
    let spell = ability(
        &mut app,
        AbilityBuilder::new()
            .set_tasks(|| bsn! { Tasks [ @wait_task(0.25) ] })
            .build(),
    );
    let caster = caster_with(&mut app, &[&spell]);
    record_all(&mut app);

    activate(&mut app, caster, &spell);
    ticks(&mut app, ticks_for(0.25) - 3);
    assert!(
        recorded::<AbilityEnded>(&app).is_empty(),
        "the task is still running"
    );

    ticks(&mut app, 4);
    let ended = recorded::<AbilityEnded>(&app);
    assert_eq!(ended.len(), 1);
    assert!(
        !ended[0].cancelled,
        "a task that finished on its own is not a cancel"
    );
}

#[test]
fn activating_one_ability_cancels_others_by_tag() {
    let mut app = app();
    let channel = channel(&mut app);
    let interrupt = ability(
        &mut app,
        AbilityBuilder::new()
            .cancels_abilities_with::<Magic>()
            .build(),
    );
    let caster = caster_with(&mut app, &[&channel, &interrupt]);
    record_all(&mut app);

    activate(&mut app, caster, &channel);
    ticks(&mut app, 2);
    activate(&mut app, caster, &interrupt);
    ticks(&mut app, 2);

    let channel_entity = ability_entity(&mut app, caster, &channel);
    assert!(!with_abilities(&mut app, move |a| a.is_active(channel_entity)));
    assert!(
        recorded::<AbilityEnded>(&app)
            .iter()
            .any(|ended| ended.entity == channel_entity && ended.cancelled)
    );
}

#[test]
fn an_active_ability_blocks_others_by_tag() {
    let mut app = app();
    let focus = ability(
        &mut app,
        AbilityBuilder::new()
            .blocks_abilities_with::<Magic>()
            .set_tasks(|| bsn! { Tasks [ @wait_task(10.0) ] })
            .build(),
    );
    let spell = ability(&mut app, AbilityBuilder::new().with_tag::<Magic>().build());
    let caster = caster_with(&mut app, &[&focus, &spell]);
    record_all(&mut app);

    activate(&mut app, caster, &focus);
    ticks(&mut app, 2);
    activate(&mut app, caster, &spell);
    ticks(&mut app, 1);
    assert_eq!(failures(&app), vec![ActivationError::Blocked]);

    let focus_entity = ability_entity(&mut app, caster, &focus);
    with_abilities(&mut app, move |a| a.cancel(focus_entity));
    ticks(&mut app, 2);
    activate(&mut app, caster, &spell);
    ticks(&mut app, 1);
    assert_eq!(failures(&app).len(), 1, "the block lifts with the blocker");
}

#[test]
fn try_activate_by_tag_activates_the_first_ability_that_can() {
    let mut app = app();
    let opener = ability(
        &mut app,
        AbilityBuilder::new()
            .with_tag::<Magic>()
            .with_cost::<Mana>(10.0)
            .set_tasks(|| bsn! { Tasks [ @wait_task(10.0) ] })
            .build(),
    );
    let follow_up = ability(
        &mut app,
        AbilityBuilder::new()
            .with_tag::<Magic>()
            .with_cost::<Mana>(10.0)
            .set_tasks(|| bsn! { Tasks [ @wait_task(10.0) ] })
            .build(),
    );
    let caster = caster_with(&mut app, &[&opener, &follow_up]);
    record_all(&mut app);

    with_abilities(&mut app, move |a| {
        a.try_activate_by_tag::<Magic>(caster, TargetData::SelfCast)
    });
    ticks(&mut app, 2);

    let opener_entity = ability_entity(&mut app, caster, &opener);
    let follow_up_entity = ability_entity(&mut app, caster, &follow_up);
    assert_eq!(base::<Mana>(&app, caster), 40.0, "one call pays one cost");
    let activated = recorded::<AbilityActivated>(&app);
    assert_eq!(activated.len(), 1, "one call is one activation");
    assert_eq!(
        activated[0].entity, opener_entity,
        "the first granted ability with the tag is the one that activates"
    );
    assert!(
        !with_abilities(&mut app, move |a| a.is_active(follow_up_entity)),
        "the second never leaves Ready"
    );
}

#[test]
fn a_sequence_runs_its_tasks_one_after_the_other() {
    let mut app = app();
    let combo = ability(
        &mut app,
        AbilityBuilder::new()
            .set_tasks(
                || bsn! { Tasks [ @sequence() Tasks [ @wait_task(0.25) -- @wait_task(0.25) ] ] },
            )
            .build(),
    );
    let caster = caster_with(&mut app, &[&combo]);
    record_all(&mut app);

    activate(&mut app, caster, &combo);
    ticks(&mut app, ticks_for(0.35));
    assert!(
        recorded::<AbilityEnded>(&app).is_empty(),
        "ended before the second task in the sequence ran"
    );

    ticks(&mut app, ticks_for(0.25));
    assert_eq!(recorded::<AbilityEnded>(&app).len(), 1);
}

attribute!(Rage, u32);

#[test]
fn a_plan_step_cannot_take_an_integer_base_below_zero() {
    let mut app = app();
    app.add_plugins(init_attribute::<Rage>);
    let berserk = ability(
        &mut app,
        AbilityBuilder::new()
            .on_execute(Plan::new().step(Rage::sub::<Target, _>(40u32)))
            .build(),
    );
    let caster = spawn(
        &mut app,
        ActorBuilder::new()
            .name("Berserker")
            .with::<Rage>(20u32)
            .grant_ability(&berserk),
    );

    activate(&mut app, caster, &berserk);
    ticks(&mut app, 2);

    assert_eq!(
        base::<Rage>(&app, caster),
        0u32,
        "the step overshoots the base, so the base has to saturate at zero, not wrap"
    );
}

#[test]
fn a_plan_reads_the_ground_location() {
    let mut app = app();
    let drop_x = Node::<f32, AbilityExprSchema, _>::new(|ctx: &AbilityExprContext| {
        ctx.location.map_or(0.0, |point| point.x)
    });
    let step = <Power as Attribute>::set::<Caster, AbilityExprSchema>(drop_x);
    let meteor = ability(
        &mut app,
        AbilityBuilder::new()
            .on_execute(Plan::new().step(step))
            .build(),
    );
    let caster = caster_with(&mut app, &[&meteor]);

    let handle = meteor.clone();
    with_abilities(&mut app, move |a| {
        a.try_activate(
            caster,
            &handle,
            TargetData::Location(Vec3::new(12.5, 3.0, -4.0)),
        )
    });
    ticks(&mut app, 2);

    assert_eq!(
        base::<Power>(&app, caster),
        12.5,
        "the plan reads the same ground point the On<ExecuteAbility> observer gets in trigger.target"
    );
}

#[test]
fn a_granted_ability_carries_its_definition_name() {
    let mut app = app();
    let fireball = ability(
        &mut app,
        AbilityBuilder::new()
            .with_name("Fireball".to_string())
            .build(),
    );
    let jab = ability(&mut app, AbilityBuilder::new().build());
    let caster = caster_with(&mut app, &[&fireball, &jab]);

    let named = ability_entity(&mut app, caster, &fireball);
    assert_eq!(
        app.world().get::<Name>(named).map(Name::as_str),
        Some("Fireball"),
        "the overlay reads the Name off the ability entity"
    );

    let unnamed = ability_entity(&mut app, caster, &jab);
    assert_eq!(
        app.world().get::<Name>(unnamed).map(Name::as_str),
        Some("Ability"),
        "without with_name the row keeps the builder default"
    );
}

#[test]
fn revoke_removes_the_ability() {
    let mut app = app();
    let jab = ability(&mut app, AbilityBuilder::new().build());
    let caster = caster_with(&mut app, &[&jab]);

    let handle = jab.clone();
    assert!(with_abilities(&mut app, move |a| a.revoke(caster, &handle)));
    ticks(&mut app, 1);
    let handle = jab.clone();
    let left = with_abilities(&mut app, move |a| {
        a.get_ability_entity_by_handle(caster, &handle)
    });
    assert_eq!(left, None);
}
