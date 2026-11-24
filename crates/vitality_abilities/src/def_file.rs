//! Def files: the declarative half of an [`AbilityDef`], written as data instead of Rust.
//!
//! Same `key = value` shape and [`DefNames`] registry as [`vitality_effects::def_file`], where a
//! name the app never registered fails the file. A `.ron` ability file holds the same fields as an
//! `AbilityFile` struct. Costs, task scenes and observers stay Rust.

use crate::builder::AbilityBuilder;
use crate::definition::AbilityDef;
use bevy_ecs::prelude::*;
use serde::Deserialize;
use std::path::Path;
use vitality_core::mutator::EntityActions;
use vitality_effects::def_file::{check_seconds, def_field, def_fields, ron_options, scalar_field};

pub use vitality_effects::def_file::{DefFileError, DefNames};

/// Reads a def file into an [`AbilityDef`], resolving every name through `names`.
///
/// ```
/// # use vitality_abilities::def_file::ability_def_from_str;
/// # use vitality_effects::DefNames;
/// let def = ability_def_from_str("name = Fireball\ncooldown = 4.0", &DefNames::default()).unwrap();
/// assert_eq!(def.name, "Fireball");
/// ```
pub fn ability_def_from_str(source: &str, names: &DefNames) -> Result<AbilityDef, DefFileError> {
    let mut name = None;
    let mut cooldown = None;
    let mut tags = Vec::new();
    let mut owned = Vec::new();
    let mut requires = Vec::new();
    let mut blocked_by = Vec::new();
    let mut blocks = Vec::new();
    let mut cancels = Vec::new();

    for (line, key, value) in def_fields(source)? {
        match key.as_str() {
            "name" => scalar_field(&mut name, line, "name", value)?,
            "cooldown" => scalar_field(&mut cooldown, line, "cooldown", value)?,
            "tag" => tags.push((line, value)),
            "activation_owned" => owned.push((line, value)),
            "activation_requires" => requires.push((line, value)),
            "activation_blocked_by" => blocked_by.push((line, value)),
            "blocks" => blocks.push((line, value)),
            "cancels" => cancels.push((line, value)),
            key => {
                return Err(DefFileError::line(
                    line,
                    format!(
                        "`{key}` is not a key of an ability file, which holds name, cooldown, \
                         tag, activation_owned, activation_requires, activation_blocked_by, \
                         blocks and cancels"
                    ),
                ));
            }
        }
    }

    let mut builder = AbilityBuilder::new();
    if let Some((_, name)) = name {
        builder = builder.with_name(name);
    }
    if let Some((line, value)) = &cooldown {
        let seconds = value.parse::<f32>().map_err(|_| {
            DefFileError::line(*line, format!("`{value}` is not a cooldown in seconds"))
        })?;
        builder = builder.with_cooldown(check_seconds(
            &format!("line {line}"),
            value,
            seconds,
            "a cooldown",
        )?);
    }
    let mut def = builder.build();

    for (line, tag) in &tags {
        add_ability_tag(&mut def, &format!("line {line}"), tag, names)?;
    }
    for (line, tag) in &owned {
        def.activation_owned_tags
            .push(names.tag_at(&format!("line {line}"), tag)?);
    }
    for (line, tag) in &requires {
        def.activation_requires
            .push(names.tag_at(&format!("line {line}"), tag)?);
    }
    for (line, tag) in &blocked_by {
        def.activation_blocked_by
            .push(names.tag_at(&format!("line {line}"), tag)?);
    }
    for (line, tag) in &blocks {
        def.blocks_abilities_with
            .push(names.tag_at(&format!("line {line}"), tag)?);
    }
    for (line, tag) in &cancels {
        def.cancels_abilities_with
            .push(names.tag_at(&format!("line {line}"), tag)?);
    }
    Ok(def)
}

/// Reads a def file off disk; a bundle ships the same text through [`ability_def_from_str`].
pub fn ability_def_from_file(
    path: impl AsRef<Path>,
    names: &DefNames,
) -> Result<AbilityDef, DefFileError> {
    let source = vitality_effects::def_file::def_file_source(path)?;
    ability_def_from_str(&source, names)
}

/// An `EntityActions` that inserts the tag, which is what the ability's own tags do on grant.
fn add_ability_tag(
    def: &mut AbilityDef,
    at: &str,
    tag: &str,
    names: &DefNames,
) -> Result<(), DefFileError> {
    let spec = names.tag_at(at, tag)?;
    def.mutators.push(EntityActions::new(move |commands| {
        commands.queue_silenced(move |mut entity: EntityWorldMut| spec.insert_on(&mut entity));
    }));
    def.tags.push(spec);
    Ok(())
}

/// The RON mirror of an ability file's keys.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AbilityFile {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    cooldown: Option<f32>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    activation_owned: Vec<String>,
    #[serde(default)]
    activation_requires: Vec<String>,
    #[serde(default)]
    activation_blocked_by: Vec<String>,
    #[serde(default)]
    blocks: Vec<String>,
    #[serde(default)]
    cancels: Vec<String>,
}

/// Reads a RON def file into an [`AbilityDef`], with the same refusals as [`ability_def_from_str`].
///
/// ```
/// # use vitality_abilities::def_file::ability_def_from_ron;
/// # use vitality_effects::DefNames;
/// let def = ability_def_from_ron(
///     r#"AbilityFile(name: "Fireball", cooldown: 4.0)"#,
///     &DefNames::default(),
/// )
/// .unwrap();
/// assert_eq!(def.name, "Fireball");
/// ```
pub fn ability_def_from_ron(source: &str, names: &DefNames) -> Result<AbilityDef, DefFileError> {
    let file: AbilityFile = ron_options().from_str(source).map_err(DefFileError::new)?;

    let mut builder = AbilityBuilder::new();
    if let Some(name) = file.name {
        builder = builder.with_name(name);
    }
    if let Some(cooldown) = file.cooldown {
        builder = builder.with_cooldown(check_seconds(
            "cooldown",
            &cooldown.to_string(),
            cooldown,
            "a cooldown",
        )?);
    }
    let mut def = builder.build();

    for (index, tag) in file.tags.iter().enumerate() {
        add_ability_tag(&mut def, &def_field("tags", index), tag, names)?;
    }
    for (index, tag) in file.activation_owned.iter().enumerate() {
        def.activation_owned_tags
            .push(names.tag_at(&def_field("activation_owned", index), tag)?);
    }
    for (index, tag) in file.activation_requires.iter().enumerate() {
        def.activation_requires
            .push(names.tag_at(&def_field("activation_requires", index), tag)?);
    }
    for (index, tag) in file.activation_blocked_by.iter().enumerate() {
        def.activation_blocked_by
            .push(names.tag_at(&def_field("activation_blocked_by", index), tag)?);
    }
    for (index, tag) in file.blocks.iter().enumerate() {
        def.blocks_abilities_with
            .push(names.tag_at(&def_field("blocks", index), tag)?);
    }
    for (index, tag) in file.cancels.iter().enumerate() {
        def.cancels_abilities_with
            .push(names.tag_at(&def_field("cancels", index), tag)?);
    }
    Ok(def)
}

/// Reads a RON def file off disk; a bundle ships the same text through [`ability_def_from_ron`].
pub fn ability_def_from_ron_file(
    path: impl AsRef<Path>,
    names: &DefNames,
) -> Result<AbilityDef, DefFileError> {
    let source = vitality_effects::def_file::def_file_source(path)?;
    ability_def_from_ron(&source, names)
}
