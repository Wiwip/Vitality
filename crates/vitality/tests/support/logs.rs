#![allow(dead_code)]
//! Log capture for cells that pin a warning: keeps the level; pool threads go through a process registry.

use bevy::log::tracing::field::{Field, Visit};
use bevy::log::tracing::subscriber::{set_global_default, with_default};
use bevy::log::tracing::{Event, Level, Subscriber};
use bevy::log::tracing_subscriber::layer::{Context, SubscriberExt};
use bevy::log::tracing_subscriber::{Layer, Registry};
use std::sync::{Arc, Mutex, OnceLock};

/// the cells holding a [`Scope`] right now
static SCOPED: OnceLock<Mutex<Vec<Logs>>> = OnceLock::new();

static INSTALL: OnceLock<()> = OnceLock::new();

/// every record of the whole binary: `warn_once!` is per call site, so only the first cell sees it
static BINARY: OnceLock<Logs> = OnceLock::new();

#[derive(Clone, Default)]
pub struct Logs(Arc<Mutex<Vec<(Level, String)>>>);

impl Logs {
    /// see [`Logs::of_binary`]
    pub fn of_binary() -> Self {
        install();
        BINARY.get_or_init(Logs::default).clone()
    }

    /// a cell's own buffer; keep it alive past the cell body, as the capture drops with the guard
    pub fn scoped() -> Scope {
        install();
        let logs = Logs::default();
        scoped().lock().unwrap().push(logs.clone());
        Scope { logs }
    }

    /// every record as `LEVEL message`, which is what the message assertions match
    pub fn all(&self) -> String {
        self.render(|_| true)
    }

    /// how many WARN records carry `needle`, for the cells that count a `warn_once!`
    pub fn warn_count(&self, needle: &str) -> usize {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter(|(level, message)| level == &Level::WARN && message.contains(needle))
            .count()
    }

    /// the WARN records only, which is what tells a `warn!` from a `debug!`
    pub fn warns(&self) -> String {
        self.render(|level| level == &Level::WARN)
    }

    fn render(&self, keep: impl Fn(&Level) -> bool) -> String {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter(|(level, _)| keep(level))
            .map(|(level, message)| format!("{level:?} {message}\n"))
            .collect()
    }
}

/// one cell's capture; derefs to [`Logs`] for the assertions
pub struct Scope {
    logs: Logs,
}

impl Scope {
    /// installs this capture on the calling thread and in the registry for the pool-thread records
    pub fn run<T>(&self, f: impl FnOnce() -> T) -> T {
        let subscriber = Registry::default().with(Recorder(self.logs.clone()));
        with_default(subscriber, f)
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        let mut scoped = scoped().lock().unwrap();
        if let Some(pos) = scoped.iter().position(|l| Arc::ptr_eq(&l.0, &self.logs.0)) {
            scoped.swap_remove(pos);
        }
    }
}

impl std::ops::Deref for Scope {
    type Target = Logs;

    fn deref(&self) -> &Logs {
        &self.logs
    }
}

fn scoped() -> &'static Mutex<Vec<Logs>> {
    SCOPED.get_or_init(|| Mutex::new(Vec::new()))
}

/// bevy's LogPlugin owns the global subscriber, so the first install wins and a loss is harmless
fn install() {
    INSTALL.get_or_init(|| {
        let _ = set_global_default(Registry::default().with(Fanout));
    });
}

/// catches what no thread-local subscriber saw: a record fired on a task-pool thread
struct Fanout;

impl<S: Subscriber> Layer<S> for Fanout {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let record = record_of(event);
        if let Some(logs) = BINARY.get() {
            logs.0.lock().unwrap().push(record.clone());
        }
        for logs in scoped().lock().unwrap().iter() {
            logs.0.lock().unwrap().push(record.clone());
        }
    }
}

struct Recorder(Logs);

impl<S: Subscriber> Layer<S> for Recorder {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        self.0.0.lock().unwrap().push(record_of(event));
    }
}

fn record_of(event: &Event<'_>) -> (Level, String) {
    let mut message = Message(String::new());
    event.record(&mut message);
    (*event.metadata().level(), message.0)
}

struct Message(String);

impl Visit for Message {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.0 = format!("{value:?}");
        }
    }
}
