use crate::AbilityMut;
use crate::context::AbilityExprContextMut;
use crate::definition::AbilityDef;
use crate::{Ability, AbilityRecovery, TargetData};
use bevy::asset::Assets;
use bevy::ecs::resource::IsResource;
use bevy::prelude::*;

pub fn tick_ability_cooldown(mut query: Query<&mut AbilityRecovery>, time: Res<Time>) {
    query.par_iter_mut().for_each(|mut recovery| {
        recovery.tick(time.delta());
    });
}

#[derive(EntityEvent)]
pub struct ActivateAbility {
    #[event_target]
    pub ability: Entity,
    pub target: TargetData,
    pub source: Entity,
}

/// Bypass [TryActivateAbility]'s checks. Usually triggered after a successful [TryActivateAbility].
pub(crate) fn activate_ability(
    trigger: On<ActivateAbility>,
    mut actors: Query<AbilityMut, Without<IsResource>>,
    ability_assets: Res<Assets<AbilityDef>>,
) -> Result<(), BevyError> {
    debug!("{}: Commit ability cost.", trigger.ability);
    let actor = actors.get(trigger.ability)?;
    let ability = actor
        .get::<Ability>()
        .ok_or("Activated entity is not an ability")?;
    let ability_spec = ability_assets
        .get(&ability.handle.clone())
        .ok_or("No ability asset")?;

    let mut context = match trigger.target {
        TargetData::Target(target) => {
            let [caster_mut, target_mut, ability_mut] =
                actors.get_many_mut([trigger.source, target, trigger.ability])?;
            AbilityExprContextMut {
                caster_mut,
                ability_mut,
                target_mut: Some(target_mut),
            }
        }
        TargetData::Location(_) | TargetData::SelfCast => {
            let [caster_mut, ability_mut] =
                actors.get_many_mut([trigger.source, trigger.ability])?;
            AbilityExprContextMut {
                caster_mut,
                ability_mut,
                target_mut: None,
            }
        }
    };

    for plan in &ability_spec.on_execute {
        plan.run(&mut context);
    }

    // Calculates the costs of the ability and applies them
    ability_spec.cost_modifiers.run(&mut context);
    Ok(())
}
