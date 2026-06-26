use crate::ability::{
    Ability, AbilityOf, AbilityRecovery, BeginAbility, EndAbility, ExecuteAbility,
};
use crate::assets::AbilityDef;
use crate::context::{
    AbilityExprContext, AbilityExprContextMut, AbilityExprSchema, EffectExprContext,
    EffectExprContextMut,
};
use crate::{AppAttributeBindings, AttributesMut, AttributesRef};
use bevy::asset::Assets;
use bevy::ecs::resource::IsResource;
use bevy::prelude::*;
use bevy::reflect::TypeRegistryArc;
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
        /*debug!(
            "Ability({}) conditions not met for: {}.",
            //ability_ref.id(),
            ability_def.name
        );*/
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
pub(crate) struct AbilityCooldownReset {
    pub source: Entity,
    pub target: Entity,
    #[event_target]
    pub ability: Entity,
}

pub(crate) fn reset_ability_cooldown(
    trigger: On<AbilityCooldownReset>,
    mut cooldowns: Query<(&AbilityOf, &mut AbilityRecovery)>,
    query: Query<AttributesRef>,
) -> Result<(), BevyError> {
    let Ok((_parent, mut cooldown)) = cooldowns.get_mut(trigger.ability) else {
        // This event does not affect an ability without a cooldown.
        return Ok(());
    };

    let [source, target, owner] =
        query.get_many([trigger.source, trigger.target, trigger.ability])?;
    let context = EffectExprContext {
        target_actor: Some(target),
        source_actor: source,
        effect_holder: owner,
    };

    /*let cd_value = cooldown.value.eval(&context)?;

    cooldown
        .timer
        .set_duration(Duration::from_secs_f64(cd_value));
    cooldown.timer.reset();*/
    Ok(())
}

#[derive(EntityEvent)]
pub struct ActivateAbility {
    #[event_target]
    pub target: Entity,
    pub source: Entity,
    pub ability: Entity,
}

/// Bypass [TryActivateAbility]'s checks. Usually triggered after a successful [TryActivateAbility].
pub(crate) fn activate_ability(
    trigger: On<ActivateAbility>,
    mut actors: Query<AttributesMut, Without<IsResource>>,
    ability_assets: Res<Assets<AbilityDef>>,
    mut commands: Commands,
) -> Result<(), BevyError> {
    debug!("{}: Commit ability cost.", trigger.ability);
    let actor = actors.get(trigger.ability)?;
    let ability = actor.get::<Ability>().unwrap();
    let ability_spec = ability_assets
        .get(&ability.0.clone())
        .ok_or("No ability asset")?;

    if trigger.source == trigger.target {
        for plan in &ability_spec.on_execute {
            let [caster_mut, ability_mut] =
                actors.get_many_mut([trigger.source, trigger.ability])?;
            let mut context = AbilityExprContextMut {
                caster_mut,
                ability_mut,
                target_mut: None,
            };

            plan.run(&mut context);
        }

        // Calculates the costs of the ability and applies them
        let [caster_mut, ability_mut] =
            actors.get_many_mut([trigger.source, trigger.ability])?;
        let mut context = AbilityExprContextMut {
            caster_mut,
            ability_mut,
            target_mut: None,
        };
        ability_spec.cost_modifiers.run(&mut context);
    } else {
        for plan in &ability_spec.on_execute {
            let [caster_mut, target_mut, ability_mut] =
                actors.get_many_mut([trigger.source, trigger.target, trigger.ability])?;
            let mut context = AbilityExprContextMut {
                caster_mut,

                ability_mut,
                target_mut: Some(target_mut),
            };
            plan.run(&mut context);
        }

        // Calculates the costs of the ability and applies them
        let [caster_mut, target_mut, ability_mut] =
            actors.get_many_mut([trigger.source, trigger.target, trigger.ability])?;
        let mut context = AbilityExprContextMut {
            caster_mut,
            ability_mut,
            target_mut: Some(target_mut),
        };
        ability_spec.cost_modifiers.run(&mut context);
    };

    // Activate the ability
    debug!("{}: Execute ability", trigger.ability);
    commands.trigger(BeginAbility {
        source: trigger.source,
        ability: trigger.ability,
    });
    commands.trigger(ExecuteAbility {
        source: trigger.source,
        target: trigger.target,
        ability: trigger.ability,
    });
    commands.trigger(EndAbility {
        source: trigger.source,
        ability: trigger.ability,
    });
    Ok(())
}
