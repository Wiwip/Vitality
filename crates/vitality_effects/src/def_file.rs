//! Def files: the declarative half of an [`EffectDef`], written as data instead of Rust.
//!
//! Every line is `key = value`, `#` starts a comment and runs to the end of its line, and a key may
//! repeat where the def holds a list. Names are the one thing a file cannot invent: [`DefNames`]
//! holds the attributes and tags the app registered, and a file spells them exactly as Rust does,
//! so `Health` is the attribute [`crate::init_effect_attribute`] registered and anything else fails
//! naming its line.
//!
//! An effect file holds `name`, `policy`, `tag`, `requires`, `blocked_by`, `cleanses` and
//! `modifier`, and a file with no `policy` is a permanent effect.
//!
//! Expressions, scenes and observers stay Rust, so an ability cost, a clamp or a `Damage::src()`
//! read cannot come out of a file.

use crate::builder::EffectBuilder;
use crate::definition::EffectDef;
use crate::modifier::{AttributeModifier, EffectSubject, ModOp, Modifier};
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use express_it::nodes::LiteralNode;
use smol_str::SmolStr;
use std::sync::Arc;
use vitality_core::attributes::Attribute;
use vitality_core::num_traits::FromPrimitive;
use vitality_core::tags::TagSpec;
use vitality_core::util::pretty_type_name;

/// A file names an attribute and a literal, so one fn pointer builds that attribute's modifiers
/// without the file ever knowing the type.
type NewModifier = fn(value: f64, op: ModOp, who: EffectSubject) -> Option<Box<dyn Modifier>>;

/// What a def file is allowed to name. A name missing here fails the file that used it.
#[derive(Resource, Default)]
pub struct DefNames {
    attributes: HashMap<SmolStr, NewModifier>,
    tags: HashMap<SmolStr, TagSpec>,
}

impl DefNames {
    /// [`crate::init_effect_attribute`] calls this for every registered attribute, so a file spells
    /// an attribute the way Rust does: the short name the registry records.
    pub fn attribute<T: Attribute>(&mut self) {
        self.attributes
            .insert(pretty_type_name::<T>().into(), new_modifier::<T>);
    }

    pub fn tag<T: Component + Default>(&mut self) {
        self.tags
            .insert(pretty_type_name::<T>().into(), TagSpec::of::<T>());
    }

    /// the tag a def-file line names, for the abilities' def files as much as the effects'
    pub fn tag_spec(&self, line: usize, name: &str) -> Result<TagSpec, DefFileError> {
        self.tags.get(name).copied().ok_or_else(|| {
            DefFileError(format!(
                "line {line}: no tag `{name}`: a def file only names tags the app put in DefNames \
                 with tag::<{name}>()"
            ))
        })
    }

    fn modifier(
        &self,
        line: usize,
        name: &str,
        value: f64,
        op: ModOp,
        who: EffectSubject,
    ) -> Result<Box<dyn Modifier>, DefFileError> {
        let build = self.attributes.get(name).ok_or_else(|| {
            DefFileError(format!(
                "line {line}: no attribute `{name}`: a def file only names attributes the app \
                 registered with init_attribute::<{name}>()"
            ))
        })?;
        build(value, op, who).ok_or_else(|| {
            DefFileError(format!(
                "line {line}: {value} is not a `{name}` value: a modifier on {name} has to be a \
                 value {name} itself holds"
            ))
        })
    }
}

/// A def file that does not describe a def, in the words the designer of that file needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefFileError(String);

impl core::fmt::Display for DefFileError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for DefFileError {}

impl DefFileError {
    pub fn new(message: impl core::fmt::Display) -> Self {
        Self(message.to_string())
    }

    pub fn line(line: usize, message: impl core::fmt::Display) -> Self {
        Self(format!("line {line}: {message}"))
    }
}

/// The `key = value` lines of a def file, numbered, comments and blanks gone. Shared with the
/// ability files, which read the same shape and a different set of keys.
pub fn def_fields(source: &str) -> Result<Vec<(usize, SmolStr, String)>, DefFileError> {
    let mut fields = Vec::new();
    for (index, line) in source.lines().enumerate() {
        let line = line.split('#').next().unwrap_or_default().trim();
        if line.is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(DefFileError::line(
                index + 1,
                format!("`{line}` is not `key = value`"),
            ));
        };
        let (key, value) = (key.trim(), value.trim());
        if key.is_empty() || value.is_empty() {
            return Err(DefFileError::line(
                index + 1,
                format!("`{line}` has an empty key or value"),
            ));
        }
        fields.push((index + 1, key.into(), value.into()));
    }
    Ok(fields)
}

