use crate::helpers::const_name::const_name;
use crate::helpers::unused_arg_corrector;
use crate::helpers::variable_force::{self as vf, T, Tree};
use crate::prelude::*;
use ruby_prism::Visit;
use std::collections::HashMap;

pub const NAME: &str = "Lint/UnusedMethodArgument";
pub static COP: UnusedMethodArgument = UnusedMethodArgument;
pub struct UnusedMethodArgument;

/// Per def (keyed by start offset): body is nil, body is `not_implemented?`, body yields.
#[derive(Default)]
struct DefInfo {
    empty: bool,
    not_implemented: bool,
    yields: bool,
}

struct Defs<'a> {
    allowed: &'a [String],
    info: HashMap<usize, DefInfo>,
}

struct Yields(bool);

impl<'pr> Visit<'pr> for Yields {
    fn visit_yield_node(&mut self, _: &ruby_prism::YieldNode<'pr>) {
        self.0 = true;
    }
}

impl<'pr> Visit<'pr> for Defs<'_> {
    fn visit_def_node(&mut self, node: &ruby_prism::DefNode<'pr>) {
        let body = node.body();
        let mut y = Yields(false);
        if let Some(b) = &body {
            y.visit(b);
        }
        let single = body.as_ref().and_then(|b| b.as_statements_node()).and_then(|s| {
            let mut it = s.body().iter();
            let first = it.next()?;
            it.next().is_none().then_some(first)
        });
        // {(send nil? :raise #allowed_exception_class? ...) (send nil? :fail ...)}
        let not_implemented = single.and_then(|n| n.as_call_node()).is_some_and(|c| {
            if c.receiver().is_some() || c.block().is_some_and(|b| b.as_block_node().is_some()) {
                return false;
            }
            match c.name().as_slice() {
                b"fail" => true,
                b"raise" => send_args(&c).first().is_some_and(|a| {
                    const_name(a).is_some_and(|n| self.allowed.iter().any(|x| *x == n))
                }),
                _ => false,
            }
        });
        let start = node.location().start_offset();
        self.info.insert(start, DefInfo { empty: body.is_none(), not_implemented, yields: y.0 });
        ruby_prism::visit_def_node(self, node);
    }
}

impl Cop for UnusedMethodArgument {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::PROGRAM_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let allowed: Vec<String> = cx.cfg.strings("NotImplementedExceptions");
        let mut defs = Defs { allowed: &allowed, info: HashMap::new() };
        defs.visit(node);
        let tree = Tree::build(&node.as_program_node().unwrap());
        let allow_kw = cx.cfg.bool("AllowUnusedKeywordArguments") == Some(true);
        let ignore_empty = cx.cfg.bool("IgnoreEmptyMethods") == Some(true);
        let ignore_not_impl = cx.cfg.bool("IgnoreNotImplementedMethods") == Some(true);
        let mut offenses: Vec<(usize, String)> = Vec::new();
        vf::investigate(&tree, |tree, scope, vars| {
            for &v in &scope.variables {
                let var = &vars[v];
                if !var.is_method_argument(tree) {
                    continue;
                }
                let decl_t = tree.t(var.declaration);
                let keyword = matches!(decl_t, T::Kwarg | T::Kwoptarg);
                if keyword && allow_kw {
                    continue;
                }
                let def = scope.scope.node;
                let info = defs.info.get(&tree.nodes[def].start);
                if info.is_some_and(|i| (ignore_empty && i.empty) || (ignore_not_impl && i.not_implemented)) {
                    continue;
                }
                if decl_t == T::Blockarg && info.is_some_and(|i| i.yields) {
                    continue;
                }
                if var.name.starts_with('_') || !var.references.is_empty() {
                    continue;
                }
                let mut msg = format!("Unused method argument - `{}`.", var.name);
                if !keyword {
                    msg.push_str(&format!(
                        " If it's necessary, use `_` or `_{}` as an argument name to indicate that it won't be used. \
                         If it's unnecessary, remove it.",
                        var.name
                    ));
                }
                let none_referenced = scope
                    .variables
                    .iter()
                    .map(|&i| &vars[i])
                    .filter(|x| x.is_method_argument(tree))
                    .all(|x| x.references.is_empty());
                if none_referenced {
                    msg.push_str(&format!(
                        " You can also write as `{}(*)` if you want the method to accept any arguments but don't care \
                         about them.",
                        tree.nodes[def].name
                    ));
                }
                offenses.push((var.declaration, msg));
            }
        });
        let src = cx.src;
        for (decl, msg) in offenses {
            let (s, e) = unused_arg_corrector::name_range(&tree, decl);
            let fix = unused_arg_corrector::correction(src.text, &tree, decl);
            cx.add_fix(s, e, msg, |c| {
                if let Some((fs, fe, text)) = fix {
                    c.replace(fs, fe, &text);
                }
            });
        }
    }
}
