//! Every variant of `leaf/expr_shape` computes the same sum, so the group compares cost and nothing
//! else.

#[path = "../benches/support/common.rs"]
mod common;
#[path = "../benches/support/expr.rs"]
mod expr;

use expr::{NODE_COUNTS, eval_erased, eval_mono, eval_typemap, expected, expr_fixture};

#[test]
fn expr_values_match() {
    let fixture = expr_fixture();
    for (nodes, erased) in &fixture.erased {
        let want = expected(&fixture, *nodes);
        assert!(want != 0.0, "a zero sum would hide a missed read");
        assert_eq!(
            eval_erased(&fixture, erased) as f64,
            want,
            "erased, {nodes} nodes"
        );
        assert_eq!(
            eval_mono(&fixture, *nodes) as f64,
            want,
            "mono, {nodes} nodes"
        );
        assert_eq!(
            eval_typemap(&fixture, *nodes),
            want,
            "typemap, {nodes} nodes"
        );
    }
    assert_eq!(fixture.erased.len(), NODE_COUNTS.len());
}