/// Reads a def file into an [`EffectDef`], resolving every name through `names`.
///
/// ```
/// # use vitality_effects::prelude::*;
/// # use vitality_effects::def_file::{DefNames, effect_def_from_str};
/// attribute!(Health, f32);
///
/// let mut names = DefNames::default();
/// names.attribute::<Health>();
/// let def = effect_def_from_str(
///     "name = Regeneration\npolicy = every 1.0 for 12.0\nmodifier = Health add 2 target",
///     &names,
/// )
/// .unwrap();
/// assert_eq!(def.modifiers.len(), 1);
/// assert!(def.application_policy.is_periodic());
/// ```
pub fn effect_def_from_str(source: &str, names: &DefNames) -> Result<EffectDef, DefFileError> {
    let mut name = None;
    let mut policy = None;
    let mut tags = Vec::new();
    let mut requires = Vec::new();
    let mut blocked_by = Vec::new();
    let mut cleanses = Vec::new();
    let mut modifiers = Vec::new();

    for (line, key, value) in def_fields(source)? {
        match key.as_str() {
            "name" => name = Some(value),
            "policy" => policy = Some((line, value)),
            "tag" => tags.push((line, value)),
            "requires" => requires.push((line, value)),
            "blocked_by" => blocked_by.push((line, value)),
            "cleanses" => cleanses.push((line, value)),
            "modifier" => modifiers.push((line, value)),
            key => {
                return Err(DefFileError::line(
                    line,
                    format!(
                        "`{key}` is not a key of an effect file, which holds name, policy, tag, \
                         requires, blocked_by, cleanses and modifier"
                    ),
                ));
            }
        }
    }

    let mut builder = match &policy {
        None => EffectBuilder::permanent(),
        Some((line, spec)) => policy_builder(*line, spec)?,
    };
    if let Some(name) = name {
        builder = builder.name(name);
    }
    let mut def = builder.build();
    for (line, tag) in &tags {
        def.granted_tags.push(names.tag_spec(*line, tag)?);
    }
    for (line, tag) in &requires {
        def.required_tags.push(names.tag_spec(*line, tag)?);
    }
    for (line, tag) in &blocked_by {
        def.blocked_by_tags.push(names.tag_spec(*line, tag)?);
    }
    for (line, tag) in &cleanses {
        def.cleanses.push(names.tag_spec(*line, tag)?);
    }
    for (line, spec) in &modifiers {
        def.modifiers.push(modifier(names, *line, spec)?);
    }
    Ok(def)
}

/// Reads a def file off disk. A game that ships its defs inside a bundle parses the same text with
/// [`effect_def_from_str`] instead.
pub fn effect_def_from_file(
    path: impl AsRef<std::path::Path>,
    names: &DefNames,
) -> Result<EffectDef, DefFileError> {
    let source = def_file_source(path)?;
    effect_def_from_str(&source, names)
}

/// The text of a def file, named by path when it cannot be read.
pub fn def_file_source(path: impl AsRef<std::path::Path>) -> Result<String, DefFileError> {
    let path = path.as_ref();
    std::fs::read_to_string(path)
        .map_err(|err| DefFileError::new(format!("{}: {err}", path.display())))
}

/// `instant`, `permanent`, `for <seconds>`, `every <seconds>` or `every <seconds> for <seconds>`.
fn policy_builder(line: usize, spec: &str) -> Result<EffectBuilder, DefFileError> {
    let wrong = || {
        DefFileError::line(
            line,
            format!(
                "`policy = {spec}` is not `instant`, `permanent`, `for <seconds>`, \
                 `every <seconds>` or `every <seconds> for <seconds>`"
            ),
        )
    };
    let mut words = spec.split_whitespace();
    let seconds = |word: &str, what: &str| {
        word.parse::<f32>()
            .map_err(|_| DefFileError::line(line, format!("`{word}` is not {what} in seconds")))
    };
    let builder = match (words.next(), words.next(), words.next(), words.next()) {
        (Some("instant"), None, None, None) => EffectBuilder::instant(),
        (Some("permanent"), None, None, None) => EffectBuilder::permanent(),
        (Some("for"), Some(value), None, None) => {
            EffectBuilder::for_seconds(seconds(value, "a duration")?)
        }
        (Some("every"), Some(value), None, None) => {
            EffectBuilder::every_second_permanently(seconds(value, "an interval")?)
        }
        (Some("every"), Some(value), Some("for"), Some(duration)) => {
            let interval = seconds(value, "an interval")?;
            EffectBuilder::every_second_for_duration(interval, seconds(duration, "a duration")?)
        }
        _ => return Err(wrong()),
    };
    Ok(builder)
}

/// `<attribute> <op> <value> <who>`, spelled like [`ModOp`] and [`EffectSubject`] name themselves.
fn modifier(names: &DefNames, line: usize, spec: &str) -> Result<Box<dyn Modifier>, DefFileError> {
    let mut words = spec.split_whitespace();
    let (Some(attribute), Some(op), Some(value), Some(who), None) = (
        words.next(),
        words.next(),
        words.next(),
        words.next(),
        words.next(),
    ) else {
        return Err(DefFileError::line(
            line,
            format!(
                "`modifier = {spec}` is `<attribute> <op> <value> <who>`: an op of add, sub, mul, \
                 increase or set, and a who of target, source or effect"
            ),
        ));
    };
    let op = match op {
        "add" => ModOp::Add,
        "sub" => ModOp::Sub,
        "mul" => ModOp::Mul,
        "increase" => ModOp::Increase,
        "set" => ModOp::Set,
        op => {
            return Err(DefFileError::line(
                line,
                format!("`{op}` is not an op: add, sub, mul, increase or set"),
            ));
        }
    };
    let who = match who {
        "target" => EffectSubject::Target,
        "source" => EffectSubject::Source,
        "effect" => EffectSubject::Effect,
        who => {
            return Err(DefFileError::line(
                line,
                format!("`{who}` is not a subject: target, source or effect"),
            ));
        }
    };
    let value = value
        .parse::<f64>()
        .map_err(|_| DefFileError::line(line, format!("`{value}` is not a number")))?;
    names.modifier(line, attribute, value, op, who)
}

fn new_modifier<T: Attribute>(
    value: f64,
    op: ModOp,
    who: EffectSubject,
) -> Option<Box<dyn Modifier>> {
    let value = T::Property::from_f64(value)?;
    Some(Box::new(AttributeModifier::<T>::new(
        value,
        op,
        who,
        Arc::new(LiteralNode { value }),
    )))
}
