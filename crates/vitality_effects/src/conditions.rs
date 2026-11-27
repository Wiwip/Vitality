use crate::context::{EffectExprContext, EffectExprSchema};
use crate::modifier::EffectSubject;
use crate::prelude::ActorExprSchema;
use bevy_ecs::prelude::{Component, Resource};
use bevy_reflect::Reflect;
use bevy_reflect::TypePath;
use express_it::expr::{DepKey, Deps, Expr};
use serde::Serialize;
use std::any::TypeId;
use std::fmt::Formatter;
use std::marker::PhantomData;
use std::ops::{Bound, RangeBounds};
use std::sync::atomic::{AtomicU64, Ordering};
use vitality_core::attributes::{Attribute, dep_key};
use vitality_core::context::ActorExprContext;
use vitality_core::context::{Source, Target};
use vitality_core::util::pretty_type_name;

#[derive(TypePath)]
pub struct IsAttributeWithinBounds<T: Attribute> {
    who: EffectSubject,
    bounds: (Bound<T::Property>, Bound<T::Property>),
}

impl<T: Attribute> IsAttributeWithinBounds<T> {
    pub fn new(range: impl RangeBounds<T::Property>, who: EffectSubject) -> Self {
        Self {
            who,
            bounds: (range.start_bound().cloned(), range.end_bound().cloned()),
        }
    }

    pub fn target(range: impl RangeBounds<T::Property> + Send + Sync + 'static) -> Self {
        IsAttributeWithinBounds::<T>::new(range, EffectSubject::Target)
    }

    pub fn source(range: impl RangeBounds<T::Property> + Send + Sync + 'static) -> Self {
        IsAttributeWithinBounds::<T>::new(range, EffectSubject::Source)
    }
}

impl<T: Attribute> std::fmt::Debug for IsAttributeWithinBounds<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Attribute {} on {:?} in range {:?}",
            pretty_type_name::<T>(),
            self.who,
            self.bounds
        )
    }
}

impl<T: Attribute> Expr<bool, EffectExprSchema> for IsAttributeWithinBounds<T> {
    fn render(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self}")
    }

    fn eval(&self, ctx: &EffectExprContext) -> bool {
        let opt_attribute = match self.who {
            EffectSubject::Target => match ctx.target_actor {
                Some(target) => target.get::<T>(),
                None => ctx.source_actor.get::<T>(),
            },
            EffectSubject::Source => ctx.source_actor.get::<T>(),
            EffectSubject::Effect => ctx.effect_holder.get::<T>(),
        };
        let Some(attribute) = opt_attribute else {
            return false;
        };

        self.bounds.contains(&attribute.val())
    }

    fn get_dependencies(&self, deps: &mut Deps) {
        match self.who {
            EffectSubject::Target => deps.insert(DepKey::Type(dep_key::<T, Target>())),
            EffectSubject::Source => deps.insert(DepKey::Type(dep_key::<T, Source>())),
            EffectSubject::Effect => deps.insert(DepKey::Type(TypeId::of::<T>())),
        };
    }
}

impl<T: Attribute> std::fmt::Display for IsAttributeWithinBounds<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let (start, end) = &self.bounds;

        let start_str = match start {
            Bound::Included(v) => format!("[{v}"),
            Bound::Excluded(v) => format!("]{v}"),
            Bound::Unbounded => "(-∞".to_string(),
        };

        let end_str = match end {
            Bound::Included(v) => format!("{v}]"),
            Bound::Excluded(v) => format!("{v}["),
            Bound::Unbounded => "∞)".to_string(),
        };

        write!(
            f,
            "Attribute {} on {:?} in range {}, {}",
            pretty_type_name::<T>(),
            self.who,
            start_str,
            end_str
        )
    }
}

#[derive(Serialize)]
pub struct ChanceCondition(pub f32);

impl Expr<bool, EffectExprSchema> for ChanceCondition {
    fn eval(&self, ctx: &EffectExprContext) -> bool {
        ctx.rng.next_f32() < self.0
    }

    fn render(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:.2} chance", self.0)
    }
}

/// splitmix64 stream for [`ChanceCondition`], so a run replays from its seed
#[derive(Resource, Debug)]
pub struct ConditionRng {
    state: AtomicU64,
}

impl ConditionRng {
    pub const DEFAULT_SEED: u64 = 0x5EED_0FC0_FFEE;
    const GAMMA: u64 = 0x9E37_79B9_7F4A_7C15;

    pub fn new(seed: u64) -> Self {
        Self {
            state: AtomicU64::new(seed),
        }
    }

    /// per-row stream, so a draw doesn't depend on which thread ran the row
    pub fn fork(base: u64, index: u64) -> Self {
        Self::new(Self::mix(base ^ Self::mix(index.wrapping_add(Self::GAMMA))))
    }

    pub fn next_u64(&self) -> u64 {
        let state = self
            .state
            .fetch_add(Self::GAMMA, Ordering::Relaxed)
            .wrapping_add(Self::GAMMA);
        Self::mix(state)
    }

    /// uniform in [0, 1), from the top 24 bits
    pub fn next_f32(&self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    fn mix(mut z: u64) -> u64 {
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

impl Default for ConditionRng {
    fn default() -> Self {
        Self::new(Self::DEFAULT_SEED)
    }
}

impl std::fmt::Debug for ChanceCondition {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Chance: {:.3}", self.0)
    }
}

#[derive(Serialize)]
pub struct HasComponent<C: Component> {
    pub who: EffectSubject,
    phantom_data: PhantomData<C>,
}

impl<C: Component> HasComponent<C> {
    pub fn new(target: EffectSubject) -> Self {
        Self {
            who: target,
            phantom_data: PhantomData,
        }
    }

    pub fn source() -> Self {
        Self::new(EffectSubject::Source)
    }

    pub fn target() -> Self {
        Self::new(EffectSubject::Target)
    }

    pub fn effect() -> Self {
        Self::new(EffectSubject::Effect)
    }
}

impl<C: Component + Reflect> Expr<bool, EffectExprSchema> for HasComponent<C> {
    fn render(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "has {} on {:?}", pretty_type_name::<C>(), self.who)
    }

    fn eval(&self, ctx: &EffectExprContext) -> bool {
        match self.who {
            EffectSubject::Target => match ctx.target_actor {
                Some(target) => target.get::<C>().is_some(),
                None => ctx.source_actor.get::<C>().is_some(),
            },
            EffectSubject::Source => ctx.source_actor.get::<C>().is_some(),
            EffectSubject::Effect => ctx.effect_holder.get::<C>().is_some(),
        }
    }

    fn get_dependencies(&self, deps: &mut Deps) {
        deps.insert(DepKey::Type(TypeId::of::<C>()));
    }
}

impl<C: Component> Expr<bool, ActorExprSchema> for HasComponent<C> {
    fn render(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "has {} on {:?}", pretty_type_name::<C>(), self.who)
    }

    fn eval(&self, ctx: &ActorExprContext) -> bool {
        match self.who {
            EffectSubject::Target => false,
            EffectSubject::Source => ctx.actor_context.get::<C>().is_some(),
            EffectSubject::Effect => false,
        }
    }

    fn get_dependencies(&self, deps: &mut Deps) {
        deps.insert(DepKey::Type(TypeId::of::<C>()));
    }
}

impl<C: Component> std::fmt::Debug for HasComponent<C> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Has Tag {} on {}", pretty_type_name::<C>(), self.who)
    }
}
