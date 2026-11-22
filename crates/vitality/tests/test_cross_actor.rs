mod gameplay;

#[path = "support/logs.rs"]
mod logs;

use crate::logs::Logs;
use bevy::ecs::relationship::RelationshipTarget;
use bevy::prelude::Entity;
use gameplay::*;
use vitality::attributes::{AttributeDependency, AttributeDependents};
use vitality::effect::modifier::{EffectSubject, ModOp, ModifierOf, OwnedModifiers};
use vitality::effect::{EffectSource, EffectTarget};
use vitality::prelude::*;

/// The resolver settles a cross-actor chain in the same tick; a few ticks still leaves room.
const SETTLE: usize = 3;

fn set_power(app: &mut bevy::prelude::App, actor: bevy::prelude::Entity, value: f32) {
    with_vitality(app, move |v| v.set_base::<Power>(actor, value));
}

#[test]
fn a_live_aura_follows_its_casters_stat() {
    let mut app = app();
    let caster = spawn(&mut app, hero_def());
    let target = spawn(&mut app, hero_def());
    let aura = effect(
        &mut app,
        EffectBuilder::permanent()
            .modify::<Armor>(Power::src(), ModOp::Add, EffectSubject::Target)
            .build(),
    );
    apply(&mut app, caster, target, &aura);
    ticks(&mut app, SETTLE);
    assert_eq!(current::<Armor>(&app, target), 10.0);

    set_power(&mut app, caster, 30.0);
    ticks(&mut app, SETTLE);
    assert_eq!(current::<Armor>(&app, target), 30.0);
}

#[test]
fn a_live_modifier_reading_the_target_follows_the_target() {
    let mut app = app();
    let caster = spawn(&mut app, hero_def());
    let target = spawn(&mut app, hero_def());
    let scaling = effect(
        &mut app,
        EffectBuilder::permanent()
            .modify::<Armor>(Power::dst(), ModOp::Add, EffectSubject::Target)
            .build(),
    );
    apply(&mut app, caster, target, &scaling);
    ticks(&mut app, SETTLE);
    assert_eq!(current::<Armor>(&app, target), 10.0);

    set_power(&mut app, target, 25.0);
    ticks(&mut app, SETTLE);
    assert_eq!(
        current::<Armor>(&app, target),
        25.0,
        "a dst() read must depend on the target, not the source"
    );

    set_power(&mut app, caster, 99.0);
    ticks(&mut app, SETTLE);
    assert_eq!(current::<Armor>(&app, target), 25.0);
}

#[test]
fn a_modifier_reading_both_actors_follows_both() {
    let mut app = app();
    let caster = spawn(&mut app, hero_def());
    let target = spawn(&mut app, hero_def().with::<Power>(1.0));
    let both = effect(
        &mut app,
        EffectBuilder::permanent()
            .modify::<Armor>(
                Power::src() + Power::dst(),
                ModOp::Add,
                EffectSubject::Target,
            )
            .build(),
    );
    apply(&mut app, caster, target, &both);
    ticks(&mut app, SETTLE);
    assert_eq!(current::<Armor>(&app, target), 11.0);

    set_power(&mut app, caster, 20.0);
    ticks(&mut app, SETTLE);
    assert_eq!(current::<Armor>(&app, target), 21.0);

    set_power(&mut app, target, 5.0);
    ticks(&mut app, SETTLE);
    assert_eq!(current::<Armor>(&app, target), 25.0);
}

#[test]
fn a_snapshot_ignores_later_changes() {
    let mut app = app();
    let caster = spawn(&mut app, hero_def());
    let target = spawn(&mut app, hero_def());
    let frozen = effect(
        &mut app,
        EffectBuilder::permanent()
            .modify_snapshot::<Armor>(Power::src(), ModOp::Add, EffectSubject::Target)
            .build(),
    );
    apply(&mut app, caster, target, &frozen);
    ticks(&mut app, SETTLE);

    set_power(&mut app, caster, 50.0);
    ticks(&mut app, SETTLE);
    assert_eq!(current::<Armor>(&app, target), 10.0);
}

