#![allow(dead_code)]

//! Family B (`BENCH-PLAN.md` §2B): is the effect walk repeated once per attribute (T2)?
//!
//! One actor, A attributes, E permanent effects x 1 modifier each, and the only thing the two groups
//! change is how many of the A attributes carry a `Dirty<T>` when the timed tick runs: one
//! ([`Dirtied::One`]) or all ([`Dirtied::All`]). The deliverable is the ratio
//! `cost(all) / (A * cost(one))`, which [`print_ratios`] prints per A from the `estimates.json` files
//! criterion wrote for those very ids.
//!
//! The `dirty=` field of the id is the whole experiment, so [`recompute_shape`] proves it per
//! attribute instead of asserting an entity count. Two facts about the dirty set are load-bearing:
//!
//! * `build_app_with` hands back a world dirty on *every* attribute and unresolved (its
//!   `dirty_fraction` is applied *on top* of the markers the `Insert<T>` observers already put
//!   there), so the shape settles with a `tick` and then [`floor::clear_dirty`]. A shape that only
//!   ticked would be timing all-A resolve work while its id says `dirty=one`.
//! * an attribute that resolves re-dirties itself: `update_attribute` writes through `&mut T`
//!   (`vitality_core/src/attributes.rs:315`) and `on_change_notify_attribute_parents::<T>` in
//!   `EffectsSet::Notify` (`vitality_effects/src/systems.rs:274`) reads that as a change and triggers
//!   a fresh `MarkNodeDirty`. So `one_attr` is "one attribute *permanently* dirty" and `all_attrs` is
//!   the realistic steady state — the ids say `dirty=one` / `dirty=all` with that meaning, and
//!   [`assert_dirty_attrs`] after the timed loop is what keeps `dirty=one` from silently becoming
//!   `dirty=all`.

use crate::common::{
    ATTRIBUTE_POOL, AttrSpec, HarnessWorld, WorldSpec, actor_entities, build_app_with,
};
use crate::floor;
use bevy::prelude::{App, Entity, World};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// The A axis of §2B. 20 is the deepest attribute count the default pool serves without declaring a
/// second pool, and `common.rs` says so.
pub const RECOMPUTE_ATTR_COUNTS: [usize; 3] = [1, 5, 20];

/// The E axis of §2B: permanent effects, each carrying exactly one modifier.
pub const RECOMPUTE_EFFECT_COUNTS: [usize; 4] = [1, 5, 20, 100];

/// How many of the shape's attributes the timed tick finds dirty.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dirtied {
    /// The floor: the same world, nothing dirty. §2B's own formula subtracts `idle(A)`; this is that
    /// term, measured inside the shape instead of borrowed from `floor/idle_tick`'s 1000 actors.
    None,
    One,
    All,
}

impl Dirtied {
    /// The `dirty=` field of the id.
    pub fn tag(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::One => "one",
            Self::All => "all",
        }
    }

    /// The group this shape is timed in.
    pub fn group(self) -> &'static str {
        match self {
            Self::None => "recompute/none_dirty",
            Self::One => "recompute/one_attr",
            Self::All => "recompute/all_attrs",
        }
    }

    /// The attributes of `pool` this shape leaves dirty. `One` is always the pool's first entry, so
    /// the ratio's two sides dirty the same attribute of the same generated world.
    pub fn pick<'a>(self, pool: &'a [AttrSpec]) -> &'a [AttrSpec] {
        match self {
            Self::None => &pool[..0],
            Self::One => &pool[..1],
            Self::All => pool,
        }
    }
}

/// The shape §2B times: `actors` is 1 because the ratio is about one actor's A attribute storages,
/// and `dirty_fraction` is 0 because the dirty set is set per attribute by [`recompute_shape`], never
/// by the generator's seeded fraction.
pub fn recompute_spec(attributes: usize, effects: usize) -> WorldSpec {
    WorldSpec {
        attributes_per_actor: attributes,
        effects_per_actor: effects,
        modifiers_per_effect: 1,
        actors: 1,
        dirty_fraction: 0.0,
        ticking_fraction: 0.0,
    }
}

