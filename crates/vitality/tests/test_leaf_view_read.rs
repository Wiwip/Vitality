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
use bevy::ecs::world::EntityRefExcept;
use bevy::prelude::*;
use common::{HarnessWorld, WorldSpec, current_values};
use std::any::{TypeId, type_name};
use vitality::attributes::Attribute;
use vitality::effect::modifier::ModifierOf;
use vitality::effect::{
    AppliedEffects, Effect, EffectDuration, EffectSource, EffectSources, EffectTarget,
    EffectTicker, EffectsRef,
};
use vitality_core::view::ActorRead;

/// One entry per attribute of the width `leaf_bench_spec` builds, in pool order, so the guard reads
/// every leaf the shape carries rather than only slot 0.
type ViewRead = fn(&EffectsRef<'_, '_>) -> Option<f64>;

const LEAF_READS: &[(&str, ViewRead)] = &[
    ("Attr0", read_attr::<common::Attr0>),
    ("Attr1", read_attr::<common::Attr1>),
    ("Attr2", read_attr::<common::Attr2>),
    ("Attr3", read_attr::<common::Attr3>),
    ("Attr4", read_attr::<common::Attr4>),
    ("Attr5", read_attr::<common::Attr5>),
    ("Attr6", read_attr::<common::Attr6>),
    ("Attr7", read_attr::<common::Attr7>),
];

/// `read_attr` for the pool's `f32` attributes, through the trait and not through `EntityRefExcept`'s
/// own inherent `get`: on a concrete view type `view.get::<T>()` picks the inherent method, which is
/// not the call `Attribute::at()` makes.
fn read_attr<T>(view: &EffectsRef<'_, '_>) -> Option<f64>
where
    T: Attribute<Property = f32>,
{
    through_view::<_, T>(view).map(|attr| attr.current_value() as f64)
}

/// The library's leaf read, and the only place this file spells it. Generic in the *view* rather than
/// in the bundle: a bare type parameter has no inherent methods, so the `get` below can only be
/// `ActorRead::get`. Spell the same read on a concrete `EntityRefExcept` and `view.get::<T>()` binds
/// bevy's inherent method, which re-resolves every member of the exclusion bundle through `Components`
/// on each call — the read L-2 replaced. Both answer `None` for a bundle member, so no value this file
/// compares could tell them apart (L-2 review, f64114df); the unfixed read is simply not nameable here.
fn through_view<V, T>(view: &V) -> Option<&T>
where
    V: ActorRead,
    T: Component,
{
    view.get::<T>()
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
        LEAF_READS.len(),
        "shape {} builds {} attributes and LEAF_READS covers {}, so this guard no longer reads \
         every attribute the benches time",
        spec.shape_id(),
        pool.len(),
        LEAF_READS.len()
    );
    for (slot, (attr, (name, _))) in pool.iter().zip(LEAF_READS).enumerate() {
        assert_eq!(
            attr.name,
            *name,
            "slot {slot} of shape {} is {}, not {name}: the table would read a different attribute \
             than the one the bench times",
            spec.shape_id(),
            attr.name
        );
    }

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
        let view = leaf::leaf_view(fixture.app.world(), &mut fixture.view, actor);
        for (slot, (name, read)) in LEAF_READS.iter().enumerate() {
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
        let view = leaf::leaf_view(world, &mut fixture.view, actor);
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
        let view = leaf::leaf_view(world, &mut fixture.view, actor);
        let mut ids = Vec::new();
        let mut carried = Vec::new();
        refused::<leaf::LeafBundle, ChildOf>(&view, world, actor, &mut ids, &mut carried);
        refused::<leaf::LeafBundle, ModifierOf>(&view, world, actor, &mut ids, &mut carried);
        refused::<leaf::LeafBundle, Effect>(&view, world, actor, &mut ids, &mut carried);
        refused::<leaf::LeafBundle, EffectDuration>(&view, world, actor, &mut ids, &mut carried);
        refused::<leaf::LeafBundle, EffectTicker>(&view, world, actor, &mut ids, &mut carried);
        refused::<leaf::LeafBundle, EffectSource>(&view, world, actor, &mut ids, &mut carried);
        refused::<leaf::LeafBundle, EffectTarget>(&view, world, actor, &mut ids, &mut carried);
        refused::<leaf::LeafBundle, AppliedEffects>(&view, world, actor, &mut ids, &mut carried);
        refused::<leaf::LeafBundle, EffectSources>(&view, world, actor, &mut ids, &mut carried);
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
    let view = leaf::leaf_view(world, &mut fixture.view, actor);
    assert!(
        through_view::<_, M>(&view).is_none(),
        "{} lives on {actor:?} and is a member of {}, so the view must answer None for it",
        type_name::<M>(),
        type_name::<B>()
    );
}

/// One member of `B` on one actor: `None` through the view if the entity carries it, its id in
/// `checked` so the caller can compare the walk with what the bundle declares, and a note in `carried`
/// so the caller can prove the loop had something to check.
fn refused<B, M>(
    view: &EntityRefExcept<'_, '_, B>,
    world: &World,
    actor: Entity,
    checked: &mut Vec<ComponentId>,
    carried: &mut Vec<&'static str>,
) where
    B: Bundle,
    M: Component,
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
        assert!(
            through_view::<_, M>(view).is_none(),
            "{} lives on {actor:?} and is a member of {}, so the view must answer None for it",
            type_name::<M>(),
            type_name::<B>()
        );
    }
}
