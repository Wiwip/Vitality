//! Def files: the declarative half of an [`EffectDef`], written as data instead of Rust.
//!
//! Every line is `key = value`, `#` starts a comment, and a key may repeat where the def holds a list.
//! [`DefNames`] holds the names the app registered, spelled exactly as Rust does, and anything else
//! fails naming its line.
//!
//! An effect file holds `name`, `policy`, `tag`, `requires`, `blocked_by`, `cleanses` and
//! `modifier`, and a file with no `policy` is a permanent effect.
//!
//! A `.ron` effect file holds the same fields as an `EffectFile` struct and is refused the same way;
//! only the spelling differs.
//!
//! Expressions, scenes and observers stay Rust, so costs and clamps cannot come out of a file.

use crate::builder::EffectBuilder;
use crate::definition::EffectDef;
use crate::modifier::{AttributeModifier, EffectSubject, ModOp, Modifier};
use bevy_ecs::prelude::*;
use bevy_platform::collections::HashMap;
use express_it::nodes::LiteralNode;
use serde::Deserialize;
use smol_str::SmolStr;
use std::sync::Arc;
use std::time::Duration;
use vitality_core::attributes::Attribute;
use vitality_core::tags::TagSpec;
use vitality_core::util::pretty_type_name;
use vitality_core::value::AttributeValue;

/// one fn pointer builds each attribute's modifiers, so a file never learns the property type
type NewModifier = fn(value: f64, op: ModOp, who: EffectSubject) -> Option<Box<dyn Modifier>>;

/// What a def file is allowed to name. A name missing here fails the file that used it.
#[derive(Resource, Default)]
pub struct DefNames {
    attributes: HashMap<SmolStr, NewModifier>,
    tags: HashMap<SmolStr, TagSpec>,
}

