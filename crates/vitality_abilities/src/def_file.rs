//! Def files: the declarative half of an [`AbilityDef`], written as data instead of Rust.
//!
//! Same `key = value` shape as [`vitality_effects::def_file`], the same [`DefNames`] registry, and
//! the same rule that a name the app never registered fails the file. Costs, task scenes and
//! observers stay Rust, so what an ability spends and what it runs cannot come out of a file.

use crate::builder::AbilityBuilder;
use crate::definition::AbilityDef;
use bevy::prelude::*;
use std::path::Path;
use vitality_core::mutator::EntityActions;
use vitality_effects::def_file::def_fields;

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
            "name" => name = Some(value),
            "cooldown" => cooldown = Some((line, value)),
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
    if let Some(name) = name {
        builder = builder.with_name(name);
    }
    if let Some((line, value)) = &cooldown {
        let seconds = value.parse::<f32>().map_err(|_| {
            DefFileError::line(*line, format!("`{value}` is no cooldown in seconds"))
        })?;
        builder = builder.with_cooldown(seconds);
    }
    let mut def = builder.build();

    for (line, tag) in &tags {
        let spec = names.tag_spec(*line, tag)?;
        def.mutators.push(EntityActions::new(move |commands| {
            commands.queue_silenced(move |mut entity: EntityWorldMut| spec.insert_on(&mut entity));
        }));
        def.tags.push(spec);
    }
    for (line, tag) in &owned {
        def.activation_owned_tags.push(names.tag_spec(*line, tag)?);
    }
    for (line, tag) in &requires {
        def.activation_requires.push(names.tag_spec(*line, tag)?);
    }
    for (line, tag) in &blocked_by {
        def.activation_blocked_by.push(names.tag_spec(*line, tag)?);
    }
    for (line, tag) in &blocks {
        def.blocks_abilities_with.push(names.tag_spec(*line, tag)?);
    }
    for (line, tag) in &cancels {
        def.cancels_abilities_with.push(names.tag_spec(*line, tag)?);
    }
    Ok(def)
}

/// Reads a def file off disk. A game that ships its defs inside a bundle parses the same text with
/// [`ability_def_from_str`] instead.
pub fn ability_def_from_file(
    path: impl AsRef<Path>,
    names: &DefNames,
) -> Result<AbilityDef, DefFileError> {
    let source = vitality_effects::def_file::def_file_source(path)?;
    ability_def_from_str(&source, names)
}
