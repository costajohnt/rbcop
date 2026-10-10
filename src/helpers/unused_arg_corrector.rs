//! RuboCop's UnusedArgCorrector (and `declaration_node.loc.name`) on `variable_force` trees.

use crate::helpers::range_help::{Side, Surrounding, range_with_surrounding_space};
use crate::helpers::variable_force::{T, Tree};

/// `loc.name` of an argument declaration: the name without `*`, `**`, `&`, `:` or default value.
pub fn name_range(tree: &Tree, decl: usize) -> (usize, usize) {
    let n = &tree.nodes[decl];
    let len = n.name.len();
    match n.t {
        T::Restarg | T::Kwrestarg | T::Blockarg => (n.end - len, n.end),
        _ => (n.start, n.start + len),
    }
}

/// The edit `UnusedArgCorrector.correct` makes, as (start, end, replacement); None for keyword
/// arguments (no edit, so the offense is not `[Correctable]`).
pub fn correction(src: &str, tree: &Tree, decl: usize) -> Option<(usize, usize, String)> {
    let n = &tree.nodes[decl];
    match n.t {
        T::Kwarg | T::Kwoptarg => None,
        T::Blockarg => {
            let opts = Surrounding { side: Side::Left, ..Default::default() };
            let (mut start, end) = range_with_surrounding_space(src, n.start, n.end, opts);
            while start > 0 && src.as_bytes()[start - 1] == b',' {
                start -= 1;
            }
            Some((start, end, String::new()))
        }
        _ => {
            let (s, e) = name_range(tree, decl);
            Some((s, e, format!("_{}", n.name)))
        }
    }
}