/// The id §2B wants: `common::shape_id` with its `dirty=` field replaced by the set the shape was
/// proved to carry. Replacing rather than reformatting, so the axis order and every other field stay
/// `common.rs`'s.
pub fn dirtied_id(spec: &WorldSpec, dirtied: Dirtied) -> String {
    let plain = spec.shape_id();
    let id = plain.replace("/dirty=0", &format!("/dirty={}", dirtied.tag()));
    assert!(
        id != plain,
        "shape {plain} carries no /dirty=0 field, so the dirty set cannot be put in its id: the \
         generator would be leaving actors dirty that the id does not count"
    );
    id
}

/// A settled shape with its dirty set marked and proved. Held whole: the benches time
/// `run_fixed_tick` on `app` for hundreds of samples, so the world has to outlive the closure.
pub struct RecomputeShape {
    pub app: App,
    pub actor: Entity,
    pub dirtied: Dirtied,
    /// `Dirty<T>` components [`floor::clear_dirty`] removed inside [`floor::prepare_floor`]. Zero
    /// would mean the shape never had markers to clear, i.e. the build did not dirty anything and the
    /// positive control in `test_recompute_ratio` is wrong.
    pub markers_cleared: usize,
}

/// Builds `spec` from the default pool, settles it with [`floor::prepare_floor`] (tick, clear, one
/// idle `run_fixed_tick` that must leave zero dirty), marks the attributes [`Dirtied::pick`] names,
/// and asserts the per-attribute dirty state it claims.
pub fn recompute_shape(spec: &WorldSpec, dirtied: Dirtied) -> RecomputeShape {
    let mut app = build_app_with(spec, ATTRIBUTE_POOL);
    let actors = actor_entities(app.world_mut());
    assert_eq!(
        actors.len(),
        spec.actors,
        "shape {} built {} actors: the ratio holds the actor count at 1 so it prices one actor's A \
         attribute storages",
        spec.shape_id(),
        actors.len(),
    );
    let actor = actors[0];
    let markers_cleared = floor::prepare_floor(&mut app);
    let pool = app.world().resource::<HarnessWorld>().attrs();
    assert_eq!(
        pool.len(),
        spec.attributes_per_actor,
        "shape {} registered {} attributes",
        spec.shape_id(),
        pool.len(),
    );
    for attr in dirtied.pick(pool) {
        attr.mark_dirty(app.world_mut(), actor);
        // `AttrSpec::mark_dirty` is a `World::trigger`, and the observer's `try_insert(Dirty<T>)` sits
        // in the world's deferred queue until something flushes it: without this the mark is invisible.
        app.world_mut().flush();
    }
    assert_dirty_attrs(app.world(), pool, actor, dirtied, "after marking");
    RecomputeShape {
        app,
        actor,
        dirtied,
        markers_cleared,
    }
}

/// The attributes of `pool` carrying a `Dirty<T>` on `actor`, by name. Per attribute, because an
/// entity count cannot tell "the one I marked" from "everything the fan-out re-marked".
pub fn dirty_names(world: &World, pool: &[AttrSpec], actor: Entity) -> Vec<&'static str> {
    pool.iter()
        .filter(|attr| attr.is_dirty(world, actor))
        .map(|attr| attr.name)
        .collect()
}

/// The dirty set on `actor` is exactly [`Dirtied::pick`]'s. Every call site names the moment it
/// checks, because the same call runs before marking, after marking and after the timed loop.
pub fn assert_dirty_attrs(
    world: &World,
    pool: &[AttrSpec],
    actor: Entity,
    dirtied: Dirtied,
    when: &str,
) {
    let expected: Vec<&'static str> = dirtied.pick(pool).iter().map(|a| a.name).collect();
    let got = dirty_names(world, pool, actor);
    assert_eq!(
        got,
        expected,
        "{when}: id `dirty={}` holds {} dirty attribute(s) {got:?}, expected the {} of {:?}",
        dirtied.tag(),
        got.len(),
        dirtied.tag(),
        expected,
    );
}

