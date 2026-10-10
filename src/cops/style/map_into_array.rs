use crate::helpers::range_help::{Side, Surrounding, range_with_surrounding_space};
use crate::helpers::variable_force::{self as vf, T, Tree};
use crate::prelude::*;
use ruby_prism::CallNode;

pub const NAME: &str = "Style/MapIntoArray";
pub static COP: MapIntoArray = MapIntoArray;
pub struct MapIntoArray;

type R = (usize, usize);

/// One VariableForce variable, in `after_leaving_scope` order.
struct Var {
    name: String,
    decl: (R, T),

    assigns: Vec<R>,
    refs: Vec<R>,
    lvar_refs: Vec<R>,
}

#[derive(Default)]
struct Vars(Vec<Var>);

fn range(n: &Node) -> R {
    (n.location().start_offset(), n.location().end_offset())
}

fn single<'pr>(s: Option<ruby_prism::StatementsNode<'pr>>) -> Option<Node<'pr>> {
    let v: Vec<Node> = s?.body().iter().collect();
    (v.len() == 1).then(|| v.into_iter().next().unwrap())
}

/// `(send (lvar _) {:<< :push :append} #suitable_argument_node?)`: the push call.
fn push_call<'pr>(n: &Node<'pr>) -> Option<CallNode<'pr>> {
    let c = n.as_call_node()?;
    if !matches!(method_name(&c), "<<" | "push" | "append") || is_csend(&c) || c.block().is_some() {
        return None;
    }
    c.receiver()?.as_local_variable_read_node()?;
    let args = call_args(&c);
    let [arg] = args.as_slice() else { return None };
    let forwarded_kwrest = arg
        .as_keyword_hash_node()
        .is_some_and(|h| matches!(h.elements().iter().collect::<Vec<_>>().as_slice(), [e] if e.as_assoc_splat_node().is_some_and(|s| s.value().is_none())));
    let unsuitable = arg.as_splat_node().is_some() || arg.as_forwarding_arguments_node().is_some() || forwarded_kwrest;
    (!unsuitable).then_some(c)
}

/// `empty_array_asgn?` on a statement.
fn empty_array_asgn(n: &Node) -> bool {
    let Some(w) = n.as_local_variable_write_node() else { return false };
    let v = w.value();
    if let Some(a) = v.as_array_node() {
        return a.elements().iter().next().is_none();
    }
    let Some(c) = v.as_call_node() else { return false };
    if is_csend(&c) || c.block().is_some() {
        return false;
    }
    let args = call_args(&c);
    let empty_array = |a: &Node| a.as_array_node().is_some_and(|a| a.elements().iter().next().is_none());
    match (c.receiver(), method_name(&c)) {
        (Some(r), "[]") => helpers::parser_parent::is_top_const(&r, "Array") && args.is_empty(),
        (Some(r), "new") => {
            helpers::parser_parent::is_top_const(&r, "Array") && (args.is_empty() || (args.len() == 1 && empty_array(&args[0])))
        }
        (None, "Array") => args.len() == 1 && empty_array(&args[0]),
        _ => false,
    }
}