#[test]
fn a_periodic_snapshot_keeps_firing_its_captured_value() {
    let mut app = app();
    let caster = spawn(&mut app, hero_def());
    let target = spawn(&mut app, hero_def());
    let frozen = effect(
        &mut app,
        EffectBuilder::every_second_permanently(0.25)
            .modify_snapshot::<Armor>(Power::src(), ModOp::Add, EffectSubject::Target)
            .build(),
    );
    apply(&mut app, caster, target, &frozen);
    ticks(&mut app, ticks_for(0.25) + SETTLE);
    assert_eq!(
        current::<Armor>(&app, target),
        10.0,
        "the first fire pays what the apply captured"
    );

    set_power(&mut app, caster, 50.0);
    ticks(&mut app, ticks_for(0.25) + SETTLE);
    assert_eq!(
        current::<Armor>(&app, target),
        20.0,
        "the second fire still pays the captured 10, not the caster's new 50"
    );
}

#[test]
fn a_periodic_live_modifier_follows_its_caster() {
    let mut app = app();
    let caster = spawn(&mut app, hero_def());
    let target = spawn(&mut app, hero_def());
    let scaling = effect(
        &mut app,
        EffectBuilder::every_second_permanently(0.25)
            .modify::<Armor>(Power::src(), ModOp::Add, EffectSubject::Target)
            .build(),
    );
    apply(&mut app, caster, target, &scaling);
    ticks(&mut app, ticks_for(0.25) + SETTLE);
    assert_eq!(current::<Armor>(&app, target), 10.0);

    set_power(&mut app, caster, 50.0);
    ticks(&mut app, ticks_for(0.25) + SETTLE);
    assert_eq!(
        current::<Armor>(&app, target),
        60.0,
        "a live expression has to follow the caster on every fire"
    );
}

#[test]
fn the_dag_resolver_settles_a_chain_in_one_tick() {
    let mut app = app();
    let a = spawn(&mut app, hero_def());
    let b = spawn(&mut app, hero_def().with::<Power>(0.0));
    let c = spawn(&mut app, hero_def().with::<Power>(0.0));
    let armor_from_power = effect(
        &mut app,
        EffectBuilder::permanent()
            .modify::<Armor>(Power::src(), ModOp::Add, EffectSubject::Target)
            .build(),
    );
    let power_from_armor = effect(
        &mut app,
        EffectBuilder::permanent()
            .modify::<Power>(Armor::src(), ModOp::Add, EffectSubject::Target)
            .build(),
    );
    apply(&mut app, a, b, &armor_from_power);
    apply(&mut app, b, c, &power_from_armor);
    ticks(&mut app, 2);
    assert_eq!(current::<Power>(&app, c), 10.0);

    set_power(&mut app, a, 40.0);
    ticks(&mut app, 1);
    assert_eq!(
        current::<Power>(&app, c),
        40.0,
        "a -> b -> c within one tick"
    );
}

#[test]
fn a_cross_actor_effect_is_wired_from_its_source_to_its_target() {
    let mut app = app();
    let caster = spawn(&mut app, hero_def());
    let target = spawn(&mut app, hero_def());
    let aura = effect(
        &mut app,
        EffectBuilder::permanent()
            .modify::<Armor>(Power::src(), ModOp::Add, EffectSubject::Target)
            .build(),
    );
    apply(&mut app, caster, target, &aura);
    ticks(&mut app, SETTLE);

    let on_target = with_effects(&mut app, move |effects| effects.on(target));
    assert_eq!(
        on_target.len(),
        1,
        "one permanent effect on the target, so the entity read below is the aura's"
    );
    assert!(
        with_effects(&mut app, move |effects| effects.on(caster)).is_empty(),
        "the caster carries the effect it applied, so an applied effect is not owned per target"
    );

    let effect_entity = on_target[0];
    let world = app.world();
    assert_eq!(
        world.get::<EffectTarget>(effect_entity).map(|t| t.0),
        Some(target),
        "the effect's EffectTarget is not the actor that carries it"
    );
    assert_eq!(
        world.get::<EffectSource>(effect_entity).map(|s| s.0),
        Some(caster),
        "the effect's EffectSource is not the actor that applied it, so a src() read has the wrong actor"
    );

    let modifiers: Vec<Entity> = world
        .get::<OwnedModifiers>(effect_entity)
        .expect("the effect owns its modifiers")
        .iter()
        .collect();
    assert_eq!(modifiers.len(), 1, "one modifier per effect in this shape");
    let modifier = modifiers[0];
    assert_eq!(
        world.get::<ModifierOf>(modifier).map(|m| m.0),
        Some(effect_entity),
        "the modifier names an effect the target does not carry"
    );

    // the fan-out has to hang off the actor whose attribute is read, or a caster write reaches nobody
    let dependency = world
        .get::<AttributeDependency<Power>>(modifier)
        .expect("a modifier that reads an attribute spawns with its dependency edge");
    assert_eq!(
        dependency.source, caster,
        "the modifier reads Power from {} and the effect's source is {caster}",
        dependency.source,
    );
    let dependents: Vec<Entity> = world
        .get::<AttributeDependents<Power>>(caster)
        .expect("the actor whose Power is read carries the fan-out list")
        .iter()
        .collect();
    assert_eq!(
        dependents,
        vec![modifier],
        "the caster's Power fan-out list is {dependents:?}, not the one modifier that reads it"
    );
    assert!(
        world.get::<AttributeDependents<Power>>(target).is_none(),
        "the target {target} carries the fan-out list for a read of the caster's Power"
    );
}

