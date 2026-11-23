//! the def-file loaders that read off disk, which no `assets/` file backs any more

mod gameplay;

use bevy::prelude::*;
use gameplay::*;
use hfsm_bevy::StateTimer;
use std::path::PathBuf;
use std::time::Duration;
use vitality::ability::ability_state::{AbilityMachine, AbilityState};
use vitality::ability::{AbilityDef, TargetData, ability_def_from_file};
use vitality::attributes::ReflectAccessAttribute;
use vitality::effect::modifier::{AttributeModifier, EffectSubject, ModOp, OwnedModifiers};
use vitality::effect::{
    DefNames, EffectApplicationPolicy, effect_def_from_file, effect_def_from_str,
};
use vitality::{attribute, init_attribute};

/// one scratch dir per test, so a parallel run cannot read another test's file
fn scratch(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("vitality-def-file-{test}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch dir under the temp dir");
    dir
}

fn def_file(test: &str, file: &str, body: &str) -> PathBuf {
    let path = scratch(test).join(file);
    std::fs::write(&path, body).expect("the def file is written");
    path
}

fn names() -> DefNames {
    let mut names = DefNames::default();
    names.attribute::<Health>();
    names.tag::<Burning>();
    names.tag::<Casting>();
    names
}

fn ability(app: &mut App, def: AbilityDef) -> Handle<AbilityDef> {
    app.world_mut()
        .resource_mut::<Assets<AbilityDef>>()
        .add(def)
}

#[test]
fn an_effect_file_off_disk_loads_the_def_it_describes() {
    let path = def_file(
        "effect",
        "regeneration.effect",
        "# a def the app never built in Rust\nname = Regeneration\npolicy = every 1.0 for 12.0\ntag = Burning\nmodifier = Health add 2 target\n",
    );
    let def = effect_def_from_file(&path, &names()).expect("the file on disk parses");

    assert_eq!(
        def.application_policy,
        EffectApplicationPolicy::every_seconds_for_duration(1.0, 12.0),
        "the policy line carries both the interval and the duration"
    );
    assert_eq!(def.modifiers.len(), 1);
    assert_eq!(def.granted_tags.len(), 1);

    // the name, the op, the value and the subject only surface once the def is applied
    let mut app = app();
    let handle = effect(&mut app, def);
    let hero = spawn(&mut app, hero_def());
    apply(&mut app, hero, hero, &handle);
    ticks(&mut app, 1);

    let effect_entity = with_effects(&mut app, move |effects| {
        effects
            .find(hero, &handle)
            .expect("the hero carries the effect the file describes")
    });
    assert_eq!(
        app.world()
            .get::<Name>(effect_entity)
            .map(|name| name.as_str()),
        Some("Regeneration")
    );
    let modifier = app
        .world()
        .get::<OwnedModifiers>(effect_entity)
        .expect("the effect owns its modifier")
        .iter()
        .next()
        .expect("the file wrote one modifier");
    let modifier = app
        .world()
        .get::<AttributeModifier<Health>>(modifier)
        .expect("the modifier is a Health write");
    assert!(
        matches!(modifier.operation, ModOp::Add),
        "the file wrote {:?}, not an add",
        modifier.operation
    );
    assert!(
        matches!(modifier.who, EffectSubject::Target),
        "the modifier writes the target, not {:?}",
        modifier.who
    );
    assert_eq!(modifier.value, 2.0);

    ticks(&mut app, ticks_for(1.0));
    assert_eq!(
        current::<Health>(&app, hero),
        102.0,
        "one tick of the file's own number"
    );
}

attribute!(Hp, u32);

#[test]
fn a_file_cannot_write_a_number_the_integer_attribute_it_names_does_not_hold() {
    let mut names = names();
    names.attribute::<Hp>();

    let Err(error) = effect_def_from_str("modifier = Hp add 0.5 target", &names) else {
        panic!("a half narrowed to 0u32 is not the number that line wrote");
    };
    let error = error.to_string();
    assert!(
        error.contains("line 1") && error.contains("Hp"),
        "the refusal has to name the line and the attribute it narrowed on, got: {error}"
    );

    let Err(error) = effect_def_from_str("modifier = Hp add 4294967296 target", &names) else {
        panic!("one past u32::MAX is refused the way it always was");
    };
    let error = error.to_string();
    assert!(error.contains("line 1") && error.contains("Hp"), "{error}");

    // the whole number on the u32 and the half on the f32 still land where they did
    let mut app = app();
    app.add_plugins(init_attribute::<Hp>);
    let hero = spawn(&mut app, hero_def().with::<Hp>(10u32));
    let def = effect_def_from_str(
        "name = Drill\nmodifier = Hp add 2 target\nmodifier = Health add 0.5 target",
        &names,
    )
    .expect("2 is a u32 and 0.5 is an f32");
    assert_eq!(def.modifiers.len(), 2);
    let handle = effect(&mut app, def);
    apply(&mut app, hero, hero, &handle);
    ticks(&mut app, 1);
    assert_eq!(current::<Hp>(&app, hero), 12u32);
    assert_eq!(current::<Health>(&app, hero), 100.5);
}