/// `return_value_used?` for the node whose Prism ancestors are `anc` (nearest last).
fn return_value_used(anc: &[Node], node: R) -> bool {
    let mut cur = node;
    let mut k = anc.len();
    while k > 0 {
        let p = &anc[k - 1];
        if let Some(s) = p.as_statements_node() {
            let stmts: Vec<Node> = s.body().iter().collect();
            let last = stmts.last().is_some_and(|l| range(l) == cur);
            let gp = k.checked_sub(2).map(|i| &anc[i]);
            let kwbegin = gp.and_then(|g| g.as_begin_node()).filter(|b| b.begin_keyword_loc().is_some());
            let handler = gp
                .and_then(|g| g.as_begin_node())
                .is_some_and(|b| b.rescue_clause().is_some() || b.ensure_clause().is_some());
            if handler {
                // (rescue body ...) uses the value; (ensure body ...) is a void context.
                if stmts.len() >= 2 && !last {
                    return false;
                }
                return gp.unwrap().as_begin_node().unwrap().rescue_clause().is_some();
            }
            if kwbegin.is_some() || gp.is_some_and(|g| g.as_parentheses_node().is_some()) {
                if !last {
                    return false;
                }
                cur = range(gp.unwrap());
                k -= 2;
                continue;
            }
            if stmts.len() >= 2 {
                if !last {
                    return false;
                }
                cur = range(p);
            }
            k -= 1;
            continue;
        }
        return match kind::of(p) {
            kind::PROGRAM_NODE => false,
            kind::BLOCK_NODE => {
                // BlockNode#void_context?: the owning call is `each` / `tap`.
                let owner = k.checked_sub(2).and_then(|i| anc[i].as_call_node());
                !owner.is_some_and(|c| matches!(method_name(&c), "each" | "tap"))
            }
            kind::DEF_NODE => {
                let d = p.as_def_node().unwrap();
                let name = std::str::from_utf8(d.name().as_slice()).unwrap_or("");
                let initialize = d.receiver().is_none() && name == "initialize";
                let setter = name.ends_with('=') && !matches!(name, "==" | "!=" | "<=" | ">=" | "===");
                !(initialize || setter)
            }
            kind::FOR_NODE | kind::ENSURE_NODE => false,
            _ => true,
        };
    }
    false
}