/// One buffed actor out of sixteen: an apply or a resolve that leaks across actors, or that only
/// reaches the first actor of the world, fails here rather than in a bench.
#[test]
fn only_the_actor_an_effect_lands_on_moves() {
    let mut app = app();
    let caster = spawn(&mut app, hero_def().with::<Power>(25.0));
    let actors: Vec<Entity> = (0..16).map(|_| spawn(&mut app, hero_def())).collect();
    let aura = effect(
        &mut app,
        EffectBuilder::permanent()
            .modify::<Armor>(Power::src(), ModOp::Add, EffectSubject::Target)
            .build(),
    );
    let buffed = actors[7];
    apply(&mut app, caster, buffed, &aura);
    ticks(&mut app, SETTLE);

    for (index, actor) in actors.iter().enumerate() {
        let want = if *actor == buffed { 25.0 } else { 0.0 };
        assert_eq!(
            current::<Armor>(&app, *actor),
            want,
            "actor {index} reads the wrong value: only {buffed} was given the aura",
        );
    }
    assert_eq!(
        current::<Armor>(&app, caster),
        0.0,
        "a target-subject modifier moved its caster"
    );
}

#[test]
fn an_instant_source_modifier_writes_its_caster() {
    let mut app = app();
    let caster = spawn(&mut app, hero_def().with::<Power>(25.0));
    let target = spawn(&mut app, hero_def());
    let hex = effect(
        &mut app,
        EffectBuilder::instant()
            .modify::<Armor>(Power::src(), ModOp::Add, EffectSubject::Source)
            .build(),
    );
    apply(&mut app, caster, target, &hex);
    ticks(&mut app, SETTLE);

    assert_eq!(
        current::<Armor>(&app, caster),
        25.0,
        "a Source modifier did not move its caster's Armor"
    );
    assert_eq!(
        current::<Armor>(&app, target),
        0.0,
        "a Source modifier wrote {target}, the actor the effect landed on"
    );
}

#[test]
fn a_periodic_source_modifier_writes_its_caster() {
    let mut app = app();
    let caster = spawn(&mut app, hero_def().with::<Power>(25.0));
    let target = spawn(&mut app, hero_def());
    let drain = effect(
        &mut app,
        EffectBuilder::every_second_permanently(0.25)
            .modify::<Armor>(Power::src(), ModOp::Add, EffectSubject::Source)
            .build(),
    );
    apply(&mut app, caster, target, &drain);
    ticks(&mut app, ticks_for(0.25) + SETTLE);

    assert_eq!(
        current::<Armor>(&app, caster),
        25.0,
        "a Source modifier did not move its caster's Armor"
    );
    assert_eq!(
        current::<Armor>(&app, target),
        0.0,
        "a Source modifier wrote {target}, the actor the effect landed on"
    );
}

