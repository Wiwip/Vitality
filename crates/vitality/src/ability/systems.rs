use crate::AttributesMut;
use crate::ability::{Ability, AbilityRecovery, TargetData};
use crate::assets::AbilityDef;
use crate::context::{AbilityExprContext, AbilityExprContextMut, AbilityExprSchema};
use bevy::asset::Assets;
use bevy::ecs::resource::IsResource;
use bevy::prelude::*;
use express_it::expr::{BoolExpr, Expr};

pub fn tick_ability_cooldown(mut query: Query<&mut AbilityRecovery>, time: Res<Time>) {
    query.par_iter_mut().for_each(|mut recovery| {
        recovery.duration += time.delta();
    });
}

pub fn can_activate_ability(
    context: &AbilityExprContext,
    ability_def: &AbilityDef,
    conditions: &BoolExpr<AbilityExprSchema>,
) -> Result<bool, BevyError> {
    let meet_conditions = conditions.eval(context);

    if !meet_conditions {
        return Ok(false);
    }

    let can_activate = ability_def
        .cost_condition
        .iter()
        .all(|condition| condition.eval(context));

    if !can_activate {
        debug!("Insufficient resources to activate ability!");
        return Ok(false);
    }
    Ok(true)
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
    mut actors: Query<AttributesMut, Without<IsResource>>,
    ability_assets: Res<Assets<AbilityDef>>,
) -> Result<(), BevyError> {
    debug!("{}: Commit ability cost.", trigger.ability);
    let actor = actors.get(trigger.ability)?;
    let ability = actor.get::<Ability>().unwrap();
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