impl Cop for MapIntoArray {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::PROGRAM_NODE, kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if let Some(p) = node.as_program_node() {
            let tree = Tree::build(&p);
            let mut all = Vec::new();
            vf::investigate(&tree, |tree, scope, vars| {
                for &v in &scope.variables {
                    let var = &vars[v];
                    all.push(Var {
                        name: var.name.clone(),
                        decl: (tree.range(var.declaration), tree.t(var.declaration)),
                        assigns: var.assignments.iter().map(|a| tree.range(a.node)).collect(),
                        refs: var.references.iter().map(|r| tree.range(r.node)).collect(),
                        lvar_refs: var
                            .references
                            .iter()
                            .filter(|r| tree.t(r.node) == T::Lvar)
                            .map(|r| tree.range(r.node))
                            .collect(),
                    });
                }
            });
            cx.state::<Vars>().0 = all;
            return;
        }
        let each = node.as_call_node().unwrap();
        let Some(block) = each.block().and_then(|b| b.as_block_node()) else { return };
        if method_name(&each) != "each" || is_csend(&each) || each.arguments().is_some() {
            return;
        }
        if each.receiver().is_none_or(|r| r.as_self_node().is_some()) {
            return;
        }
        let Some(push_stmt) = block.body().and_then(|b| b.as_statements_node()).and_then(|s| single(Some(s))) else {
            return;
        };
        let Some(push) = push_call(&push_stmt) else { return };
        // ^({begin kwbegin block} ...)
        let anc = cx.ancestors;
        let n = anc.len();
        let Some(stmts) = anc.last().and_then(|p| p.as_statements_node()) else { return };
        let siblings: Vec<Node> = stmts.body().iter().collect();
        let gp = n.checked_sub(2).map(|i| &anc[i]);
        let parent_ok = siblings.len() >= 2
            || gp.is_some_and(|g| {
                matches!(kind::of(g), kind::BLOCK_NODE | kind::LAMBDA_NODE | kind::PARENTHESES_NODE)
                    || g.as_begin_node().is_some_and(|b| {
                        b.begin_keyword_loc().is_some() && b.rescue_clause().is_none() && b.ensure_clause().is_none()
                    })
            });
        if !parent_ok {
            return;
        }
        let block_r = range(node);
        let lvar = push.receiver().unwrap();
        let lvar_r = range(&lvar);
        let dest_name = std::str::from_utf8(lvar.as_local_variable_read_node().unwrap().name().as_slice()).unwrap_or("").to_string();
        let Some(dest) = cx.state::<Vars>().0.iter().find(|v| v.name == dest_name && v.lvar_refs.contains(&lvar_r)) else {
            return;
        };
        let (decl, assigns, refs) = (dest.decl.0, dest.assigns.clone(), dest.refs.clone());
        // empty_array_tap on the declaration: `[].tap { |dest| <this each block> }`.
        let tap = siblings.len() == 1
            && n >= 3
            && anc[n - 2].as_block_node().is_some_and(|b| {
                b.parameters().and_then(|p| p.as_block_parameters_node()).is_some_and(|bp| {
                    let args = helpers::block_args::children(&bp);
                    matches!(args.as_slice(), [a] if a.as_required_parameter_node().is_some() && range(a) == decl)
                })
            })
            && anc[n - 3].as_call_node().is_some_and(|c| {
                method_name(&c) == "tap"
                    && !is_csend(&c)
                    && c.arguments().is_none()
                    && c.receiver().is_some_and(|r| r.as_array_node().is_some_and(|a| a.elements().iter().next().is_none()))
            });
        let asgn = if tap {
            None
        } else {
            let Some(&asgn_r) = assigns.iter().rev().find(|a| a.1 < block_r.0) else { return };
            let Some(asgn) = siblings.iter().find(|s| range(s) == asgn_r && s.as_local_variable_write_node().is_some()) else {
                return;
            };
            if !empty_array_asgn(asgn) {
                return;
            }
            let joined = (asgn_r.0.min(block_r.0), asgn_r.1.max(block_r.1));
            let inside = |r: &R| joined.0 <= r.0 && r.1 <= joined.1;
            if refs.iter().filter(|r| inside(r)).count() != 1 || assigns.iter().filter(|r| inside(r)).count() != 1 {
                return;
            }
            Some(asgn_r)
        };
        let new_method = cx
            .config
            .for_cop("Style/CollectionMethods")
            .map("PreferredMethods")
            .and_then(|m| m.str("map"))
            .unwrap_or("map")
            .to_string();
        let msg = format!("Use `{new_method}` instead of `each` to map elements into an array.");
        let used = return_value_used(anc, block_r);
        let src = cx.src.text;
        let Some(sel) = each.message_loc() else { return };
        let sel = (sel.start_offset(), sel.end_offset());
        let tap_edits = tap.then(|| {
            let call = anc[n - 3].as_call_node().unwrap();
            let close = anc[n - 2].as_block_node().unwrap().closing_loc();
            let end = range_with_surrounding_space(
                src,
                close.start_offset(),
                close.end_offset(),
                Surrounding { side: Side::Left, ..Default::default() },
            );
            ((call.location().start_offset(), block_r.0), end)
        });
        let asgn_removal = asgn.map(|(s, e)| {
            let r = range_with_surrounding_space(src, s, e, Surrounding { side: Side::Right, ..Default::default() });
            range_with_surrounding_space(src, r.0, r.1, Surrounding { side: Side::Right, newlines: false, ..Default::default() })
        });
        let push_r = range(&push_stmt);
        let arg = call_args(&push).into_iter().next().unwrap();
        let arg_r = range(&arg);
        let wrap_hash = arg.as_keyword_hash_node().is_some();
        // block.right_sibling is `(lvar dest)`.
        let idx = siblings.iter().position(|s| range(s) == block_r);
        let next_lvar = idx.and_then(|i| siblings.get(i + 1)).and_then(|s| {
            let l = s.as_local_variable_read_node()?;
            (l.name().as_slice() == dest_name.as_bytes()).then(|| range(s))
        });
        let next_removal = next_lvar
            .map(|(s, e)| range_with_surrounding_space(src, s, e, Surrounding { side: Side::Left, ..Default::default() }));
        cx.add_fix(block_r.0, block_r.1, msg, |c| {
            if used {
                return;
            }
            c.replace(sel.0, sel.1, &new_method);
            if let Some((a, b)) = tap_edits {
                c.remove(a.0, a.1);
                c.remove(b.0, b.1);
            } else if let Some((s, e)) = asgn_removal {
                c.remove(s, e);
            }
            if wrap_hash {
                c.wrap(arg_r.0, arg_r.1, "{ ", " }");
            }
            c.remove(push_r.0, arg_r.0);
            c.remove(arg_r.1, push_r.1);
            if let Some((s, e)) = next_removal {
                c.remove(s, e);
            }
            c.insert_before(block_r.0, block_r.1, &format!("{dest_name} = "));
        });
    }
}