/// A persistent modifier is folded into the actor that carries the effect, so a `Source` one still
/// writes the target; what it owes is the apply-time warning, not a silent write.
#[test]
fn a_persistent_source_modifier_writes_the_target_and_says_so() {
    // warn_once! is per call site for the whole binary and the spawn_order cell applies the same
    // routing, so the assertion reads what the binary captured, whichever cell tripped the site
    let logs = Logs::of_binary();
    let mut app = app();
    let caster = spawn(&mut app, hero_def().with::<Power>(25.0));
    let target = spawn(&mut app, hero_def());
    let aura = effect(
        &mut app,
        EffectBuilder::permanent()
            .modify::<Armor>(Power::src(), ModOp::Add, EffectSubject::Source)
            .build(),
    );
    apply(&mut app, caster, target, &aura);
    ticks(&mut app, SETTLE);

    assert_eq!(
        current::<Armor>(&app, target),
        25.0,
        "a persistent modifier folds into the actor that carries the effect, and that is where a Source write lands"
    );
    assert_eq!(
        current::<Armor>(&app, caster),
        0.0,
        "the caster carries the effect it applied, so writing it as well would double the modifier"
    );

    let effect_entity = with_effects(&mut app, move |effects| effects.on(target))[0];
    let modifier = app
        .world()
        .get::<OwnedModifiers>(effect_entity)
        .expect("the effect owns its modifier")
        .iter()
        .next()
        .expect("one modifier");
    let described = app
        .world()
        .get::<AttributeModifier<Armor>>(modifier)
        .expect("the modifier component")
        .describe();
    assert!(
        !described.ends_with(" source"),
        "the overlay prints describe(), so a modifier that ends in `source` tells the developer the \
         caster's Armor moves: {described}"
    );
    assert!(
        described.contains("source→target"),
        "and it has to print the actor the folded write really lands on: {described}"
    );

    let warns = logs.warns();
    assert!(
        warns.contains("Mod<Armor>") && warns.contains("folds every modifier"),
        "the routing the modifier refuses to advertise in its name it owes out loud, at warn \
         level, got: {warns}"
    );
}

/// The instant write names an actor this effect has none of, so nothing moves: the effect holder is
/// neither its source nor its target.
#[test]
fn an_instant_effect_subject_modifier_is_dropped_with_a_warn() {
    let logs = Logs::of_binary();
    let mut app = app();
    let caster = spawn(&mut app, hero_def());
    let target = spawn(&mut app, hero_def());
    let hex = effect(
        &mut app,
        EffectBuilder::instant()
            .modify::<Armor>(25.0, ModOp::Add, EffectSubject::Effect)
            .build(),
    );
    apply(&mut app, caster, target, &hex);
    ticks(&mut app, SETTLE);

    assert_eq!(base::<Armor>(&app, target), 0.0, "the write stays dropped");
    assert_eq!(current::<Armor>(&app, target), 0.0);
    assert_eq!(
        current::<Armor>(&app, caster),
        0.0,
        "and it lands on no other actor instead"
    );

    // warn_once! is per call site for the whole binary, so this matches text and never a count
    let warns = logs.warns();
    assert!(
        warns.contains("Mod<Armor>")
            && warns.contains("EffectSubject::Effect")
            && warns.contains("instant")
            && warns.contains("EffectSubject::Target")
            && warns.contains("persistent"),
        "the dropped write has to name the modifier, the subject that dropped it and the remedy \
         to fix it, at warn level, got: {warns}"
    );
}

/// The resolver is parallel *across* actors only, so rebuilding the same world with the actors
/// spawned in the opposite order has to land on the same numbers, tick by tick and not just settled.
mod spawn_order {

    use crate::gameplay::*;
    use crate::logs::Logs;
    use bevy::prelude::*;
    use std::collections::BTreeMap;
    use vitality::actors::ActorBuilder;
    use vitality::effect::modifier::{EffectSubject, ModOp};
    use vitality::prelude::*;