impl DefNames {
    /// [`crate::init_effect_attribute`] calls this per attribute, so a file spells it as Rust does
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
        self.tag_at(&format!("line {line}"), name)
    }

    /// the tag named at `at`, which is a `line N` or a RON field path like `tags[0]`
    pub fn tag_at(&self, at: &str, name: &str) -> Result<TagSpec, DefFileError> {
        self.tags.get(name).copied().ok_or_else(|| {
            error_at(
                at,
                format!(
                    "no tag `{name}`: a def file only names tags the app put in DefNames with \
                     tag::<{name}>()"
                ),
            )
        })
    }

    fn modifier(
        &self,
        at: &str,
        name: &str,
        value: f64,
        op: ModOp,
        who: EffectSubject,
    ) -> Result<Box<dyn Modifier>, DefFileError> {
        let build = self.attributes.get(name).ok_or_else(|| {
            error_at(
                at,
                format!(
                    "no attribute `{name}`: a def file only names attributes the app registered \
                     with init_attribute::<{name}>()"
                ),
            )
        })?;
        build(value, op, who).ok_or_else(|| {
            error_at(
                at,
                format!(
                    "{value} is not a `{name}` value: a modifier on {name} has to be a value \
                     {name} itself holds"
                ),
            )
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

/// the same `{what}: {why}` shape for a RON file, whose places are field paths, not lines
fn error_at(at: &str, message: impl core::fmt::Display) -> DefFileError {
    DefFileError(format!("{at}: {message}"))
}

/// `key = value` lines of a def file, numbered, comments and blanks gone; the ability files share it
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

/// a def holds one of a scalar key, so a repeat is the usual paste of two defs into one
pub fn scalar_field(
    slot: &mut Option<(usize, String)>,
    line: usize,
    key: &str,
    value: String,
) -> Result<(), DefFileError> {
    if let Some((first, _)) = slot {
        return Err(DefFileError::line(
            line,
            format!("`{key}` is already set on line {first}, and a def file holds one {key}"),
        ));
    }
    *slot = Some((line, value));
    Ok(())
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
            "name" => scalar_field(&mut name, line, "name", value)?,
            "policy" => scalar_field(&mut policy, line, "policy", value)?,
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
    if let Some((_, name)) = name {
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

/// Reads a def file off disk; a bundle ships the same text through [`effect_def_from_str`].
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
    let at = format!("line {line}");
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
    let seconds = |word: &str, what: &str| match word.parse::<f32>() {
        Ok(value) => check_seconds(&at, word, value, what),
        Err(_) => Err(DefFileError::line(
            line,
            format!("`{word}` is not {what} in seconds"),
        )),
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

/// `<attribute> <op> <value> <who>`: an op of add, sub, mul, increase or set, a who of
/// target, source or effect.
fn modifier(names: &DefNames, line: usize, spec: &str) -> Result<Box<dyn Modifier>, DefFileError> {
    let at = format!("line {line}");
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
    let written = value;
    let value = value
        .parse::<f64>()
        .map_err(|_| DefFileError::line(line, format!("`{value}` is not a number")))?;
    finite_modifier(&at, attribute, written, value)?;
    names.modifier(&at, attribute, value, op, who)
}

/// nan or inf here turns the attribute non-finite and spreads to everything that reads it
fn finite_modifier(
    at: &str,
    attribute: &str,
    written: &str,
    value: f64,
) -> Result<(), DefFileError> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(error_at(
            at,
            format!(
                "`{written}` is not a finite number, which is what a modifier on {attribute} has \
                 to be"
            ),
        ))
    }
}

/// a seconds value a `Duration` can hold, asked the same way by both file formats
pub fn check_seconds(at: &str, written: &str, value: f32, what: &str) -> Result<f32, DefFileError> {
    // try_from_secs_f32 covers negative, NaN, infinite and the finite values too big for a
    // Duration, all of which panic inside bevy's Timer
    if Duration::try_from_secs_f32(value).is_ok() {
        Ok(value)
    } else {
        Err(error_at(
            at,
            format!(
                "`{written}` is not {what} in seconds, which has to be a finite number of 0 or \
                 more that a `Duration` can hold"
            ),
        ))
    }
}

/// a name the def can be printed with, asked the same way by both file formats
pub fn check_name(at: &str, name: &str) -> Result<(), DefFileError> {
    if name.is_empty() {
        return Err(error_at(
            at,
            "`\"\"` is not a name, which has to hold at least one character that the overlay can \
             print",
        ));
    }
    Ok(())
}

fn new_modifier<T: Attribute>(
    written: f64,
    op: ModOp,
    who: EffectSubject,
) -> Option<Box<dyn Modifier>> {
    let value = T::Property::from_f64(written);
    // from_f64 truncates, so `Hp add 0.5` built a modifier worth 0u32; a float only sheds mantissa
    if value.to_f64() != written && !holds_fractions::<T::Property>() {
        return None;
    }
    // the f64 literal fits but the narrower property does not: 1e300 saturates an f32 to inf
    if !value.to_f64().is_finite() {
        return None;
    }
    Some(Box::new(AttributeModifier::<T>::new(
        value,
        op,
        who,
        Arc::new(LiteralNode { value }),
    )))
}

/// how a file tells an integer property from a float one: an integer narrows 0.5 away, a float keeps it
fn holds_fractions<P: AttributeValue>() -> bool {
    P::from_f64(0.5).to_f64() == 0.5
}

/// The RON mirror of an effect file's keys, so both formats describe one `EffectDef`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EffectFile {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    policy: Option<RonPolicy>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    requires: Vec<String>,
    #[serde(default)]
    blocked_by: Vec<String>,
    #[serde(default)]
    cleanses: Vec<String>,
    #[serde(default)]
    modifiers: Vec<RonModifier>,
}

/// `Instant`, `Permanent`, `For(3.0)` or `Every(interval: 1.0, duration: 12.0)`; renamed so a
/// refusal names `Policy`, the file's word, not the internal type.
#[derive(Deserialize)]
#[serde(rename = "Policy", deny_unknown_fields)]
enum RonPolicy {
    Instant,
    Permanent,
    For(f32),
    Every {
        interval: f32,
        #[serde(default)]
        duration: Option<f32>,
    },
}

/// `Modifier(attribute: "Health", op: Add, value: 2.0, subject: Target)`; the file spells the
/// struct `Modifier`, which is the name ron matches a struct literal against.
#[derive(Deserialize)]
#[serde(rename = "Modifier", deny_unknown_fields)]
struct RonModifier {
    attribute: String,
    op: RonOp,
    value: f64,
    subject: RonSubject,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename = "Op")]
enum RonOp {
    Add,
    Sub,
    Mul,
    Increase,
    Set,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename = "Subject")]
enum RonSubject {
    Target,
    Source,
    Effect,
}

impl From<RonOp> for ModOp {
    fn from(op: RonOp) -> Self {
        match op {
            RonOp::Add => ModOp::Add,
            RonOp::Sub => ModOp::Sub,
            RonOp::Mul => ModOp::Mul,
            RonOp::Increase => ModOp::Increase,
            RonOp::Set => ModOp::Set,
        }
    }
}

impl From<RonSubject> for EffectSubject {
    fn from(who: RonSubject) -> Self {
        match who {
            RonSubject::Target => EffectSubject::Target,
            RonSubject::Source => EffectSubject::Source,
            RonSubject::Effect => EffectSubject::Effect,
        }
    }
}

/// the parser both RON loaders use: `implicit_some` so a file writes `name: "Fireball"`
pub fn ron_options() -> ron::Options {
    ron::Options::default().with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME)
}

