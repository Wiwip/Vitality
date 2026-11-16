use crate::actors::{Actor, ActorDef};
use bevy::prelude::*;
use std::any::TypeId;
use std::marker::PhantomData;
use vitality_core::CurrentValueChanged;
use vitality_core::attributes::{Attribute, ClampBounds, dep_key};
use vitality_core::context::{ActorExprContext, Source};
use vitality_core::modifier::AttributeCalculatorCached;

pub use vitality_core::attributes::Clamp;

/// An attribute a clamp of `T` reads changed, so its limits must be re-evaluated.
#[derive(Component)]
#[component(storage = "SparseSet")]
pub struct ClampStale<T>(PhantomData<T>);

impl<T> Default for ClampStale<T> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

pub(crate) fn mark_clamp_stale<T: Attribute>(entity: &mut EntityCommands) {
    entity.try_insert(ClampStale::<T>::default());
}

/// Marks the clamps that read `D` on this actor stale.
pub fn clamp_dependency_changed<D: Attribute>(
    trigger: On<CurrentValueChanged<D>>,
    actors: Query<&Actor>,
    actor_assets: Res<Assets<ActorDef>>,
    mut commands: Commands,
) {
    let Ok(actor) = actors.get(trigger.entity) else {
        return;
    };
    let Some(def) = actor_assets.get(&actor.0) else {
        return;
    };
    let mut entity = commands.entity(trigger.entity);
    for key in [dep_key::<D, Source>(), TypeId::of::<D>()] {
        for (_, mark) in def.clamp_dependents.get(&key).into_iter().flatten() {
            mark(&mut entity);
        }
    }
}

/// The three queries `apply_clamps` takes as one `ParamSet`; spelled out they sit over clippy's limit.
type ClampBatch<'w, 's, T> = (
    Query<
        'w,
        's,
        Entity,
        (
            With<Clamp<T>>,
            Or<(Changed<T>, Added<Clamp<T>>, With<ClampStale<T>>)>,
        ),
    >,
    Query<'w, 's, EntityRef<'w>>,
    Query<
        'w,
        's,
        (
            &'w mut T,
            &'w AttributeCalculatorCached<T>,
            &'w mut Clamp<T>,
        ),
    >,
);

/// The re-evaluated limits of one pass, one row per candidate actor.
type ClampLimits<T> = Vec<(
    Entity,
    <T as Attribute>::Property,
    <T as Attribute>::Property,
)>;

/// Re-evaluates stale limits, then clamps base and current value of `T`.
pub fn apply_clamps<T: Attribute>(
    mut set: ParamSet<ClampBatch<'_, '_, T>>,
    mut commands: Commands,
    mut candidates: Local<Vec<Entity>>,
    mut limits: Local<ClampLimits<T>>,
) {
    candidates.clear();
    candidates.extend(set.p0().iter());
    if candidates.is_empty() {
        return;
    }

    limits.clear();
    {
        let actors = set.p1();
        for &entity in candidates.iter() {
            let Ok(attribute_ref) = actors.get(entity) else {
                continue;
            };
            let Some(bounds) = attribute_ref.get::<ClampBounds<T>>() else {
                continue;
            };
            let context = ActorExprContext {
                actor_context: attribute_ref.into(),
            };
            limits.push((entity, bounds.min.eval(&context), bounds.max.eval(&context)));
        }
    }

    let mut attributes = set.p2();
    for &(entity, min, max) in limits.iter() {
        commands.entity(entity).try_remove::<ClampStale<T>>();
        let Ok((mut attribute, cache, mut clamp)) = attributes.get_mut(entity) else {
            continue;
        };
        if !clamp.ready || clamp.min_limit != min || clamp.max_limit != max {
            clamp.min_limit = min;
            clamp.max_limit = max;
            clamp.ready = true;
        }

        let base = attribute.base_value();
        let clamped_base = clamp.apply(base);
        if clamped_base != base {
            attribute.set_base_value(clamped_base);
        }
        let current = clamp.apply(cache.calculator.eval(attribute.base_value()));
        if current != attribute.current_value() {
            attribute.set_current_value(current);
        }
    }
}
