use crate::helpers::unused_arg_corrector;
use crate::helpers::variable_force::{self as vf, T, Tree};
use crate::prelude::*;

pub const NAME: &str = "Lint/UnusedBlockArgument";
pub static COP: UnusedBlockArgument = UnusedBlockArgument;
pub struct UnusedBlockArgument;

const UNDERSCORE: &str = "If it's necessary, use `_` or `_{}` as an argument name to indicate that it won't be used.";

fn underscore(name: &str) -> String {
    UNDERSCORE.replace("{}", name)
}

impl Cop for UnusedBlockArgument {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::PROGRAM_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let tree = Tree::build(&node.as_program_node().unwrap(), cx.src);
        let ignore_empty = cx.cfg.bool("IgnoreEmptyBlocks") == Some(true);
        let allow_kw = cx.cfg.bool("AllowUnusedKeywordArguments") == Some(true);
        let mut offenses: Vec<(usize, String)> = Vec::new();
        vf::investigate(&tree, |tree, scope, vars| {
            for &v in &scope.variables {
                let var = &vars[v];
                let decl_t = tree.t(var.declaration);
                let block = scope.scope.node;
                // allowed_block?
                if !var.is_block_argument(tree) || (ignore_empty && tree.child(block, 2).is_none()) {
                    continue;
                }
                if matches!(decl_t, T::Kwarg | T::Kwoptarg) && allow_kw {
                    continue;
                }
                let shadow = decl_t == T::Shadowarg;
                if shadow && !var.assignments.is_empty() {
                    continue;
                }
                if var.name.starts_with('_') || !var.references.is_empty() {
                    continue;
                }
                let kind = if shadow { "block local variable" } else { "block argument" };
                let mut msg = format!("Unused {kind} - `{}`.", var.name);
                if !shadow {
                    let all: Vec<_> = scope.variables.iter().map(|&i| &vars[i]).filter(|x| x.is_block_argument(tree)).collect();
                    let none_referenced = all.iter().all(|x| x.references.is_empty());
                    let send = tree.child(block, 0).map(|s| tree.nodes[s].name.as_str()).unwrap_or("");
                    let send_is_named = tree.child(block, 0).is_some_and(|s| tree.t(s) == T::Send);
                    let augmentation = if send_is_named && send == "lambda" {
                        let mut m = underscore(&var.name);
                        if none_referenced {
                            m.push_str(
                                " Also consider using a proc without arguments instead of a lambda if you want it to \
                                 accept any arguments but don't care about them.",
                            );
                        }
                        m
                    } else if none_referenced && !(send_is_named && send == "define_method") {
                        if all.len() > 1 {
                            "You can omit all the arguments if you don't care about them.".to_string()
                        } else {
                            "You can omit the argument if you don't care about it.".to_string()
                        }
                    } else {
                        underscore(&var.name)
                    };
                    msg = format!("{msg} {augmentation}");
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
