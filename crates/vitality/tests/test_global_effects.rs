mod gameplay;

use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use gameplay::*;
use vitality::ability::AbilityBuilder;
use vitality::ability::AbilityDef;
use vitality::actors::{Actor, ActorBuilder};
use vitality::attributes::ManageAttributes;
use vitality::context::Vitality;
use vitality::effect::conditions::IsAttributeWithinBounds;
use vitality::effect::modifier::{EffectSubject, ModOp};
use vitality::effect::{Effect, EffectInactive, GlobalEffectPlugin};
use vitality::prelude::*;
use vitality::registry::ability_registry::AbilityToken;
use vitality::registry::effect_registry::EffectToken;
use vitality::registry::{Registry, RegistryMut};
use vitality::{AttributesPlugin, attribute, init_attribute};

#[test]
fn a_global_effect_lands_on_actors_spawned_after_it_is_added() {
    let mut app = app();
    let earlier = spawn(&mut app, hero_def());
    let aura = effect(
        &mut app,
        EffectBuilder::permanent()
            .modify::<Power>(5.0, ModOp::Add, EffectSubject::Target)
            .build(),
    );
    record::<EffectApplied>(&mut app);

    let registered = aura.clone();
    let global = with_vitality(&mut app, move |vitality| {
        vitality.add_global_effect(registered);
        vitality.global_actor()
    })
    .expect("PreStartup spawns the global actor before the first update");

    let later = spawn(&mut app, hero_def());
    ticks(&mut app, 2);

    let probe = {
        let registered = aura.clone();
        with_effects(&mut app, move |effects| {
            (
                effects.has(later, &registered),
                effects.has(earlier, &registered),
            )
        })
    };
    assert!(
        probe.0,
        "an actor spawned after add_global_effect must carry it"
    );
    assert_eq!(current::<Power>(&app, later), 15.0);
    assert!(
        !probe.1,
        "registering a global effect never replays it onto the actors that already exist"
    );
    assert_eq!(current::<Power>(&app, earlier), 10.0);

    let applied = recorded::<EffectApplied>(&app);
    assert_eq!(
        applied.len(),
        1,
        "only the actor spawned later is applied to"
    );
    assert_eq!(applied[0].entity, later);
    assert_eq!(
        applied[0].source, global,
        "the global effect actor is the source"
    );

    assert_eq!(
        with_effects(&mut app, move |effects| effects.on(global).len()),
        0,
        "the global actor's own Add<ManageAttributes> fired in PreStartup, while the registry \
         was still empty, so it gets nothing"
    );
}

#[derive(Component, Copy, Clone)]
struct Prop;

#[test]
fn a_global_effect_reaches_an_entity_carrying_a_chosen_marker_tag() {
    let mut app = app();
    app.add_plugins(GlobalEffectPlugin::<Prop>::new());
    let blessing = effect(
        &mut app,
        EffectBuilder::permanent()
            .modify::<Health>(10.0, ModOp::Add, EffectSubject::Target)
            .build(),
    );
    with_vitality(&mut app, move |vitality| {
        vitality.add_global_effect(blessing);
    });

    let post = app.world_mut().spawn((Prop, Health::new(100.0))).id();
    ticks(&mut app, 2);

    assert_eq!(
        with_effects(&mut app, move |effects| effects.on(post)).len(),
        1,
        "the marker tag is what pulls the global effect onto the entity"
    );
    assert_eq!(
        current::<Health>(&app, post),
        110.0,
        "the global bonus is live on the marker carrier"
    );
    assert!(
        !has::<ManageAttributes>(&app, post),
        "it took no actor marker to get one"
    );
}

attribute!(TestAttribute, f64);

#[derive(Component, Copy, Clone, Debug, PartialEq)]
struct ConditionTag;

fn prepare_actor(mut ctx: Vitality, registry: Registry) {
    let actor_template = ctx.add_actor(
        ActorBuilder::new()
            .name("TestActor")
            .with::<TestAttribute>(0.0)
            .grant_ability(&registry.ability(&TEST_ABILITY_TOKEN))
            .with_effect(&registry.effect(&CONDITION_EFFECT))
            .build(),
    );
    ctx.spawn_actor_from_handle(&actor_template);
}

fn prepare_effects(mut registry: RegistryMut) {
    registry.add_effect(
        TEST_EFFECT,
        Effect::permanent()
            .name("Increase Effect".into())
            .modify::<TestAttribute>(200.0, ModOp::Add, EffectSubject::Target)
            .build(),
    );

    registry.add_effect(
        CONDITION_EFFECT,
        Effect::permanent()
            .name("Condition Effect".into())
            .active_while(IsAttributeWithinBounds::<TestAttribute>::target(150.0..))
            .insert(ConditionTag)
            .build(),
    );
}

pub const TEST_EFFECT: EffectToken = EffectToken::new_static("test.test");
pub const CONDITION_EFFECT: EffectToken = EffectToken::new_static("test.condition");

fn prepare_abilities(mut abilities: RegistryMut) {
    abilities.add_ability(TEST_ABILITY_TOKEN, fireball_ability());
}

pub const TEST_ABILITY_TOKEN: AbilityToken = AbilityToken::new_static("test.test");

pub fn fireball_ability() -> AbilityDef {
    AbilityBuilder::new()
        .with_name("Test Ability".into())
        .build()
}

#[test]
fn test_attribute_condition() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default(), AttributesPlugin));
    app.add_plugins(init_attribute::<TestAttribute>);
    // A headless update lasts microseconds, so with the real clock FixedUpdate never runs.
    app.insert_resource(TimeUpdateStrategy::FixedTimesteps(1));

    app.add_systems(
        Startup,
        (prepare_effects, prepare_abilities, prepare_actor).chain(),
    );

    // conditions run on the fixed clock, which has no tick in the first update
    app.update();
    app.update();

    let mut query = app.world_mut().query::<(Entity, &Actor, &TestAttribute)>();
    let actor_id = query.single(app.world()).unwrap().0;

    // Check that the effect is inactive
    let mut query = app
        .world_mut()
        .query::<(Entity, &Effect, &ConditionTag, Option<&EffectInactive>)>();
    let opt_inactive = query.single(app.world()).unwrap().3;
    assert!(opt_inactive.is_some());

    app.world_mut()
        .run_system_once(move |mut ctx: Vitality, registry: Registry| {
            ctx.apply_effect_to_self(actor_id, &registry.effect(&TEST_EFFECT));
        })
        .unwrap();

    app.update();
    app.update();

    // Check that the effect is active
    let mut query = app
        .world_mut()
        .query::<(Entity, &Effect, &ConditionTag, Option<&EffectInactive>)>();
    let opt_inactive = query.single(app.world()).unwrap().3;
    assert!(opt_inactive.is_none());
}