/// The file criterion 0.8 writes for one benchmark: `<root>/<group>/<id>/new/estimates.json`, where
/// every `/` of the id becomes `_` (`criterion-0.8.2/src/report.rs:172`,
/// `BenchmarkId::directory_name`). Kept separate so a missing estimate can print the path it wanted.
fn estimates_path(root: &Path, group: &str, bench_id: &str) -> PathBuf {
    root.join(group.replace('/', "_"))
        .join(bench_id.replace('/', "_"))
        .join("new")
        .join("estimates.json")
}

/// Where criterion writes its output. Criterion resolves its own target directory rather than using
/// one relative to the bench's cwd, so the only reliable way to find it is to look: `CARGO_TARGET_DIR`
/// first, then `target/criterion` in the cwd and in every ancestor of it. `None` means nothing this
/// process can see was written there, which `print_ratios` says out loud rather than printing a ratio
/// it did not measure.
pub fn criterion_root() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(target) = std::env::var("CARGO_TARGET_DIR") {
        candidates.push(PathBuf::from(target).join("criterion"));
    }
    let Ok(cwd) = std::env::current_dir() else {
        return None;
    };
    candidates.push(cwd.join("target/criterion"));
    let mut ancestor = cwd;
    while ancestor.pop() {
        candidates.push(ancestor.join("target/criterion"));
    }
    candidates.into_iter().find(|path| path.is_dir())
}

/// The two numbers a ratio row needs out of one `estimates.json`: `mean.point_estimate` and half of
/// its 95 % CI. A ratio is a difference of two means, and a difference is only a difference when it
/// clears both half-widths added together — which is how `-0.33` and `1.65` got printed at this tip.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Estimate {
    pub ns: f64,
    pub ci_half: f64,
}

impl Estimate {
    /// Whether the gap between two means is inside the noise both of them carry.
    pub fn indistinguishable_from(&self, other: &Self) -> bool {
        (self.ns - other.ns).abs() <= self.ci_half + other.ci_half
    }
}

/// Why [`estimate`] refused to hand back a number. Every variant carries the path, because the
/// failure this loop actually hit was reading a `target/criterion` tree an *older* bench binary wrote
/// and calling the result measured.
pub enum Refusal {
    /// No file there: the benchmark did not run, or criterion was told not to save a baseline.
    Missing(PathBuf),
    /// Older than the running binary, so some other build wrote it.
    Stale(PathBuf),
    /// A file is there and its `mean` is not what criterion writes.
    Unparsable(PathBuf),
}

impl Refusal {
    fn explain(&self) -> String {
        match self {
            Self::Missing(path) => format!("no estimate at {}", path.display()),
            Self::Stale(path) => format!(
                "{} is older than this bench binary, so it was written by an earlier build",
                path.display()
            ),
            Self::Unparsable(path) => format!(
                "{} has no readable mean.point_estimate + confidence_interval",
                path.display()
            ),
        }
    }
}

/// mtime of the executable running the bench: the stamp every estimate has to be younger than.
fn binary_mtime() -> Option<SystemTime> {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.metadata().ok())
        .and_then(|meta| meta.modified().ok())
}

/// The estimate criterion wrote for `group`/`bench_id`, or the reason there is none. Refuses a file
/// older than this binary rather than printing a ratio made of an older build's numbers; criterion
/// writes the file as its benchmark finishes, so in a real run every estimate of the run is newer than
/// the binary that produced it.
pub fn estimate(root: &Path, group: &str, bench_id: &str) -> Result<Estimate, Refusal> {
    let path = estimates_path(root, group, bench_id);
    let meta = match path.metadata() {
        Ok(meta) => meta,
        Err(_) => return Err(Refusal::Missing(path)),
    };
    if let Some(built) = binary_mtime() {
        if meta.modified().is_ok_and(|written| written < built) {
            return Err(Refusal::Stale(path));
        }
    }
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(_) => return Err(Refusal::Unparsable(path)),
    };
    parse_estimate(&text).ok_or(Refusal::Unparsable(path))
}

