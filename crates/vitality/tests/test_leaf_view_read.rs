//! L-2 guard: the leaf read through the public view — `ActorRead::get`, the call `Attribute::at()`
//! resolves to — hands back the attribute it is asked for, for every attribute of the width the
//! `leaf/*` benches build, for every actor in harness order, and at every exclusion-bundle width
//! `leaf/exclusion_length` sweeps. Making the read cheaper is allowed to change what it pays for; it
//! is not allowed to change which component comes back, or to start handing back a component the
//! bundle excludes.

#[path = "../benches/support/common.rs"]
mod common;
#[path = "../benches/support/leaf.rs"]
mod leaf;

use bevy::ecs::bundle::Bundle;
use bevy::ecs::component::{Component, ComponentId};
use bevy::ecs::query::QueryState;
use bevy::ecs::world::EntityRefExcept;
use bevy::prelude::*;
use common::{HarnessWorld, WorldSpec, current_values};
use std::any::{TypeId, type_name};
use vitality::attributes::Attribute;
use vitality::effect::modifier::ModifierOf;
use vitality::effect::{
    AppliedEffects, Effect, EffectDuration, EffectSource, EffectSources, EffectTarget, EffectTicker,
};
use vitality_core::view::ActorRead;

/// One read per attribute of the width `leaf_bench_spec` builds, so the guard covers every leaf the
/// shape carries rather than only slot 0.
const LEAF_READ_COUNT: usize = 8;

