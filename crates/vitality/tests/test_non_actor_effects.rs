mod gameplay;

use gameplay::*;
use vitality::attributes::ManageAttributes;
use vitality::effect::modifier::{EffectSubject, ModOp};
use vitality::prelude::*;

#[test]
fn an_effect_folds_into_an_entity_that_is_not_an_actor() {
    let mut app = app();
    let post = app.world_mut().spawn(Health::new(100.0)).id();
    let blessing = effect(
        &mut app,
        EffectBuilder::permanent()
            .modify::<Health>(10.0, ModOp::Add, EffectSubject::Target)
            .build(),
    );

    apply(&mut app, post, post, &blessing);
    ticks(&mut app, 1);

    assert!(
        !has::<ManageAttributes>(&app, post),
        "the point of the cell is that nothing had to be added to the entity to fold into"
    );
    assert_eq!(
        with_effects(&mut app, move |e| e.on(post)).len(),
        1,
        "the effect is listed on the entity"
    );
    assert_eq!(
        base::<Health>(&app, post),
        100.0,
        "a fold never moves the base"
    );
    assert_eq!(
        current::<Health>(&app, post),
        110.0,
        "the entity's own attribute takes the +10 the same way an actor's would"
    );
}