    #[derive(Component, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
    struct Who(&'static str);

    const ACTORS: [&str; 3] = ["alpha", "beta", "gamma"];

    /// three cross-actor hops plus the clamp hop into alpha.Health, which is more than one pass settles,
    /// so eight ticks leaves room
    const TICKS: usize = 8;

    type Reads = [(&'static str, for<'a> fn(&'a App, Entity) -> f32); 10];

    const READS: Reads = [
        ("Health", current::<Health>),
        ("Health.base", base::<Health>),
        ("MaxHealth", current::<MaxHealth>),
        ("MaxHealth.base", base::<MaxHealth>),
        ("Mana", current::<Mana>),
        ("Mana.base", base::<Mana>),
        ("Power", current::<Power>),
        ("Power.base", base::<Power>),
        ("Armor", current::<Armor>),
        ("Armor.base", base::<Armor>),
    ];

    fn def(who: &'static str) -> ActorBuilder {
        let actor = ActorBuilder::new().name(who).insert(Who(who));
        match who {
            "alpha" => actor
                // above the max its clamp will see once beta's modifier lands, so the clamp has to bite
                .with::<Health>(110.0)
                .with::<MaxHealth>(120.0)
                .with::<Mana>(50.0)
                .with::<Power>(10.0)
                .with::<Armor>(4.0)
                .clamp::<Health>(0.0, MaxHealth::src()),
            "beta" => actor
                .with::<Health>(60.0)
                .with::<MaxHealth>(80.0)
                .with::<Mana>(20.0)
                .with::<Power>(5.0)
                .with::<Armor>(3.0),
            _ => actor
                .with::<Health>(250.0)
                .with::<MaxHealth>(250.0)
                .with::<Mana>(0.0)
                .with::<Power>(7.0)
                .with::<Armor>(11.0),
        }
    }

    fn world(order: [&'static str; 3]) -> (App, BTreeMap<&'static str, Entity>) {
        // this scenario folds a permanent Source modifier too, so it is a second trip of that one
        // warn_once! call site: install the capture before anything can fire
        let _logs = Logs::of_binary();
        let mut app = app();
        let mut by_id = BTreeMap::new();
        for who in order {
            by_id.insert(who, spawn(&mut app, def(who)));
        }

        let link = |app: &mut App, from: &str, to: &str, builder: EffectBuilder| {
            let handle = effect(app, builder.build());
            apply(app, by_id[from], by_id[to], &handle);
        };
        // a three-hop chain that ends back on an actor that is already upstream of it
        link(
            &mut app,
            "alpha",
            "beta",
            EffectBuilder::permanent().modify::<Armor>(
                Power::src(),
                ModOp::Add,
                EffectSubject::Target,
            ),
        );
        link(
            &mut app,
            "beta",
            "gamma",
            EffectBuilder::permanent().modify::<Armor>(
                Armor::src(),
                ModOp::Add,
                EffectSubject::Target,
            ),
        );
        link(
            &mut app,
            "gamma",
            "beta",
            EffectBuilder::permanent().modify::<Health>(
                Armor::src(),
                ModOp::Sub,
                EffectSubject::Target,
            ),
        );
        // one actor read by two partners at once
        link(
            &mut app,
            "gamma",
            "alpha",
            EffectBuilder::permanent().modify::<Mana>(
                Power::src() + Armor::src(),
                ModOp::Add,
                EffectSubject::Target,
            ),
        );
        // the clamp hop: alpha.Health only gets clamped once this arrives
        link(
            &mut app,
            "beta",
            "alpha",
            EffectBuilder::permanent()
                .modify::<MaxHealth>(Power::src(), ModOp::Sub, EffectSubject::Target)
                .modify::<MaxHealth>(10.0, ModOp::Sub, EffectSubject::Target),
        );
        // a source-subject modifier alongside the target-subject ones, and a snapshot
        link(
            &mut app,
            "alpha",
            "gamma",
            EffectBuilder::permanent().modify::<Mana>(
                Power::src(),
                ModOp::Add,
                EffectSubject::Source,
            ),
        );
        link(
            &mut app,
            "alpha",
            "beta",
            EffectBuilder::permanent().modify_snapshot::<Mana>(
                Power::src() + Armor::src(),
                ModOp::Add,
                EffectSubject::Target,
            ),
        );

        (app, by_id)
    }

    fn values(
        app: &App,
        by_id: &BTreeMap<&'static str, Entity>,
    ) -> Vec<(&'static str, &'static str, f32)> {
        let mut out = Vec::new();
        for who in ACTORS {
            let actor = by_id[who];
            for (label, read) in READS {
                out.push((who, label, read(app, actor)));
            }
        }
        out
    }

    fn visit_order(by_id: &BTreeMap<&'static str, Entity>) -> Vec<&'static str> {
        let mut by_index: Vec<(u32, &'static str)> = by_id
            .iter()
            .map(|(who, entity)| (entity.index().index(), *who))
            .collect();
        by_index.sort();
        by_index.into_iter().map(|(_, who)| who).collect()
    }

    #[test]
    fn the_dag_result_does_not_depend_on_actor_order() {
        compare_spawn_orders();
    }

    fn compare_spawn_orders() {
        let (mut forward, forward_ids) = world(ACTORS);
        let (mut backward, backward_ids) = world(["gamma", "beta", "alpha"]);

        // if both worlds walked the actors in the same order the comparison below would prove nothing
        let mut opposite_of_forward = visit_order(&forward_ids);
        opposite_of_forward.reverse();
        assert_eq!(
            visit_order(&backward_ids),
            opposite_of_forward,
            "both worlds visit the actors in the same entity order ({:?} / {:?})",
            visit_order(&forward_ids),
            visit_order(&backward_ids),
        );

        // both worlds have run the same number of updates by here, so comparing only the settled vectors
        // would let one world resolve an actor a tick later and pass
        let mut left = values(&forward, &forward_ids);
        let mut right = values(&backward, &backward_ids);
        for tick in 1..=TICKS {
            ticks(&mut forward, 1);
            ticks(&mut backward, 1);
            left = values(&forward, &forward_ids);
            right = values(&backward, &backward_ids);
            let diffs: Vec<String> = left
                .iter()
                .zip(&right)
                .filter(|((_, _, a), (_, _, b))| a != b)
                .map(|((who, label, a), (_, _, b))| {
                    format!("{who}.{label}: forward {a} != backward {b}")
                })
                .collect();
            assert!(
                diffs.is_empty(),
                "after {tick} fixed ticks, {} of {} values differ:\n{}",
                diffs.len(),
                left.len(),
                diffs.join("\n"),
            );
        }

        // a difference is only worth reporting if both sides had already settled
        ticks(&mut forward, 4);
        ticks(&mut backward, 4);
        assert_eq!(
            values(&forward, &forward_ids),
            left,
            "the forward world was still settling at tick {TICKS}"
        );
        assert_eq!(
            values(&backward, &backward_ids),
            right,
            "the backward world was still settling at tick {TICKS}"
        );

        // and the cross-actor links really are live, so agreeing on 30 numbers is not agreeing on nothing
        let wired: Vec<(&str, f32)> = [
            (
                "beta.Armor = 3 + alpha.Power",
                current::<Armor>(&forward, forward_ids["beta"]),
            ),
            (
                "gamma.Armor = 11 + beta.Armor",
                current::<Armor>(&forward, forward_ids["gamma"]),
            ),
            (
                "beta.Health = 60 - gamma.Armor",
                current::<Health>(&forward, forward_ids["beta"]),
            ),
            (
                "alpha.MaxHealth = 120 - beta.Power - 10",
                current::<MaxHealth>(&forward, forward_ids["alpha"]),
            ),
            (
                "alpha.Mana = 50 + gamma.Power + gamma.Armor",
                current::<Mana>(&forward, forward_ids["alpha"]),
            ),
            (
                "alpha.Health = clamp(110, ..=alpha.MaxHealth)",
                current::<Health>(&forward, forward_ids["alpha"]),
            ),
            (
                "gamma.Mana = 0 + alpha.Power, the source-subject link",
                current::<Mana>(&forward, forward_ids["gamma"]),
            ),
        ]
        .into_iter()
        .collect();
        assert_eq!(
            wired,
            vec![
                ("beta.Armor = 3 + alpha.Power", 13.0),
                ("gamma.Armor = 11 + beta.Armor", 24.0),
                ("beta.Health = 60 - gamma.Armor", 36.0),
                ("alpha.MaxHealth = 120 - beta.Power - 10", 105.0),
                ("alpha.Mana = 50 + gamma.Power + gamma.Armor", 81.0),
                ("alpha.Health = clamp(110, ..=alpha.MaxHealth)", 105.0),
                (
                    "gamma.Mana = 0 + alpha.Power, the source-subject link",
                    10.0
                ),
            ],
            "the world is not wired the way this test thinks it is"
        );
    }
}
