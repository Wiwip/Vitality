use crate::context::AbilityExprSchema;
use bevy::prelude::*;
use express_it::expr::{BoolExpr, StoredExpr};
use express_it::plan::Plan;
use vitality_core::context::ActorExprSchema;
use vitality_core::mutator::EntityActions;

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
