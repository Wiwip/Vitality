use crate::context::EffectExprSchema;
use crate::modifier::Modifier;
use crate::modifier::ModifierFn;
use crate::{EffectApplicationPolicy, EffectStackingPolicy};
use bevy::prelude::*;
use express_it::expr::StoredExpr;
use vitality_core::mutator::EntityActions;

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
