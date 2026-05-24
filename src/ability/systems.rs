use crate::ability::{
    Ability, AbilityCooldown, AbilityOf, BeginAbility, EndAbility, ExecuteAbility,
    GrantedAbilities, TryActivateAbility,
};
use crate::assets::AbilityDef;
use crate::context::{
    AbilityExprContext, AbilityExprSchema, EffectExprContext, EffectExprContextMut,
};
use crate::{AppAttributeBindings, AttributesMut, AttributesRef};
use bevy::asset::Assets;
use bevy::ecs::resource::IsResource;
use bevy::prelude::*;
use bevy::reflect::TypeRegistryArc;
use express_it::logic::BoolExpr;
use std::time::Duration;

pub fn tick_ability_cooldown(mut query: Query<&mut AbilityCooldown>, time: Res<Time>) {
    query.par_iter_mut().for_each(|mut cooldown| {
        cooldown.timer.tick(time.delta());
    });
}

/// Tries to activate an ability.
///
/// Base conditions are:
/// - Cooldown
/// - Conditions
/// - Cost
pub fn try_activate_ability_observer(
    trigger: On<TryActivateAbility>,
    actors: Query<
        (AttributesRef, &GrantedAbilities),
        (Without<AbilityCooldown>, Without<IsResource>),
    >,
    abilities: Query<(AttributesRef, &Ability, Option<&AbilityCooldown>)>,
    ability_assets: Res<Assets<AbilityDef>>,
    mut commands: Commands,
    type_registry: Res<AppTypeRegistry>,
) -> Result<(), BevyError> {
    let Ok((source_entity_ref, actor_abilities)) = actors.get(trigger.ability) else {
        warn!("The Actor({}) has no GrantedAbilities", trigger.ability);
        return Ok(());
    };

    let target_entity_ref = match trigger.target_data {
        crate::ability::TargetData::SelfCast => source_entity_ref,
        crate::ability::TargetData::Target(target) => {
            let Ok((entity, _)) = actors.get(target) else {
                return Ok(());
            };
            entity
        }
    };

    for &ability_entity in actor_abilities.0.iter() {
        let (ability_ref, ability, opt_cooldown) = abilities
            .get(ability_entity)
            .expect("Ability not found in: try_activate_ability_observer.");

        // Handle cooldowns
        let is_finished = match opt_cooldown {
            None => true,
            Some(cd) => cd.timer.is_finished(),
        };
        if !is_finished {
            continue;
        }

        let ability_spec = ability_assets
            .get(&ability.0.clone())
            .ok_or("No ability asset.")?;

        let can_activate = can_activate_ability(
            &ability_ref,
            &source_entity_ref,
            &target_entity_ref,
            &ability_spec,
            &trigger.condition,
            &type_registry.0.clone(),
        )
        .ok()
        .unwrap_or(false);

        if can_activate {
            commands.trigger(AbilityCooldownReset {
                target: target_entity_ref.id(),
                source: source_entity_ref.id(),
                ability: ability_entity,
            });
            commands.trigger(ActivateAbility {
                target: target_entity_ref.id(),
                source: source_entity_ref.id(),
                ability: ability_entity,
            });
        }
    }
    Ok(())
}

