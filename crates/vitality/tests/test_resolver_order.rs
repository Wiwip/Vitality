//! R-5: the resolver is parallel *across* actors only, so rebuilding the same world with the actors
//! spawned in the opposite order has to land on the same numbers.
//!
//! `benches/support/common.rs::actor_entities` sorts by `Entity`, which is exactly what hides an
//! ordering effect, so nothing here keys on spawn order: every read goes through the [`Who`]
//! identity component this test puts on each actor.

mod gameplay;

use bevy::prelude::*;
use gameplay::*;
use std::collections::BTreeMap;
use vitality::actors::ActorBuilder;
use vitality::effect::modifier::{EffectSubject, ModOp};
use vitality::prelude::*;

/// Spawn-order-independent identity, so the two worlds can be compared actor by actor.
#[derive(Component, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
struct Who(&'static str);

/// Compared in this order, never in spawn order.
const ACTORS: [&str; 3] = ["alpha", "beta", "gamma"];

/// The deepest chain in [`world`] is three cross-actor hops (alpha.Power -> beta.Armor ->
/// gamma.Armor -> beta.Health) plus the clamp hop into alpha.Health, and v1 settles one hop per fixed
/// tick, so eight ticks leaves room on either resolver. The asserts after the comparison check that
/// instead of trusting it.
const TICKS: usize = 8;

/// Every value the comparison reads, current and base.
const READS: [(&str, fn(&App, Entity) -> f32); 10] = [
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

/// Builds the world with `order` as the spawn order. The seven links are always applied in the same
/// identity order, so spawn order is the only difference between two calls.
fn world(order: [&'static str; 3]) -> (App, BTreeMap<&'static str, Entity>) {
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
        EffectBuilder::permanent().modify::<Armor>(Power::src(), ModOp::Add, EffectSubject::Target),
    );
    link(
        &mut app,
        "beta",
        "gamma",
        EffectBuilder::permanent().modify::<Armor>(Armor::src(), ModOp::Add, EffectSubject::Target),
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
        EffectBuilder::permanent().modify::<Mana>(Power::src(), ModOp::Add, EffectSubject::Source),
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

/// Actor by actor in [`ACTORS`] order, so the two vectors line up without trusting entity order.
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

/// Actors in entity-index order, which is the order a query walks them in.
fn visit_order(by_id: &BTreeMap<&'static str, Entity>) -> Vec<&'static str> {
    let mut by_index: Vec<(u32, &'static str)> = by_id
        .iter()
        .map(|(who, entity)| (entity.index().index(), *who))
        .collect();
    by_index.sort();
    by_index.into_iter().map(|(_, who)| who).collect()
}

#[test]
fn the_result_does_not_depend_on_actor_order() {
    let resolver = mode();
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

    ticks(&mut forward, TICKS);
    ticks(&mut backward, TICKS);
    let left = values(&forward, &forward_ids);
    let right = values(&backward, &backward_ids);

    let diffs: Vec<String> = left
        .iter()
        .zip(&right)
        .filter(|((_, _, a), (_, _, b))| a != b)
        .map(|((who, label, a), (_, _, b))| format!("{who}.{label}: forward {a} != backward {b}"))
        .collect();
    assert!(
        diffs.is_empty(),
        "after {TICKS} fixed ticks on {resolver:?}, {} of {} values differ:\n{}",
        diffs.len(),
        left.len(),
        diffs.join("\n"),
    );

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
