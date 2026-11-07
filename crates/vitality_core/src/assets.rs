use crate::context::{AbilityExprSchema, EffectExprSchema};
use crate::effect::{EffectApplicationPolicy, EffectStackingPolicy};
use crate::modifier::ModifierFn;
use crate::modifier::modifier::Modifier;
use crate::mutator::EntityActions;
use crate::prelude::ActorExprSchema;
use bevy::prelude::*;
use express_it::expr::{BoolExpr, StoredExpr};
use express_it::plan::Plan;

#[derive(Asset, TypePath)]
pub struct EffectDef {
    pub application_policy: EffectApplicationPolicy,
    pub stacking_policy: EffectStackingPolicy,
    pub effect_fn: Vec<Box<ModifierFn>>,
    pub modifiers: Vec<Box<dyn Modifier>>,

    pub attach_conditions: Vec<StoredExpr<bool, EffectExprSchema>>,
    pub activate_conditions: Vec<StoredExpr<bool, EffectExprSchema>>,

    pub on_actor_triggers: Vec<EntityActions>,
    pub on_effect_triggers: Vec<EntityActions>,
}

#[derive(Asset, TypePath)]
pub struct AbilityDef {
    pub name: String,
    pub description: String,

    pub mutators: Vec<EntityActions>,
    pub observers: Vec<EntityActions>,

    pub execution_conditions: Vec<StoredExpr<bool, ActorExprSchema>>,

    pub cost_condition: Vec<StoredExpr<bool, AbilityExprSchema>>,
    pub cost_modifiers: Plan<AbilityExprSchema>,
    pub cooldown: StoredExpr<f32, AbilityExprSchema>,

    pub task_scene: Box<dyn Fn() -> Box<dyn Scene> + Send + Sync>,

    pub recovery_condition: Vec<BoolExpr<ActorExprSchema>>,

    pub on_execute: Vec<Plan<AbilityExprSchema>>,
}
