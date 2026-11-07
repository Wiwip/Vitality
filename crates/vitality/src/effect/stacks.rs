use crate::assets::EffectDef;
use crate::attribute;
use crate::effect::Effect;
use crate::effect::timing::EffectDuration;
use crate::prelude::Attribute;
use crate::{ReflectAccessAttribute, attribute_impl};
use bevy::prelude::*;
use num_traits::AsPrimitive;

pub enum EffectStackingPolicy {
    None, // Each effect is independently added to the entity
    Add { count: u32, max_stack: u32 },
    RefreshDuration, // The effect overrides previous applications
                     //RefreshDurationWithOverflow, // The effect overrides previous applications
}
attribute!(EffectIntensity, f32);

attribute_impl!(Stacks, u32);

impl Default for Stacks {
    fn default() -> Self {
        Stacks::new(1) // By default, a new effect has 1 stack
    }
}

impl Stacks {
    /// Applies the appropriate stacking policy to an effect
    pub fn apply_stacking_policy(
        policy: &EffectStackingPolicy,
        effect_entity: Entity,
        stacks: &mut Query<&mut Stacks, With<Effect>>,
        durations: &mut Query<&mut EffectDuration, With<Effect>>,
    ) {
        match policy {
            EffectStackingPolicy::Add { count, max_stack } => {
                // Apply additive stacking, increasing stack count up to max
                if let Ok(mut stack_count) = stacks.get_mut(effect_entity) {
                    let mut base_stacks = stack_count.base_value();
                    base_stacks += count;
                    stack_count.set_base_value(base_stacks.clamp(1, max_stack.as_()));
                    stack_count.set_current_value(base_stacks.clamp(1.as_(), max_stack.as_()));
                } else {
                    error!(
                        "Failed to find component Stacks for entity: {:?}",
                        effect_entity
                    );
                }
            }
            EffectStackingPolicy::RefreshDuration => {
                // Reset duration for overridden effects
                if let Ok(mut duration) = durations.get_mut(effect_entity) {
                    duration.reset();
                } else {
                    error!(
                        "Failed to find component EffectApplication for entity: {:?}",
                        effect_entity
                    );
                }
            }
            EffectStackingPolicy::None => {
                error!(
                    "Effect stacking should not be triggered for effect entity {:?} with incompatible policy (None)",
                    effect_entity
                );
            }
        }
    }
}

#[derive(Message)]
pub struct NotifyAddStackEvent {
    pub effect_entity: Entity,
    pub handle: Handle<EffectDef>,
}

pub(crate) fn read_add_stack_event(
    mut event_reader: MessageReader<NotifyAddStackEvent>,
    mut stacks: Query<&mut Stacks, With<Effect>>,
    mut applications: Query<&mut EffectDuration, With<Effect>>,
    effect_assets: Res<Assets<EffectDef>>,
) {
    for ev in event_reader.read() {
        let effect_definition = match effect_assets.get(&ev.handle) {
            Some(effect) => effect,
            None => {
                panic!(
                    "Failed to find effect definition for handle: {:?}",
                    ev.handle
                );
            }
        };

        Stacks::apply_stacking_policy(
            &effect_definition.stacking_policy,
            ev.effect_entity,
            &mut stacks,
            &mut applications,
        );
    }
}
