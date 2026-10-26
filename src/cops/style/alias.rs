use crate::prelude::*;

pub const NAME: &str = "Style/Alias";
pub static COP: Alias = Alias;
pub struct Alias;

#[derive(PartialEq)]
enum Scope {
    Lexical,
    Dynamic,
    InstanceEval,
}

/// Alias#scope_type over Prism ancestors (innermost last). `own_block` is the method name when the
/// node itself carries a literal block (its parser parent).
fn scope_type(ancestors: &[Node], own_block: Option<&str>) -> Scope {
    let block_scope = |name: &str| if name == "instance_eval" { Scope::InstanceEval } else { Scope::Dynamic };
    if let Some(name) = own_block {
        return block_scope(name);
    }
    for a in ancestors.iter().rev() {
        match a {
            Node::ClassNode { .. } | Node::ModuleNode { .. } => return Scope::Lexical,
            Node::DefNode { .. } | Node::LambdaNode { .. } => return Scope::Dynamic,
            Node::CallNode { .. } => {
                let c = a.as_call_node().unwrap();
                if c.block().is_some_and(|b| b.as_block_node().is_some()) {
                    return block_scope(method_name(&c));
                }
            }
            _ => {}
        }
    }
    Scope::Lexical
}

/// A symbol's value (`sym_node.value.to_s`).
fn sym_value(node: &Node) -> Option<String> {
    node.as_symbol_node().map(|s| String::from_utf8_lossy(s.unescaped()).into_owned())
}

/// Alias#bareword_symbol?: a symbol that can be written as a bareword.
fn bareword_symbol(node: &Node) -> bool {
    let Some(value) = sym_value(node) else { return false };
    let mut chars = value.chars();
    let ident = chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') && {
        let rest = chars.as_str();
        let rest = rest.strip_suffix(['?', '!', '=']).unwrap_or(rest);
        rest.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    };
    if ident {
        return true;
    }
    let src = node_text(node);
    !src.starts_with(":\"") && !src.starts_with(":'") && !value.starts_with(['@', '$'])
}

/// Alias#identifier: `:name` for a bareword symbol, the source otherwise.
fn identifier(node: &Node) -> String {
    if bareword_symbol(node) { format!(":{}", sym_value(node).unwrap()) } else { node_text(node).to_string() }
}

impl Cop for Alias {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE, kind::ALIAS_METHOD_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let prefer_alias = cx.cfg.str("EnforcedStyle") == Some("prefer_alias");
        if let Some(al) = node.as_alias_method_node() {
            let scope = scope_type(cx.ancestors, None);
            // alias_method_possible?: not in instance_eval, no `def` ancestor (defs does not count).
            if scope == Scope::InstanceEval
                || cx.ancestors.iter().any(|a| a.as_def_node().is_some_and(|d| d.receiver().is_none()))
            {
                return;
            }
            let (new, old) = (al.new_name(), al.old_name());
            if scope == Scope::Dynamic || !prefer_alias {
                let kw = al.keyword_loc();
                let replacement = format!("alias_method {}, {}", identifier(&new), identifier(&old));
                let (s, e) = (node.location().start_offset(), node.location().end_offset());
                cx.add_fix(kw.start_offset(), kw.end_offset(), "Use `alias_method` instead of `alias`.", |c| {
                    c.replace(s, e, &replacement)
                });
                return;
            }
            let bareword = |n: &Node| !node_text(n).starts_with(':') || n.as_interpolated_symbol_node().is_some();
            if ![&new, &old].iter().all(|n| !bareword(n) && bareword_symbol(n)) {
                return;
            }
            let (nt, ot) = (sym_value(&new).unwrap(), sym_value(&old).unwrap());
            let msg = format!("Use `alias {nt} {ot}` instead of `alias {} {}`.", node_text(&new), node_text(&old));
            let (ns, ne) = (new.location().start_offset(), new.location().end_offset());
            let (os, oe) = (old.location().start_offset(), old.location().end_offset());
            cx.add_fix(ns, oe, msg, |c| {
                c.replace(ns, ne, &nt);
                c.replace(os, oe, &ot);
            });
            return;
        }
        let call = node.as_call_node().unwrap();
        if call.receiver().is_some() || method_name(&call) != "alias_method" || !prefer_alias {
            return;
        }
        let own_block = call.block().and_then(|b| b.as_block_node()).map(|_| "alias_method");
        if scope_type(cx.ancestors, own_block) == Scope::Dynamic {
            return;
        }
        let args = send_args(&call);
        if !args.iter().all(|a| a.as_symbol_node().is_some()) || args.len() != 2 {
            return;
        }
        // alias_method_value_used?: an argument of a send, or the value of an assignment.
        if let Some(parent) = cx.ancestors.last() {
            if parent.as_arguments_node().is_some() {
                let n = cx.ancestors.len();
                if n >= 2 && cx.ancestors[n - 2].as_call_node().is_some_and(|c| !is_csend(&c)) {
                    return;
                }
            }
            if helpers::node_types::is_assignment(parent) {
                return;
            }
        }
        let scope = match cx.ancestors.iter().rev().find(|a| matches!(a, Node::ClassNode { .. } | Node::ModuleNode { .. })) {
            None => "at the top level",
            Some(Node::ClassNode { .. }) => "in a class body",
            Some(_) => "in a module body",
        };
        let replacement = format!("alias {} {}", identifier(&args[0]), identifier(&args[1]));
        let (s, e) = send_range(&call);
        let sel = call.message_loc().unwrap();
        cx.add_fix(sel.start_offset(), sel.end_offset(), format!("Use `alias` instead of `alias_method` {scope}."), |c| {
            c.replace(s, e, &replacement)
        });
    }
}