/// Reads the top-level `mean` member of an estimates file. Hand-written because `serde_json` is not a
/// dependency of this crate — the workspace declares `serde` only, and the `serde_json` in the
/// lockfile belongs to criterion, and `crates/vitality/Cargo.toml` is a protected path this loop may
/// not edit — and because the first version of this scan searched for `"point_estimate"` after
/// `"mean"`, which is the *nested* key the moment a CI carries one. So: walk members with the depth
/// tracked, take only a top-level `mean`, only its own `point_estimate`, and refuse anything that does
/// not parse instead of returning a prefix of a number.
pub fn parse_estimate(text: &str) -> Option<Estimate> {
    let mut scan = Scan::new(text.as_bytes());
    scan.ws();
    scan.expect(b"{")?;
    loop {
        scan.ws();
        if scan.eat(b'}') {
            return None;
        }
        let key = scan.string()?;
        scan.ws();
        scan.expect(b":")?;
        if key == "mean" {
            let (ns, ci_half) = scan.mean()?;
            return Some(Estimate { ns, ci_half });
        }
        scan.skip_value()?;
        scan.ws();
        if scan.eat(b',') {
            continue;
        }
        return None;
    }
}

/// Position-tracked byte cursor, just enough JSON to read one member of one object.
struct Scan<'a> {
    text: &'a [u8],
    at: usize,
}

impl<'a> Scan<'a> {
    fn new(text: &'a [u8]) -> Self {
        Self { text, at: 0 }
    }

    fn ws(&mut self) {
        while self.at < self.text.len()
            && matches!(self.text[self.at], b' ' | b'\t' | b'\n' | b'\r')
        {
            self.at += 1;
        }
    }

    fn eat(&mut self, byte: u8) -> bool {
        if self.text.get(self.at) == Some(&byte) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, token: &[u8]) -> Option<()> {
        if self.text.get(self.at..self.at + token.len())? == token {
            self.at += token.len();
            Some(())
        } else {
            None
        }
    }

    /// A JSON string, unescaped only. An escape would mean a key criterion does not write.
    fn string(&mut self) -> Option<&'a str> {
        self.expect(b"\"")?;
        let start = self.at;
        while self.at < self.text.len() && self.text[self.at] != b'"' {
            self.at += 1;
        }
        let body = std::str::from_utf8(self.text.get(start..self.at)?).ok()?;
        self.expect(b"\"")?;
        if body.contains('\\') {
            return None;
        }
        Some(body)
    }

    fn number(&mut self) -> Option<f64> {
        let start = self.at;
        self.eat(b'-');
        while self.at < self.text.len() && self.text[self.at].is_ascii_digit() {
            self.at += 1;
        }
        if self.eat(b'.') {
            while self.at < self.text.len() && self.text[self.at].is_ascii_digit() {
                self.at += 1;
            }
        }
        if self.at == start || self.text[start..self.at] == *b"-" {
            return None;
        }
        std::str::from_utf8(self.text.get(start..self.at)?)
            .ok()?
            .parse()
            .ok()
    }

    /// `mean`: its own `point_estimate`, and the bounds of its own `confidence_interval`, in whichever
    /// order criterion writes them.
    fn mean(&mut self) -> Option<(f64, f64)> {
        self.ws();
        self.expect(b"{")?;
        let mut point = None;
        let mut bounds = None;
        loop {
            self.ws();
            if self.eat(b'}') {
                break;
            }
            let key = self.string()?;
            self.ws();
            self.expect(b":")?;
            match key {
                "point_estimate" => point = Some(self.number()?),
                "confidence_interval" => bounds = Some(self.bounds()?),
                _ => self.skip_value()?,
            }
            self.ws();
            if self.eat(b',') {
                continue;
            }
            self.expect(b"}")?;
            break;
        }
        let (lower, upper) = bounds?;
        Some((point?, (upper - lower) / 2.0))
    }

    /// `confidence_interval`'s bounds, read by key so a nested `point_estimate` in there is skipped
    /// rather than mistaken for the mean's.
    fn bounds(&mut self) -> Option<(f64, f64)> {
        self.ws();
        self.expect(b"{")?;
        let (mut lower, mut upper) = (None, None);
        loop {
            self.ws();
            if self.eat(b'}') {
                break;
            }
            let key = self.string()?;
            self.ws();
            self.expect(b":")?;
            match key {
                "lower_bound" => lower = Some(self.number()?),
                "upper_bound" => upper = Some(self.number()?),
                _ => self.skip_value()?,
            }
            self.ws();
            if self.eat(b',') {
                continue;
            }
            self.expect(b"}")?;
            break;
        }
        Some((lower?, upper?))
    }

    /// Consumes a value without caring what it holds, tracking depth so a `}` inside a nested object
    /// cannot end the member walk.
    fn skip_value(&mut self) -> Option<()> {
        self.ws();
        match self.text.get(self.at)? {
            b'{' | b'[' => {
                let (open, close) = match self.text[self.at] {
                    b'{' => (b'{', b'}'),
                    _ => (b'[', b']'),
                };
                let mut depth = 0usize;
                let mut in_string = false;
                while self.at < self.text.len() {
                    let byte = self.text[self.at];
                    self.at += 1;
                    if in_string {
                        if byte == b'"' {
                            in_string = false;
                        }
                    } else {
                        match byte {
                            b'"' => in_string = true,
                            b if b == open => depth += 1,
                            b if b == close => {
                                depth -= 1;
                                if depth == 0 {
                                    return Some(());
                                }
                            }
                            _ => {}
                        }
                    }
                }
                None
            }
            b'"' => {
                self.string()?;
                Some(())
            }
            _ => {
                while self.at < self.text.len()
                    && !matches!(
                        self.text[self.at],
                        b',' | b'}' | b']' | b' ' | b'\n' | b'\r' | b'\t'
                    )
                {
                    self.at += 1;
                }
                Some(())
            }
        }
    }
}

