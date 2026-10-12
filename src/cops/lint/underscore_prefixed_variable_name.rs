use crate::prelude::*;
use helpers::unused_arg_corrector::name_range;
use helpers::variable_force::{self as vf, T, Tree};

pub const NAME: &str = "Lint/UnderscorePrefixedVariableName";
pub static COP: UnderscorePrefixedVariableName = UnderscorePrefixedVariableName;
pub struct UnderscorePrefixedVariableName;

const MSG: &str = "Do not use prefix `_` for a variable that is used.";

impl Cop for UnderscorePrefixedVariableName {
    fn kinds(&self) -> &'static [usize] {
        &[kind::PROGRAM_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let tree = Tree::build(&node.as_program_node().unwrap(), cx.src);
        let allow_kw = cx.cfg.bool("AllowKeywordBlockArguments") == Some(true);
        let mut ranges: Vec<(usize, usize)> = Vec::new();
        vf::investigate(&tree, |tree, scope, vars| {
            for &v in &scope.variables {
                let var = &vars[v];
                if !var.name.starts_with('_') || !var.references.iter().any(|r| r.explicit(tree)) {
                    continue;
                }
                let decl = var.declaration;
                if allow_kw && var.is_block_argument(tree) && matches!(tree.t(decl), T::Kwarg | T::Kwoptarg) {
                    continue;
                }
                ranges.push(if tree.t(decl) == T::MatchWithLvasgn {
                    match tree.child(decl, 0) {
                        Some(k) => tree.range(k),
                        None => continue,
                    }
                } else {
                    name_range(tree, decl)
                });
            }
        });
        for (s, e) in ranges {
            cx.add(s, e, MSG);
        }
    }
}
