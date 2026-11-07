use crate::context::EffectExprSchema;
use crate::effect::{EffectApplicationPolicy, EffectStackingPolicy};
use crate::modifier::ModifierFn;
use crate::modifier::modifier::Modifier;
use crate::mutator::EntityActions;
use bevy::prelude::*;
use express_it::expr::StoredExpr;

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