#[test]
fn an_ability_file_off_disk_loads_the_def_it_describes() {
    let path = def_file(
        "ability",
        "fireball.ability",
        "name = Fireball\ncooldown = 4.0\ntag = Casting\n",
    );
    let def = ability_def_from_file(&path, &names()).expect("the file on disk parses");

    assert_eq!(def.name, "Fireball");
    assert_eq!(def.tags.len(), 1);

    let mut app = app();
    let handle = ability(&mut app, def);
    let caster = spawn(&mut app, hero_def().grant_ability(&handle));
    let granted = {
        let handle = handle.clone();
        with_abilities(&mut app, move |abilities| {
            abilities.try_activate(caster, &handle, TargetData::SelfCast)
        })
    };
    assert!(granted, "the ability the file describes is activatable");
    ticks(&mut app, 2);

    let ability_entity = with_abilities(&mut app, move |abilities| {
        abilities
            .get_ability_entity_by_handle(caster, &handle)
            .expect("the caster holds the ability")
    });
    let (cooldown, _) = app
        .world()
        .get::<StateTimer<AbilityMachine>>(ability_entity)
        .expect("a granted ability carries its machine timer")
        .get(AbilityState::Recovery)
        .expect("the cooldown the file names arms Recovery");
    assert_eq!(cooldown.duration(), Duration::from_secs_f32(4.0));
}

#[test]
fn a_def_file_line_the_loader_cannot_use_fails_naming_its_line() {
    let names = names();
    let cases = [
        (
            "unknown-key.effect",
            "name = Curio\nduration = 3.0\n",
            2,
            "duration",
        ),
        (
            "unknown-tag.effect",
            "name = Deep Freeze\ntag = Frost\n",
            2,
            "Frost",
        ),
        (
            "unknown-attribute.effect",
            "name = Spark\nmodifier = Mana add 1 target\n",
            2,
            "Mana",
        ),
        (
            "bad-policy.effect",
            "name = Stagger\npolicy = every 1.0 soon\n",
            2,
            "every 1.0 soon",
        ),
        (
            "bad-modifier.effect",
            "name = Mend\nmodifier = Health add target\n",
            2,
            "Health add target",
        ),
        (
            "negative-duration.effect",
            "name = Doom\npolicy = for -1\n",
            2,
            "-1",
        ),
        (
            "nan-interval.effect",
            "name = Chaos\npolicy = every nan\n",
            2,
            "nan",
        ),
        (
            "huge-duration.effect",
            "name = Aeon\npolicy = for 1e30\n",
            2,
            "1e30",
        ),
        (
            "nan-modifier.effect",
            "name = Chaos\nmodifier = Health add nan target\n",
            2,
            "nan",
        ),
        (
            "inf-modifier.effect",
            "name = Breaker\nmodifier = Health add inf target\n",
            2,
            "inf",
        ),
        (
            "overflow-modifier.effect",
            "name = Breaker\nmodifier = Health add 1e300 target\n",
            2,
            "is not a `Health` value",
        ),
    ];
    for (file, body, line, bad) in cases {
        let path = def_file("line-error", file, body);
        let Err(error) = effect_def_from_file(&path, &names) else {
            panic!("{file} loaded even though line {line} spells `{bad}` wrong");
        };
        let error = error.to_string();
        assert!(
            error.starts_with(&format!("line {line}:")) && error.contains(bad),
            "{file} has to name line {line} and the `{bad}` it refused, got: {error}"
        );
    }

    let ability_cases = [
        (
            "huge-cooldown.ability",
            "name = Meteor\ncooldown = 1e30\n",
            2,
            "1e30",
        ),
        (
            "nan-cooldown.ability",
            "name = Hex\ncooldown = nan\n",
            2,
            "nan",
        ),
    ];
    for (file, body, line, bad) in ability_cases {
        let path = def_file("line-error", file, body);
        let Err(error) = ability_def_from_file(&path, &names) else {
            panic!("{file} loaded even though line {line} spells `{bad}` wrong");
        };
        let error = error.to_string();
        assert!(
            error.starts_with(&format!("line {line}:")) && error.contains(bad),
            "{file} has to name line {line} and the `{bad}` it refused, got: {error}"
        );
    }
}

#[test]
fn a_def_file_that_is_not_there_fails_naming_its_path() {
    let missing = scratch("missing").join("never-written.effect");
    let names = names();
    let Err(error) = effect_def_from_file(&missing, &names) else {
        panic!("a path nothing wrote to cannot load an effect def");
    };
    // def_file_source reports `{path}: {err}`, so the designer sees which file it lost
    assert!(
        error
            .to_string()
            .starts_with(&missing.display().to_string()),
        "{error}"
    );

    let Err(error) = ability_def_from_file(&missing, &names) else {
        panic!("a path nothing wrote to cannot load an ability def");
    };
    assert!(
        error
            .to_string()
            .starts_with(&missing.display().to_string()),
        "{error}"
    );
}
