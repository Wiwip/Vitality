use crate::context::AbilityExprSchema;
use bevy::prelude::*;
use express_it::expr::StoredExpr;
use express_it::plan::Plan;
use vitality_core::context::ActorExprSchema;
use vitality_core::mutator::EntityActions;
use vitality_core::tags::TagSpec;

#[derive(Asset, TypePath)]
pub struct AbilityDef {
    pub name: String,

    pub mutators: Vec<EntityActions>,
    pub observers: Vec<EntityActions>,

    pub execution_conditions: Vec<StoredExpr<bool, ActorExprSchema>>,

    pub cost_condition: Vec<StoredExpr<bool, AbilityExprSchema>>,
    pub cost_modifiers: Plan<AbilityExprSchema>,
    pub cooldown: StoredExpr<f32, AbilityExprSchema>,

    pub task_scene: Box<dyn Fn() -> Box<dyn Scene> + Send + Sync>,

    pub on_execute: Vec<Plan<AbilityExprSchema>>,

    /// Inserted on the ability entity; what other abilities block or cancel by.
    pub tags: Vec<TagSpec>,
    /// Granted to the caster while the ability is active.
    pub activation_owned_tags: Vec<TagSpec>,
    pub activation_requires: Vec<TagSpec>,
    pub activation_blocked_by: Vec<TagSpec>,
    /// While active, the caster's abilities with these tags can't activate.
    pub blocks_abilities_with: Vec<TagSpec>,
    /// Activating cancels the caster's active abilities with these tags.
    pub cancels_abilities_with: Vec<TagSpec>,
}