fn can_activate_ability(
    ability_ref: &AttributesRef,
    caster_ref: &AttributesRef,
    target_ref: &AttributesRef,
    ability_def: &AbilityDef,
    conditions: &BoolExpr<AbilityExprSchema>,
    type_registry: &TypeRegistryArc,
) -> Result<bool, BevyError> {
    let context = AbilityExprContext {
        target_ref,
        caster_ref,
        ability_ref,
        type_registry: type_registry.clone(),
    };

    let meet_conditions = conditions.eval(&context).unwrap_or_else(|err| {
        error_once!("Condition error: {:?}", err);
        false
    });

    if !meet_conditions {
        debug!(
            "Ability({}) conditions not met for: {}.",
            ability_ref.id(),
            ability_def.name
        );
        return Ok(false);
    }

    let can_activate = ability_def
        .cost_condition
        .iter()
        .all(|condition| condition.eval(&context).unwrap_or(false));

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
    mut cooldowns: Query<(&AbilityOf, &mut AbilityCooldown)>,
    query: Query<AttributesRef>,
    type_registry: Res<AppTypeRegistry>,
) -> Result<(), BevyError> {
    let Ok((_parent, mut cooldown)) = cooldowns.get_mut(trigger.ability) else {
        // This event does not affect an ability without a cooldown.
        return Ok(());
    };

    let [source, target, owner] =
        query.get_many([trigger.source, trigger.target, trigger.ability])?;
    let context = EffectExprContext {
        target_actor: &source,
        source_actor: &target,
        effect_holder: &owner,
        type_registry: type_registry.0.clone(),
    };

    let cd_value = cooldown.value.eval(&context)?;

    cooldown
        .timer
        .set_duration(Duration::from_secs_f64(cd_value));
    cooldown.timer.reset();
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
    mut actors: Query<AttributesMut<'static, 'static>, Without<IsResource>>,
    ability_assets: Res<Assets<AbilityDef>>,
    mut commands: Commands,
    type_registry: Res<AppTypeRegistry>,
    type_bindings: Res<AppAttributeBindings>,
) -> Result<(), BevyError> {
    debug!("{}: Commit ability cost.", trigger.ability);
    let actor = actors.get(trigger.ability)?;
    let ability = actor.get::<Ability>().unwrap();
    let ability_spec = ability_assets
        .get(&ability.0.clone())
        .ok_or("No ability asset")?;

    if trigger.source == trigger.target {
        for plan in &ability_spec.on_execute {
            let [source, ability] = actors.get_many([trigger.source, trigger.ability])?;
            let immutable_context = AbilityExprContext {
                caster_ref: &source,
                target_ref: &source,
                ability_ref: &ability,
                type_registry: type_registry.0.clone(),
            };
            let output = plan.eval(&immutable_context)?;

            let [mut source, mut owner] = actors.get_many_mut([trigger.source, trigger.ability])?;
            let mut context = EffectExprContextMut {
                source_actor: &mut source,
                target_actor: None,
                owner: &mut owner,
                type_registry: type_registry.0.clone(),
                type_bindings: type_bindings.clone(),
            };

            output.flush_into(&mut context);
        }

        // Calculates the costs of the ability and applies them
        let [source, ability] = actors.get_many([trigger.source, trigger.ability])?;
        let immutable_context = AbilityExprContext {
            caster_ref: &source,
            target_ref: &source,
            ability_ref: &ability,
            type_registry: type_registry.0.clone(),
        };

        let plan_results = ability_spec.cost_modifiers.eval(&immutable_context)?;

        let [mut source, mut owner] = actors.get_many_mut([trigger.source, trigger.ability])?;
        let mut context = EffectExprContextMut {
            source_actor: &mut source,
            target_actor: None,
            owner: &mut owner,
            type_registry: type_registry.0.clone(),
            type_bindings: type_bindings.clone(),
        };

        plan_results.flush_into(&mut context);
    } else {
        for plan in &ability_spec.on_execute {
            let [source, target, ability] =
                actors.get_many([trigger.source, trigger.target, trigger.ability])?;
            let immutable_context = AbilityExprContext {
                caster_ref: &source,
                target_ref: &target,
                ability_ref: &ability,
                type_registry: type_registry.0.clone(),
            };
            let output = plan.eval(&immutable_context)?;

            let [mut source, mut target, mut owner] =
                actors.get_many_mut([trigger.source, trigger.target, trigger.ability])?;
            let mut context = EffectExprContextMut {
                source_actor: &mut source,
                target_actor: Some(&mut target),
                owner: &mut owner,
                type_registry: type_registry.0.clone(),
                type_bindings: type_bindings.clone(),
            };

            output.flush_into(&mut context);
        }

        // Calculates the costs of the ability and applies them
        let [source, ability] = actors.get_many([trigger.source, trigger.ability])?;
        let immutable_context = AbilityExprContext {
            caster_ref: &source,
            target_ref: &source,
            ability_ref: &ability,
            type_registry: type_registry.0.clone(),
        };
        let plan_results = ability_spec.cost_modifiers.eval(&immutable_context)?;

        let [mut source, mut owner] = actors.get_many_mut([trigger.source, trigger.ability])?;
        let mut context = EffectExprContextMut {
            source_actor: &mut source,
            target_actor: None,
            owner: &mut owner,
            type_registry: type_registry.0.clone(),
            type_bindings: type_bindings.clone(),
        };

        plan_results.flush_into(&mut context);
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