/// One measured point of one side of the ratio: the shape and the criterion id it was timed under.
pub struct Point {
    pub spec: WorldSpec,
    pub bench_id: String,
}

/// Prints, per E, the raw `cost(all) / (A * cost(one))` and the floor-subtracted
/// `(all - none) / (A * (one - none))`, reading the estimates criterion wrote for the ids it just
/// timed. This *is* the §2B deliverable: the raw `time:` lines in criterion's own summary carry the
/// shape, so nothing here restates them.
///
/// Four rules, all of them earned by a wrong number that got printed before:
///
/// * a row reads all three sides and asserts the three shapes are one shape, so an `all` cannot be
///   paired with a `one` from another A or E;
/// * an estimate older than this running binary, or missing, is an `ERROR` line and no number. A
///   `target/criterion` tree left by an earlier build is otherwise indistinguishable from a
///   measurement, and this loop read one;
/// * every level carries its CI half-width, and a ratio whose denominator sits inside the half-widths
///   of the two means it is made of prints `noise` rather than a figure — the `-0.33` and `1.65`
///   marginals of an earlier run were both inside theirs;
/// * A=1 rows print and never enter a median: `Dirtied::pick(One) == pick(All)` when
///   `attributes_per_actor == 1`, so both sides of those rows are the same benchmark and their ratio
///   is arithmetic on an identity, not a measurement.
///
/// §2B's own row subtracts `idle(A)` — "so T1's floor is not counted as walk cost" — so the marginal
/// is the ratio §2B means, and the raw one is kept only because it is the number P-6's item text asks
/// for. It is not a small correction: at A=20 the idle schedule is most of `one`, so the raw ratio is
/// arithmetic on dead work.
///
/// Called after every side of the A has been timed, because the files only exist once criterion has
/// finished the benchmark that writes them.
pub fn print_ratios(a: usize, none: &[Point], one: &[Point], all: &[Point]) {
    assert_eq!(
        none.len(),
        all.len(),
        "the sides of the ratio table hold {} and {} points",
        none.len(),
        all.len()
    );
    let rows = none.len();
    let Some(root) = criterion_root() else {
        println!(
            "recompute ratio table (A={a}): ERROR no criterion output under CARGO_TARGET_DIR or any \
             ancestor's target/criterion, so no ratio can be reported"
        );
        return;
    };
    println!(
        "recompute ratio table (A={a}, ns per run_fixed_tick, estimates under {root:?}): each level is \
         the mean ± half its 95 % CI; marginal=(all-none)/(A*(one-none)) is BENCH-PLAN §2B's ratio, \
         none_dirty being the same world with zero attributes dirty (its idle schedule, §2A); \
         raw=all/(A*one) is the item's simpler form, which counts that idle schedule as walk cost; \
         `noise` means the denominator is inside the half-widths of the means it is made of, so the \
         row carries no number:"
    );
    let mut raws = Vec::new();
    let mut marginals = Vec::new();
    for ((none, one), all) in none.iter().zip(one.iter()).zip(all.iter()) {
        assert_eq!(
            one.spec.shape_id(),
            all.spec.shape_id(),
            "the sides of the ratio were measured on different shapes"
        );
        assert_eq!(
            none.spec.shape_id(),
            one.spec.shape_id(),
            "the floor side of the ratio is a different shape from the one-attribute side, so the \
             subtraction is not idle-work-vs-walk on one world"
        );
        // The full criterion ids of the row, twice over on purpose: no number may travel without the
        // shape it came from (BENCH-PLAN §1.5), and the row's `dirty=` must match the ids it reads.
        let id = format!(
            "{} vs {}",
            full_id(Dirtied::One, &one.bench_id),
            full_id(Dirtied::All, &all.bench_id)
        );
        let read = |dirtied: Dirtied, point: &Point| {
            estimate(&root, dirtied.group(), &point.bench_id).map_err(|refusal| refusal.explain())
        };
        let (none_ns, one_ns, all_ns) = match (
            read(Dirtied::None, none),
            read(Dirtied::One, one),
            read(Dirtied::All, all),
        ) {
            (Ok(none), Ok(one_side), Ok(all_side)) => (none, one_side, all_side),
            (none_side, one_side, all_side) => {
                for reason in [none_side.err(), one_side.err(), all_side.err()]
                    .into_iter()
                    .flatten()
                {
                    println!("  {id}: ERROR {reason} — no ratio for this row");
                }
                continue;
            }
        };
        println!(
            "  {id}: none={:.0}±{:.0} one={:.0}±{:.0} all={:.0}±{:.0}",
            none_ns.ns, none_ns.ci_half, one_ns.ns, one_ns.ci_half, all_ns.ns, all_ns.ci_half
        );
        let (raw_text, marginal_text) = if one_ns.indistinguishable_from(&none_ns) {
            (String::from("noise"), String::from("noise"))
        } else {
            let walk = one_ns.ns - none_ns.ns;
            let raw = all_ns.ns / (a as f64 * one_ns.ns);
            raws.push(raw);
            let marginal = if walk <= 0.0 {
                String::from("no ratio (a dirty tick cost less than the clean one, outside noise)")
            } else {
                let ratio = (all_ns.ns - none_ns.ns) / (a as f64 * walk);
                marginals.push(ratio);
                format!("{ratio:.2}")
            };
            (format!("{raw:.2}"), marginal)
        };
        println!("  {id}: raw={raw_text} marginal={marginal_text}");
    }
    if a == 1 {
        println!(
            "  A=1: no median. Dirtied::pick(One) == pick(All) when attributes_per_actor == 1, so the \
             two sides of every row above are the same benchmark run twice: their ratio is 1 by \
             construction, not by measurement."
        );
        return;
    }
    println!(
        "  A={a}: raw {}; marginal {}",
        ratios_in_words(&raws, rows),
        ratios_in_words(&marginals, rows)
    );
}

/// `median 0.07 over 4 rows, min 0.06, max 0.12` — or the reason there is no number. Rows that came
/// back `noise` or `ERROR` are not in the list, and a median over part of the grid is a number the run
/// did not measure.
fn ratios_in_words(ratios: &[f64], rows: usize) -> String {
    if ratios.len() != rows {
        return format!(
            "no median ({} of {rows} rows carried a number; the rest are noise or ERROR above)",
            ratios.len()
        );
    }
    let mut sorted = ratios.to_vec();
    sorted.sort_by(|x, y| x.partial_cmp(y).expect("ratio of two positive durations"));
    format!(
        "median {:.2} over {rows} rows, min {:.2}, max {:.2}",
        sorted[sorted.len() / 2],
        sorted[0],
        sorted[sorted.len() - 1]
    )
}

/// The id criterion printed: group + bench id, so a ratio row and a `time:` line are the same string.
fn full_id(dirtied: Dirtied, bench_id: &str) -> String {
    format!(
        "{dirtied_group}/{bench_id}",
        dirtied_group = dirtied.group()
    )
}
