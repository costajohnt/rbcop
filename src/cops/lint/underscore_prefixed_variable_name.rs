use crate::prelude::*;
use helpers::parser_tree::{Tree, T};
use helpers::variable_force::VariableForce;

pub const NAME: &str = "Lint/UnderscorePrefixedVariableName";
pub static COP: UnderscorePrefixedVariableName = UnderscorePrefixedVariableName;
pub struct UnderscorePrefixedVariableName;

const MSG: &str = "Do not use prefix `_` for a variable that is used.";

impl Cop for UnderscorePrefixedVariableName {
    fn kinds(&self) -> &'static [usize] {
        &[kind::PROGRAM_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let tree = Tree::build(node);
        let vf = VariableForce::run(&tree);
        let allow_kw = cx.cfg.bool("AllowKeywordBlockArguments").unwrap_or(false);
        for &s in &vf.left {
            for &v in &vf.scopes[s].vars {
                let var = &vf.vars[v];
                if !var.name.starts_with('_') {
                    continue;
                }
                // Reference#explicit?: not a zsuper or `binding` send.
                if !var.references.iter().any(|r| !matches!(tree.t(r.node), T::Zsuper | T::Send)) {
                    continue;
                }
                let decl = var.decl;
                if allow_kw
                    && vf.is_argument(v)
                    && tree.t(vf.scopes[var.scope].node) == T::Block
                    && matches!(tree.t(decl), T::Kwarg | T::Kwoptarg)
                {
                    continue;
                }
                let range = if tree.t(decl) == T::MatchWithLvasgn {
                    tree.kid(decl, 0).map(|k| tree.range(k))
                } else {
                    tree.nodes[decl].name_loc
                };
                if let Some((s, e)) = range {
                    cx.add(s, e, MSG);
                }
            }
        }
    }
}
