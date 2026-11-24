use crate::AbilityMut;
use crate::context::AbilityExprContextMut;
use crate::definition::AbilityDef;
use crate::{Ability, TargetData};
use bevy_asset::Assets;
use bevy_ecs::prelude::*;
use bevy_ecs::resource::IsResource;
use bevy_log::{debug, error};

#[derive(EntityEvent)]
pub struct ActivateAbility {
    #[event_target]
    pub ability: Entity,
    pub target: TargetData,
    pub source: Entity,
}

/// commits what [`ActivateAbility`] carries, bypassing the checks the ready state would run first
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

    let location = match trigger.target {
        TargetData::Location(point) => Some(point),
        _ => None,
    };

    let mut context = match trigger.target {
        // a caster aimed at itself is a self-cast: get_many_mut refuses a repeated entity
        TargetData::Target(target) if target == trigger.source => {
            let [caster_mut, ability_mut] =
                actors.get_many_mut([trigger.source, trigger.ability])?;
            AbilityExprContextMut {
                caster_mut,
                ability_mut,
                target_mut: None,
                location,
            }
        }
        TargetData::Target(target) => {
            let [caster_mut, target_mut, ability_mut] =
                actors.get_many_mut([trigger.source, target, trigger.ability])?;
            AbilityExprContextMut {
                caster_mut,
                ability_mut,
                target_mut: Some(target_mut),
                location: None,
            }
        }
        TargetData::Location(_) | TargetData::SelfCast => {
            let [caster_mut, ability_mut] =
                actors.get_many_mut([trigger.source, trigger.ability])?;
            AbilityExprContextMut {
                caster_mut,
                ability_mut,
                target_mut: None,
                location,
            }
        }
    };

    // a failed step keeps what the steps before it wrote and the activation still commits
    for plan in &ability_spec.on_execute {
        if let Err(err) = plan.run(&mut context) {
            error!("[{}] Ability execute plan failed: {err}", trigger.ability);
        }
    }

    if let Err(err) = ability_spec.cost_modifiers.run(&mut context) {
        error!("[{}] Ability cost plan failed: {err}", trigger.ability);
    }
    Ok(())
}
