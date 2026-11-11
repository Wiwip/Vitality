#![allow(dead_code)]

//! P-12: what an expression costs by shape. The same sums of attributes evaluated three ways:
//! through `Arc<dyn Expr>` (what modifiers store), through the concrete node type (what a
//! monomorphic resolver would call), and as `HashMap<TypeId, f64>` lookups (the usual typemap
//! alternative to attributes-as-components). Node counts are 1, 3 and 7: one leaf, `a + b`, and
//! `(a + b) + (c + d)`.

use crate::common::{Attr0, Attr1, Attr2, Attr3, WorldSpec, actor_entities};
use bevy::prelude::*;
use express_it::expr::Expr;
use std::any::TypeId;
use std::collections::HashMap;
use std::sync::Arc;
use vitality::attributes::Attribute;
use vitality::context::{ActorExprContext, ActorExprSchema};

pub type Erased = Arc<dyn Expr<f32, ActorExprSchema> + Send + Sync>;

pub const NODE_COUNTS: [usize; 3] = [1, 3, 7];

pub struct ExprFixture {
    pub app: App,
    pub actor: Entity,
    pub erased: Vec<(usize, Erased)>,
    pub typemap: HashMap<TypeId, f64>,
}

pub fn expr_fixture() -> ExprFixture {
    let spec = WorldSpec {
        attributes_per_actor: 4,
        effects_per_actor: 0,
        modifiers_per_effect: 0,
        actors: 1,
        dirty_fraction: 0.0,
        ticking_fraction: 0.0,
    };
    let mut app = spec.build_app();
    let actor = actor_entities(app.world_mut())[0];

    let erased: Vec<(usize, Erased)> = vec![
        (1, Arc::new(Attr0::src::<ActorExprSchema>())),
        (3, Arc::new(Attr0::src::<ActorExprSchema>() + Attr1::src())),
        (
            7,
            Arc::new(
                (Attr0::src::<ActorExprSchema>() + Attr1::src())
                    + (Attr2::src::<ActorExprSchema>() + Attr3::src()),
            ),
        ),
    ];

    let world = app.world();
    let mut typemap = HashMap::new();
    typemap.insert(TypeId::of::<Attr0>(), current::<Attr0>(world, actor));
    typemap.insert(TypeId::of::<Attr1>(), current::<Attr1>(world, actor));
    typemap.insert(TypeId::of::<Attr2>(), current::<Attr2>(world, actor));
    typemap.insert(TypeId::of::<Attr3>(), current::<Attr3>(world, actor));

    ExprFixture {
        app,
        actor,
        erased,
        typemap,
    }
}

fn current<T: Attribute<Property = f32>>(world: &World, actor: Entity) -> f64 {
    world
        .get::<T>(actor)
        .map_or(0.0, |a| a.current_value() as f64)
}

/// The expected sum for `nodes`, read straight off the components.
pub fn expected(fixture: &ExprFixture, nodes: usize) -> f64 {
    let world = fixture.app.world();
    let a = current::<Attr0>(world, fixture.actor);
    let b = current::<Attr1>(world, fixture.actor);
    let c = current::<Attr2>(world, fixture.actor);
    let d = current::<Attr3>(world, fixture.actor);
    match nodes {
        1 => a,
        3 => a + b,
        _ => (a + b) + (c + d),
    }
}

pub fn eval_erased(fixture: &ExprFixture, expr: &Erased) -> f32 {
    let context = ActorExprContext {
        actor_context: fixture.app.world().entity(fixture.actor).into(),
    };
    expr.eval(&context)
}

/// The same trees without the vtable: concrete node types, built per call so nothing is boxed.
pub fn eval_mono(fixture: &ExprFixture, nodes: usize) -> f32 {
    let context = ActorExprContext {
        actor_context: fixture.app.world().entity(fixture.actor).into(),
    };
    match nodes {
        1 => Attr0::src::<ActorExprSchema>().eval(&context),
        3 => (Attr0::src::<ActorExprSchema>() + Attr1::src()).eval(&context),
        _ => ((Attr0::src::<ActorExprSchema>() + Attr1::src())
            + (Attr2::src::<ActorExprSchema>() + Attr3::src()))
        .eval(&context),
    }
}

pub fn eval_typemap(fixture: &ExprFixture, nodes: usize) -> f64 {
    let map = &fixture.typemap;
    let get = |id: TypeId| map.get(&id).copied().unwrap_or(0.0);
    let a = get(TypeId::of::<Attr0>());
    match nodes {
        1 => a,
        3 => a + get(TypeId::of::<Attr1>()),
        _ => {
            (a + get(TypeId::of::<Attr1>()))
                + (get(TypeId::of::<Attr2>()) + get(TypeId::of::<Attr3>()))
        }
    }
}
