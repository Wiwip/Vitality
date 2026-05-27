use crate::context::{AbilityExprSchema, EffectExprSchema};
use crate::effect::{EffectApplicationPolicy, EffectStackingPolicy};
use crate::modifier::ModifierFn;
use crate::modifier::modifier::Modifier;
use crate::mutator::EntityActions;
use bevy::prelude::*;
use express_it::frame::LazyPlan;
use express_it::logic::BoolExpr;
use smol_str::SmolStr;
use std::any::Any;
use std::collections::{HashMap, VecDeque};

#[derive(Asset, TypePath)]
pub struct ActorDef {
    pub name: String,
    pub description: String,
    pub builder_actions: VecDeque<EntityActions>,
    pub abilities: Vec<Handle<AbilityDef>>,
    pub effects: Vec<Handle<EffectDef>>,

    // The value below is hidden behind 'Any' but actually:
    // Box<(Expr<T::Property>, Expr<T::Property>)>
    pub clamp_exprs: HashMap<SmolStr, Box<dyn Any + Send + Sync>>,
    pub clamp_reverse_lookup: HashMap<SmolStr, Vec<SmolStr>>,
}

#[derive(Asset, TypePath)]
pub struct EffectDef {
    pub application_policy: EffectApplicationPolicy,
    pub stacking_policy: EffectStackingPolicy,
    pub effect_fn: Vec<Box<ModifierFn>>,
    pub modifiers: Vec<Box<dyn Modifier>>,

    pub attach_conditions: Vec<BoolExpr<EffectExprSchema>>,
    pub activate_conditions: Vec<BoolExpr<EffectExprSchema>>,

    pub on_actor_triggers: Vec<EntityActions>,
    pub on_effect_triggers: Vec<EntityActions>,
}

#[derive(Asset, TypePath)]
pub struct AbilityDef {
    pub name: String,
    pub description: String,

    pub mutators: Vec<EntityActions>,
    pub observers: Vec<EntityActions>,

    pub execution_conditions: Vec<BoolExpr<AbilityExprSchema>>,

    pub cost_condition: Vec<BoolExpr<AbilityExprSchema>>,
    pub cost_modifiers: LazyPlan,

    pub scene: Box<dyn Fn() -> Box<dyn Scene> + Send + Sync>,

    pub on_execute: Vec<LazyPlan>,
}