/// generic in the view so every `get` here can only be `ActorRead::get`
fn leaf_reads<V: ActorRead>() -> [(&'static str, fn(&V) -> Option<f64>); LEAF_READ_COUNT] {
    [
        ("Attr0", read_attr::<V, common::Attr0>),
        ("Attr1", read_attr::<V, common::Attr1>),
        ("Attr2", read_attr::<V, common::Attr2>),
        ("Attr3", read_attr::<V, common::Attr3>),
        ("Attr4", read_attr::<V, common::Attr4>),
        ("Attr5", read_attr::<V, common::Attr5>),
        ("Attr6", read_attr::<V, common::Attr6>),
        ("Attr7", read_attr::<V, common::Attr7>),
    ]
}

/// One pool attribute, read through the trait: generic in the view like [`through_view`], so it cannot
/// pick up `EntityRefExcept`'s inherent `get`.
fn read_attr<V, T>(view: &V) -> Option<f64>
where
    V: ActorRead,
    T: Attribute<Property = f32>,
{
    through_view::<_, T>(view).map(|attr| attr.current_value() as f64)
}

/// hides the concrete view so the inherent `EntityRefExcept::get` can't be spelled at a call site
fn pinned_view<'w, 's, B: Bundle>(
    world: &'w World,
    query: &'s mut QueryState<EntityRefExcept<'static, 'static, B>>,
    actor: Entity,
) -> impl ActorRead + 's
where
    'w: 's,
{
    leaf::leaf_view(world, query, actor)
}

/// generic so `view.get` can only bind the trait `get`
fn through_view<V, T>(view: &V) -> Option<&T>
where
    V: ActorRead,
    T: Component,
{
    view.get::<T>()
}

/// the one exclusion check; only fed a pinned view
fn assert_refused<B, M, V>(view: &V, actor: Entity)
where
    B: Bundle,
    M: Component,
    V: ActorRead,
{
    assert!(
        through_view::<_, M>(view).is_none(),
        "{} lives on {actor:?} and is a member of {}, so the view must answer None for it",
        type_name::<M>(),
        type_name::<B>()
    );
}

#[test]
fn leaf_view_read_hands_back_the_attribute_it_names() {
    for spec in [leaf::leaf_bench_spec(), leaf::leaf_sweep_spec()] {
        every_attribute_agrees(&spec);
    }

    let spec = leaf::leaf_sweep_spec();
    every_width_agrees::<leaf::Excl0>(&spec, 0);
    every_width_agrees::<leaf::Excl5>(&spec, 5);
    every_width_agrees::<leaf::Excl20>(&spec, 20);
    every_width_agrees::<leaf::Excl40>(&spec, 40);

    // The other half of the pin. Every check above reads a component the bundle does not exclude, so
    // a read that got cheaper by dropping the exclusion would pass all of them.
    bundle_members_stay_unreadable(&spec);
    inserted_member_stays_unreadable::<leaf::Excl5, leaf::Ex0>(&spec, leaf::Ex0);
}

/// Every attribute of one shape, every actor, against the harness's own live value.
fn every_attribute_agrees(spec: &WorldSpec) {
    let mut fixture = leaf::leaf_fixture(spec);
    let pool = fixture.app.world().resource::<HarnessWorld>().attrs();
    assert_eq!(
        pool.len(),
        LEAF_READ_COUNT,
        "shape {} builds {} attributes and the read table covers {}, so this guard no longer reads \
         every attribute the benches time",
        spec.shape_id(),
        pool.len(),
        LEAF_READ_COUNT
    );

    let actors = fixture.actors.clone();
    let probe = actors[0];
    let values = current_values(fixture.app.world(), pool, probe);
    // Teeth: two attributes sharing a value would let an off-by-one hand-back compare equal.
    for (left, left_value) in values.iter().enumerate() {
        for (right, right_value) in values.iter().enumerate() {
            assert!(
                left == right || left_value != right_value,
                "attributes {left} and {right} of shape {} share current_value {left_value} on \
                 {probe:?}, so reading a neighbour's component could not be told from the right one",
                spec.shape_id()
            );
        }
    }
    let resolved = pool.iter().any(|attr| {
        attr.base(fixture.app.world(), probe).unwrap()
            != attr.current(fixture.app.world(), probe).unwrap()
    });
    assert!(
        resolved,
        "shape {} resolved nothing on {probe:?}: every attribute still reads its base_value, so \
         the reads below agree on a pending value",
        spec.shape_id()
    );

    for &actor in actors.iter() {
        let expected = current_values(fixture.app.world(), pool, actor);
        let view = pinned_view(fixture.app.world(), &mut fixture.view, actor);
        let reads = leaf_reads();
        // The table has to be the one the reads below run, so the order check sits here with them.
        for (slot, (attr, (name, _))) in pool.iter().zip(reads.iter()).enumerate() {
            assert_eq!(
                attr.name,
                *name,
                "slot {slot} of shape {} is {}, not {name}: the table would read a different \
                 attribute than the one the bench times",
                spec.shape_id(),
                attr.name
            );
        }
        for (slot, (name, read)) in reads.iter().enumerate() {
            let got = read(&view)
                .unwrap_or_else(|| panic!("{name} is not readable through the view on {actor:?}"));
            assert_eq!(
                got,
                expected[slot],
                "{name} read {got} through the view, harness says {} on {actor:?} of shape {}",
                expected[slot],
                spec.shape_id()
            );
        }
    }
}

/// One width of `leaf/exclusion_length`: the leaf attribute read through the view of a bundle the
/// library does not own, on every actor.
fn every_width_agrees<B: Bundle + leaf::Arity>(spec: &WorldSpec, width: usize) {
    let name = type_name::<B>();
    let mut fixture = leaf::excl_fixture::<B>(spec);
    let world = fixture.app.world();
    let pool = world.resource::<HarnessWorld>().attrs();
    assert_eq!(
        fixture.excl, width,
        "{name}: this world registers {} of the {width} members the bundle declares, so the read \
         does not scan the width its id carries. Build the view before measuring the width.",
        fixture.excl
    );

    for &actor in fixture.actors.iter() {
        let expected = current_values(world, pool, actor)[leaf::LEAF_SLOT as usize];
        let view = pinned_view(world, &mut fixture.view, actor);
        let got = through_view::<_, leaf::LeafAttr>(&view).unwrap_or_else(|| {
            panic!(
                "{} excludes the leaf attribute on {actor:?}",
                type_name::<B>()
            )
        });
        assert_eq!(
            got.current_value() as f64,
            expected,
            "{name} read {} through the view, harness says {expected} on {actor:?}",
            got.current_value(),
        );
    }
}

/// The bundle members that really live on an actor: the view has to answer `None` for each of them,
/// exactly as it did while the read scanned the bundle per call.
fn bundle_members_stay_unreadable(spec: &WorldSpec) {
    let mut fixture = leaf::excl_fixture::<leaf::LeafBundle>(spec);
    let registered = fixture.excl;
    for &actor in fixture.actors.iter() {
        let world = fixture.app.world();
        let view = pinned_view(world, &mut fixture.view, actor);
        let mut ids = Vec::new();
        let mut carried = Vec::new();
        refused::<leaf::LeafBundle, ChildOf, _>(&view, world, actor, &mut ids, &mut carried);
        refused::<leaf::LeafBundle, ModifierOf, _>(&view, world, actor, &mut ids, &mut carried);
        refused::<leaf::LeafBundle, Effect, _>(&view, world, actor, &mut ids, &mut carried);
        refused::<leaf::LeafBundle, EffectDuration, _>(&view, world, actor, &mut ids, &mut carried);
        refused::<leaf::LeafBundle, EffectTicker, _>(&view, world, actor, &mut ids, &mut carried);
        refused::<leaf::LeafBundle, EffectSource, _>(&view, world, actor, &mut ids, &mut carried);
        refused::<leaf::LeafBundle, EffectTarget, _>(&view, world, actor, &mut ids, &mut carried);
        refused::<leaf::LeafBundle, AppliedEffects, _>(&view, world, actor, &mut ids, &mut carried);
        refused::<leaf::LeafBundle, EffectSources, _>(&view, world, actor, &mut ids, &mut carried);
        ids.sort();
        // The list above is hand-written and only `AppliedEffects` and `EffectSources` of its nine
        // members are carried by an actor, so the checks it runs cannot notice a dropped line. The
        // oracle has to come from the bundle type, which is what the view actually excludes.
        let mut declared: Vec<ComponentId> =
            <leaf::LeafBundle as Bundle>::get_component_ids(world.components())
                .flatten()
                .collect();
        declared.sort();
        assert_eq!(
            ids,
            declared,
            "{}: this loop walked {} component(s), the bundle declares {} ({registered} of them \
             registered in this world), so a member was dropped from or swapped into the list above \
             and the guard kept printing green for the members nobody read",
            type_name::<leaf::LeafBundle>(),
            ids.len(),
            declared.len(),
        );
        assert!(
            !carried.is_empty(),
            "no member of {} lives on {actor:?} of shape {}, so this cannot tell a view that \
             excludes its bundle from one that reads everything",
            type_name::<leaf::LeafBundle>(),
            spec.shape_id()
        );
    }
}

/// `M` inserted by hand onto one actor of `B`: these markers reach the world only through the query
/// build, which is the case a per-call scan gets right by re-resolving the bundle every time.
fn inserted_member_stays_unreadable<B, M>(spec: &WorldSpec, marker: M)
where
    B: Bundle,
    M: Component,
{
    let mut fixture = leaf::excl_fixture::<B>(spec);
    let actor = fixture.actor;
    fixture.app.world_mut().entity_mut(actor).insert(marker);
    let world = fixture.app.world();
    assert!(
        world.get::<M>(actor).is_some(),
        "{name} never landed on {actor:?}, so nothing below can show it is excluded",
        name = type_name::<M>()
    );
    let view = pinned_view(world, &mut fixture.view, actor);
    assert_refused::<B, M, _>(&view, actor);
}

/// One member of `B` on one actor: `None` through the view if the entity carries it, its id in
/// `checked` so the caller can compare the walk with what the bundle declares, and a note in `carried`
/// so the caller can prove the loop had something to check.
fn refused<B, M, V>(
    view: &V,
    world: &World,
    actor: Entity,
    checked: &mut Vec<ComponentId>,
    carried: &mut Vec<&'static str>,
) where
    B: Bundle,
    M: Component,
    V: ActorRead,
{
    let id = world.components().get_id(TypeId::of::<M>()).unwrap_or_else(|| {
        panic!(
            "{} is named as a member of {} but has no `ComponentId` in this world, so the check below \
             would compare nothing: every state this repo builds registers its bundle members \
             (`EntityRefExcept::init_state`), so a missing id here is that contract breaking",
            type_name::<M>(),
            type_name::<B>(),
        )
    });
    checked.push(id);
    if world.get::<M>(actor).is_some() {
        carried.push(type_name::<M>());
        assert_refused::<B, M, V>(view, actor);
    }
}
