use crate::assets::AbilityDef;
use crate::attributes::Attribute;
use crate::context::{AbilityExprContext, AbilityExprSchema, EffectExprContext, EffectExprSchema};
use crate::inspector::pretty_type_name;
use crate::modifier::EffectSubject;
use bevy::asset::AssetId;
use bevy::prelude::{Component, TypePath};
use bevy::reflect::Reflect;
use express_it::context::{Path, ReadContext};
use express_it::expr::{Expr, ExprNode, ExpressionError};
use express_it::logic::{BoolExpr, BoolExprNode};
use serde::Serialize;
use std::collections::HashSet;
use std::fmt::Formatter;
use std::marker::PhantomData;
use std::ops::{Bound, RangeBounds};
use std::sync::Arc;
use crate::ability::Ability;

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

impl<T: Attribute> ExprNode<bool, EffectExprSchema> for IsAttributeWithinBounds<T> {
    fn eval(&self, ctx: &EffectExprContext) -> Result<bool, ExpressionError> {
        let type_name = pretty_type_name::<T>();
        let full_path = Path::new(format!("{}.{}.base_value", self.who, type_name));

        let any = ctx.get_any(&full_path)?;
        let value = any.downcast_ref::<T::Property>().unwrap();

        Ok(self.bounds.contains(&value))
    }

    fn eval_dyn(&self, _ctx: &dyn ReadContext) -> Result<bool, ExpressionError> {
        todo!()
    }

    fn get_dependencies(&self, _deps: &mut HashSet<Path>) {
        let type_name = pretty_type_name::<T>();
        _deps.insert(Path::new(type_name));
    }
}

impl<T: Attribute> Into<BoolExpr<EffectExprSchema>> for IsAttributeWithinBounds<T> {
    fn into(self) -> BoolExpr<EffectExprSchema> {
        let node = BoolExprNode::Boxed(Box::new(self));
        Expr::new(Arc::new(node))
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

impl ExprNode<bool, EffectExprSchema> for ChanceCondition {
    fn eval(&self, _: &EffectExprContext) -> Result<bool, ExpressionError> {
        Ok(rand::random::<f32>() < self.0)
    }

    fn eval_dyn(&self, _ctx: &dyn ReadContext) -> Result<bool, ExpressionError> {
        todo!()
    }

    fn get_dependencies(&self, _deps: &mut HashSet<Path>) {}
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

impl<C: Component + Reflect> ExprNode<bool, EffectExprSchema> for HasComponent<C> {
    fn eval(&self, ctx: &EffectExprContext) -> Result<bool, ExpressionError> {
        let path = Path::new(format!("source.{}", pretty_type_name::<C>()));
        let any = ctx.get_any(&path);
        Ok(any.is_ok())
    }

    fn eval_dyn(&self, ctx: &dyn ReadContext) -> Result<bool, ExpressionError> {
        let path = Path::new(format!("source.{}", pretty_type_name::<C>()));
        let any = ctx.get_any(&path);
        Ok(any.is_ok())
    }

    fn get_dependencies(&self, deps: &mut HashSet<Path>) {
        let type_name = pretty_type_name::<C>();
        deps.insert(Path::new(type_name));
    }
}

impl<C: Component + Reflect> ExprNode<bool, AbilityExprSchema> for HasComponent<C> {
    fn eval(&self, ctx: &AbilityExprContext) -> Result<bool, ExpressionError> {
        let path = Path::new(format!("ability.{}", pretty_type_name::<C>()));
        let any = ctx.get_any(&path);
        Ok(any.is_ok())
    }

    fn eval_dyn(&self, ctx: &dyn ReadContext) -> Result<bool, ExpressionError> {
        let path = Path::new(format!("ability.{}", pretty_type_name::<C>()));
        let any = ctx.get_any(&path);
        Ok(any.is_ok())
    }

    fn get_dependencies(&self, _deps: &mut HashSet<Path>) {
        let type_name = pretty_type_name::<C>();
        _deps.insert(Path::new(type_name));
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

impl ExprNode<bool, AbilityExprSchema> for IsAbility {
    fn eval(&self, ctx: &AbilityExprContext) -> Result<bool, ExpressionError> {
        let path = Path::new("ability.Ability");

        let any = ctx.get_any(&path)?;
        let value = any.downcast_ref::<Ability>().unwrap();
        
        Ok(value.0.id() == self.asset)
    }

    fn eval_dyn(&self, _ctx: &dyn ReadContext) -> Result<bool, ExpressionError> {
        todo!()
    }

    fn get_dependencies(&self, _deps: &mut HashSet<Path>) {}
}

impl std::fmt::Debug for IsAbility {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Is Ability {}", self.asset)
    }
}
