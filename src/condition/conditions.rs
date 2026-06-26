use crate::ability::Ability;
use crate::assets::AbilityDef;
use crate::attributes::Attribute;
use crate::context::{AbilityExprContext, AbilityExprSchema, EffectExprContext, EffectExprSchema};
use crate::inspector::pretty_type_name;
use crate::modifier::EffectSubject;
use bevy::asset::AssetId;
use bevy::prelude::{Component, TypePath};
use bevy::reflect::Reflect;
use express_it::expr::Expr;
use serde::Serialize;
use std::any::TypeId;
use std::collections::HashSet;
use std::fmt::Formatter;
use std::marker::PhantomData;
use std::ops::{Bound, RangeBounds};

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

    fn get_dependencies(&self, deps: &mut HashSet<TypeId>) {
        deps.insert(TypeId::of::<T>());
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
    fn eval(&self, _: &EffectExprContext) -> bool {
        rand::random::<f32>() < self.0
    }
}

impl std::fmt::Debug for ChanceCondition {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Chance: {:.3}", self.0)
    }
}

#[derive(Serialize)]
pub struct HasComponent<C: Component> {
    who: EffectSubject,
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

    fn get_dependencies(&self, deps: &mut HashSet<TypeId>) {
        deps.insert(TypeId::of::<C>());
    }
}

impl<C: Component + Reflect> Expr<bool, AbilityExprSchema> for HasComponent<C> {
    fn eval(&self, ctx: &AbilityExprContext) -> bool {
        match self.who {
            EffectSubject::Target => match ctx.target_ref {
                Some(target) => target.get::<C>().is_some(),
                None => ctx.caster_ref.get::<C>().is_some(),
            },
            EffectSubject::Source => ctx.caster_ref.get::<C>().is_some(),
            EffectSubject::Effect => ctx.ability_ref.get::<C>().is_some(),
        }
    }

    fn get_dependencies(&self, deps: &mut HashSet<TypeId>) {
        deps.insert(TypeId::of::<C>());
    }
}

impl<C: Component> std::fmt::Debug for HasComponent<C> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Has Tag {} on {}", pretty_type_name::<C>(), self.who)
    }
}

pub struct IsAbility {
    asset: AssetId<AbilityDef>,
}

impl IsAbility {
    pub fn new(asset: AssetId<AbilityDef>) -> Self {
        Self { asset }
    }
}

impl Expr<bool, AbilityExprSchema> for IsAbility {
    fn eval(&self, ctx: &AbilityExprContext) -> bool {
        let ability = match ctx.ability_ref.get::<Ability>() {
            Some(ability) => ability,
            _ => {
                return false;
            }
        };

        ability.0.id() == self.asset
    }
    fn get_dependencies(&self, _deps: &mut HashSet<TypeId>) {}
}

impl std::fmt::Debug for IsAbility {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Is Ability {}", self.asset)
    }
}
