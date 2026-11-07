# root_attribute
A simple RPG attribute framework written in rust for the Bevy game engine.

Gameplay attributes, effects and abilities for Bevy.

## Crates
| Crate | Role |
|---|---|
| `vitality_core` | `Attribute` trait and macros (`attribute!`, `tag!`), calculator, view traits, role markers |
| `vitality_effects` | effects, modifiers, conditions, dependency graph, effect registry |
| `vitality_abilities` | abilities, tasks, state machines, ability registry |
| `vitality_debug` | `bevy_ui` inspector overlay (optional) |
| `vitality` | facade: `AttributesPlugin`, actors, `Vitality` system param, examples |

Dependencies point one way: core <- effects <- abilities <- debug <- facade.

```toml
vitality = { path = "crates/vitality", default-features = false } # drops the bevy_ui overlay
```
