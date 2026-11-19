mod gameplay;

use gameplay::*;
use vitality::attribute_set;
use vitality::effect::modifier::{EffectSubject, ModOp};
use vitality::prelude::*;

attribute_set! { Hp: u32, Mp: u32 }

#[test]
fn a_set_declares_every_member_and_one_bundle_spawned_with_a_value_each() {
    let mut app = app();
    app.add_plugins((init_attribute::<Hp>, init_attribute::<Mp>));
    let carrier = app.world_mut().spawn(SetBundle::new(10, 4)).id();
    ticks(&mut app, 1);

    let blessing = effect(
        &mut app,
        EffectBuilder::permanent()
            .modify::<Hp>(5, ModOp::Add, EffectSubject::Target)
            .build(),
    );
    apply(&mut app, carrier, carrier, &blessing);
    ticks(&mut app, 1);

    assert_eq!(
        current::<Hp>(&app, carrier),
        15,
        "the bonus folded into the member the set declared reads back through current_value"
    );
    assert_eq!(
        current::<Mp>(&app, carrier),
        4,
        "the member no effect touches keeps the value it was spawned with"
    );
}
