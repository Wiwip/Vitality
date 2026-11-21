use crate::context::EffectExprSchema;
use crate::modifier::Modifier;
use crate::modifier::ModifierFn;
use crate::{EffectApplicationPolicy, EffectStackingPolicy};
use bevy_reflect::prelude::*;
use express_it::expr::StoredExpr;
use vitality_core::mutator::EntityActions;
use vitality_core::tags::TagSpec;

#[derive(TypePath)]
pub struct EffectDef {
    pub application_policy: EffectApplicationPolicy,
    pub stacking_policy: EffectStackingPolicy,
    pub effect_fn: Vec<Box<ModifierFn>>,
    pub modifiers: Vec<Box<dyn Modifier>>,

    pub attach_conditions: Vec<StoredExpr<bool, EffectExprSchema>>,
    pub activate_conditions: Vec<StoredExpr<bool, EffectExprSchema>>,

    pub on_actor_triggers: Vec<EntityActions>,
    pub on_effect_triggers: Vec<EntityActions>,

    /// Granted to the target while the effect exists, and inserted on the effect entity.
    pub granted_tags: Vec<TagSpec>,
    /// The target must carry all of these for the effect to apply.
    pub required_tags: Vec<TagSpec>,
    /// Any of these on the target blocks the effect.
    pub blocked_by_tags: Vec<TagSpec>,
    /// Applying the effect removes the target's effects tagged with any of these.
    pub cleanses: Vec<TagSpec>,
}
