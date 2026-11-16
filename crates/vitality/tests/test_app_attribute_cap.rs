//! The facade's cap sentence, asserted at the facade. The unit test in `vitality_core` runs against a
//! bare `AttributeRegistry`, where the framework's own [`Stacks`] holds no slot, so the app-level half
//! of "an app gets 127" — one framework slot out of the 128, hence 127 game attributes — was pinned
//! nowhere, and a second framework-owned attribute would make the sentence wrong in silence.

use bevy::prelude::*;
use std::any::TypeId;
use vitality::attributes::ReflectAccessAttribute;
use vitality::effect::Stacks;
use vitality::{AttributesPlugin, attribute, init_attribute};
use vitality_core::registry::{AttrSet, AttributeRegistry};

/// The game side of one whole app, in the shape of `cap_pool!` in `vitality_core`'s registry tests.
macro_rules! cap_pool {
    ($( $name:ident ),+ $(,)?) => {
        $( attribute!($name); )+

        /// One entry per type, so a test registers a prefix of the pool instead of spelling it out.
        static GAME_ATTRIBUTES: &[fn(&mut App)] = &[$(init_attribute::<$name>),+];
    };
}

cap_pool!(
    Attr0, Attr1, Attr2, Attr3, Attr4, Attr5, Attr6, Attr7, Attr8, Attr9, Attr10, Attr11, Attr12,
    Attr13, Attr14, Attr15, Attr16, Attr17, Attr18, Attr19, Attr20, Attr21, Attr22, Attr23, Attr24,
    Attr25, Attr26, Attr27, Attr28, Attr29, Attr30, Attr31, Attr32, Attr33, Attr34, Attr35, Attr36,
    Attr37, Attr38, Attr39, Attr40, Attr41, Attr42, Attr43, Attr44, Attr45, Attr46, Attr47, Attr48,
    Attr49, Attr50, Attr51, Attr52, Attr53, Attr54, Attr55, Attr56, Attr57, Attr58, Attr59, Attr60,
    Attr61, Attr62, Attr63, Attr64, Attr65, Attr66, Attr67, Attr68, Attr69, Attr70, Attr71, Attr72,
    Attr73, Attr74, Attr75, Attr76, Attr77, Attr78, Attr79, Attr80, Attr81, Attr82, Attr83, Attr84,
    Attr85, Attr86, Attr87, Attr88, Attr89, Attr90, Attr91, Attr92, Attr93, Attr94, Attr95, Attr96,
    Attr97, Attr98, Attr99, Attr100, Attr101, Attr102, Attr103, Attr104, Attr105, Attr106, Attr107,
    Attr108, Attr109, Attr110, Attr111, Attr112, Attr113, Attr114, Attr115, Attr116, Attr117,
    Attr118, Attr119, Attr120, Attr121, Attr122, Attr123, Attr124, Attr125, Attr126, Attr127
);

/// The app the crate-doc sentence describes: [`AttributesPlugin`] installed under `ResolverMode::Dag`,
/// then `game` game attributes. Never ticked — the cap is a build-time counter, and registration is
/// what `AttributesPlugin::build` and `init_attribute` do.
fn app_with(game: usize) -> App {
    let mut app = App::new();
    app.insert_resource(vitality::ResolverMode::Dag)
        .add_plugins((MinimalPlugins, AssetPlugin::default(), AttributesPlugin));
    for register in &GAME_ATTRIBUTES[..game] {
        register(&mut app);
    }
    app
}

/// 127 game attributes register, and the registry is at the cap because [`Stacks`] took the 128th slot.
#[test]
fn an_app_gets_127_game_attributes_because_the_framework_holds_one_slot() {
    assert_eq!(
        GAME_ATTRIBUTES.len(),
        AttrSet::CAPACITY,
        "the pool is an app's 127 game attributes plus the one an app cannot register",
    );

    let app = app_with(AttrSet::CAPACITY - 1);
    let registry = app.world().resource::<AttributeRegistry>();

    assert!(
        registry.id_of(TypeId::of::<Stacks>()).is_some(),
        "AttributesPlugin registers the framework's own Stacks, and that is the slot the sentence counts"
    );
    assert_eq!(
        registry.len(),
        AttrSet::CAPACITY,
        "{} game attributes plus the framework's own attribute is an app at the cap, not one short \
         of it: the registry holds {}",
        AttrSet::CAPACITY - 1,
        registry.len(),
    );
}

/// The other half of the sentence: the 128th game attribute is the one refused, and the refusal names
/// both it and the cap. `catch_unwind` cannot read the payload in this profile (cranelift lets a caught
/// panic escape the catch), so `should_panic` matches the message.
#[test]
#[should_panic(expected = "register Attr127: the attribute registry holds at most 128 attributes")]
fn the_128th_game_attribute_is_the_one_the_app_refuses() {
    let _app = app_with(AttrSet::CAPACITY);
}