/// what the RON loaders call element `index` of a list field, e.g. `modifiers[1]`
pub fn def_field(name: &str, index: usize) -> String {
    format!("{name}[{index}]")
}

/// Reads a RON def file into an [`EffectDef`], with the same refusals as [`effect_def_from_str`];
/// a failure names the field it tripped on, or ron's own position for a file it cannot parse.
///
/// ```
/// # use vitality_effects::prelude::*;
/// # use vitality_effects::def_file::{DefNames, effect_def_from_ron};
/// attribute!(Health, f32);
///
/// let mut names = DefNames::default();
/// names.attribute::<Health>();
/// let def = effect_def_from_ron(
///     r#"EffectFile(
///         name: "Regeneration",
///         policy: Every(interval: 1.0, duration: 12.0),
///         modifiers: [Modifier(attribute: "Health", op: Add, value: 2.0, subject: Target)],
///     )"#,
///     &names,
/// )
/// .unwrap();
/// assert_eq!(def.modifiers.len(), 1);
/// assert!(def.application_policy.is_periodic());
/// ```
pub fn effect_def_from_ron(source: &str, names: &DefNames) -> Result<EffectDef, DefFileError> {
    let file: EffectFile = ron_options().from_str(source).map_err(DefFileError::new)?;

    let mut builder = match file.policy {
        None => EffectBuilder::permanent(),
        Some(policy) => ron_policy(&policy)?,
    };
    if let Some(name) = file.name {
        check_name("name", &name)?;
        builder = builder.name(name);
    }
    let mut def = builder.build();
    for (index, tag) in file.tags.iter().enumerate() {
        def.granted_tags
            .push(names.tag_at(&def_field("tags", index), tag)?);
    }
    for (index, tag) in file.requires.iter().enumerate() {
        def.required_tags
            .push(names.tag_at(&def_field("requires", index), tag)?);
    }
    for (index, tag) in file.blocked_by.iter().enumerate() {
        def.blocked_by_tags
            .push(names.tag_at(&def_field("blocked_by", index), tag)?);
    }
    for (index, tag) in file.cleanses.iter().enumerate() {
        def.cleanses
            .push(names.tag_at(&def_field("cleanses", index), tag)?);
    }
    for (index, modifier) in file.modifiers.iter().enumerate() {
        let at = def_field("modifiers", index);
        let written = modifier.value.to_string();
        finite_modifier(&at, &modifier.attribute, &written, modifier.value)?;
        def.modifiers.push(names.modifier(
            &at,
            &modifier.attribute,
            modifier.value,
            modifier.op.into(),
            modifier.subject.into(),
        )?);
    }
    Ok(def)
}

/// Reads a RON def file off disk; a bundle ships the same text through [`effect_def_from_ron`].
pub fn effect_def_from_ron_file(
    path: impl AsRef<std::path::Path>,
    names: &DefNames,
) -> Result<EffectDef, DefFileError> {
    let source = def_file_source(path)?;
    effect_def_from_ron(&source, names)
}

fn ron_policy(policy: &RonPolicy) -> Result<EffectBuilder, DefFileError> {
    match policy {
        RonPolicy::Instant => Ok(EffectBuilder::instant()),
        RonPolicy::Permanent => Ok(EffectBuilder::permanent()),
        RonPolicy::For(seconds) => Ok(EffectBuilder::for_seconds(check_seconds(
            "policy",
            &seconds.to_string(),
            *seconds,
            "a duration",
        )?)),
        RonPolicy::Every { interval, duration } => {
            let interval =
                check_seconds("policy", &interval.to_string(), *interval, "an interval")?;
            match duration {
                None => Ok(EffectBuilder::every_second_permanently(interval)),
                Some(duration) => Ok(EffectBuilder::every_second_for_duration(
                    interval,
                    check_seconds(
                        "policy.duration",
                        &duration.to_string(),
                        *duration,
                        "a duration",
                    )?,
                )),
            }
        }
    }
}
