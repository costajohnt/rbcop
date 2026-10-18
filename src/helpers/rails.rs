//! Small shared pieces of rubocop-rails: constant and hash-option patterns, ClassSendNodeHelper,
//! EnforceSuperclass, and the AST predicates its mixins (ActiveRecordHelper, MigrationsHelper,
//! RoutesHelper) define.

use crate::prelude::*;
use ruby_prism::CallNode;

/// `(const {nil? cbase} :A)`, `(const (const {nil? cbase} :A) :B)`, ...: the constant `path`
/// ("ActiveRecord::Base"), optionally written with a leading `::`.
pub fn is_const(node: &Node, path: &str) -> bool {
    helpers::const_name::const_name(node).is_some_and(|n| n == path)
}

/// `(const _ :Name)`: a constant whose last segment is `name`, under any namespace.
pub fn const_named(node: &Node, name: &str) -> bool {
    if let Some(c) = node.as_constant_read_node() {
        return c.name().as_slice() == name.as_bytes();
    }
    node.as_constant_path_node().is_some_and(|p| p.name().is_some_and(|n| n.as_slice() == name.as_bytes()))
}

/// A plain `send` (not `&.`): Prism CallNode without the safe-navigation operator.
pub fn send<'pr>(node: &Node<'pr>) -> Option<CallNode<'pr>> {
    node.as_call_node().filter(|c| !is_csend(c))
}

/// rubocop-ast `command?(name)`: receiver-less call to `name`.
pub fn is_command(call: &CallNode, name: &str) -> bool {
    call.receiver().is_none() && method_name(call) == name
}

/// The pairs of a parser `hash` node (a braced or bare keyword hash); None for anything else.
pub fn hash_elements<'pr>(node: &Node<'pr>) -> Option<Vec<Node<'pr>>> {
    if let Some(h) = node.as_hash_node() {
        return Some(h.elements().iter().collect());
    }
    node.as_keyword_hash_node().map(|h| h.elements().iter().collect())
}

/// `(pair (sym :key) $_)` among a hash's elements: the value of the first pair whose key is the
/// symbol `key`.
pub fn hash_value<'pr>(hash: &Node<'pr>, key: &str) -> Option<Node<'pr>> {
    hash_elements(hash)?.into_iter().find_map(|e| {
        let a = e.as_assoc_node()?;
        (sym_value(&a.key()).as_deref() == Some(key)).then(|| pair_value(&a))
    })
}

/// The pair node `(pair (sym :key) _)` in a hash.
pub fn hash_pair<'pr>(hash: &Node<'pr>, key: &str) -> Option<ruby_prism::AssocNode<'pr>> {
    hash_elements(hash)?.into_iter().find_map(|e| {
        let a = e.as_assoc_node()?;
        (sym_value(&a.key()).as_deref() == Some(key)).then_some(a)
    })
}

/// A plain `sym` node's value (`:a`, `a:`, `:"a"`); None for dsym and anything else.
pub fn sym_value(node: &Node) -> Option<String> {
    let s = node.as_symbol_node()?;
    Some(String::from_utf8_lossy(s.unescaped()).into_owned())
}

/// A plain `str` node's value; None for dstr (including multi-line literals) and anything else.
pub fn str_value(node: &Node) -> Option<String> {
    let s = node.as_string_node()?;
    if is_parser_dstr(node) {
        return None;
    }
    Some(String::from_utf8_lossy(s.unescaped()).into_owned())
}

/// `{str sym}` value.
pub fn str_or_sym(node: &Node) -> Option<String> {
    sym_value(node).or_else(|| str_value(node))
}

/// The last argument when it is a parser `hash`.
pub fn last_hash<'pr>(call: &CallNode<'pr>) -> Option<Node<'pr>> {
    send_args(call).pop().filter(|a| a.as_hash_node().is_some() || a.as_keyword_hash_node().is_some())
}

/// The statements of a class/module/block body as parser sees them (one node, or a `begin`'s
/// children).
pub fn body_statements<'pr>(body: Option<Node<'pr>>) -> Vec<Node<'pr>> {
    match body {
        None => Vec::new(),
        Some(b) => match b.as_statements_node() {
            Some(s) => s.body().iter().collect(),
            None => vec![b],
        },
    }
}

/// ClassSendNodeHelper#class_send_nodes: a body that is one send, or the sends among a multi-
/// statement body's children (a send with a `do`/`{}` block is parser's `block`, not a send).
// ponytail: a single non-send statement yields only its own send child when it is a block call
// (parser `each_child_node(:send)` of other node types is not modelled).
pub fn class_send_nodes<'pr>(body: Option<Node<'pr>>) -> Vec<CallNode<'pr>> {
    let stmts = body_statements(body);
    let plain = |n: &Node<'pr>| send(n).filter(|c| c.block().is_none_or(|b| b.as_block_node().is_none()));
    if stmts.len() == 1 {
        let n = &stmts[0];
        if let Some(c) = plain(n) {
            return vec![c];
        }
        // (block (send ...) ...): the send is a child of the block.
        if let Some(c) = send(n).filter(|c| c.block().is_some_and(|b| b.as_block_node().is_some())) {
            return vec![c];
        }
        return Vec::new();
    }
    stmts.iter().filter_map(plain).collect()
}

/// ActiveRecordHelper#active_record?: `ApplicationRecord` or `ActiveRecord::Base`.
pub fn is_active_record(node: &Node) -> bool {
    is_const(node, "ApplicationRecord") || is_const(node, "ActiveRecord::Base")
}

/// ActiveRecordHelper#inherit_active_record_base?: some enclosing `class` has an Active Record
/// superclass.
pub fn inherit_active_record_base(ancestors: &[Node]) -> bool {
    ancestors
        .iter()
        .filter_map(|a| a.as_class_node())
        .any(|c| c.superclass().is_some_and(|s| is_active_record(&s)))
}

/// MigrationsHelper#migration_class?: `class X < ActiveRecord::Migration[N.N]`.
pub fn is_migration_class(class: &ruby_prism::ClassNode) -> bool {
    let path = class.constant_path();
    // (const {nil? cbase} _)
    let simple = path.as_constant_read_node().is_some()
        || path.as_constant_path_node().is_some_and(|p| p.parent().is_none());
    if !simple {
        return false;
    }
    let Some(sup) = class.superclass() else { return false };
    let Some(call) = send(&sup) else { return false };
    if method_name(&call) != "[]" || call.block().is_some() {
        return false;
    }
    let args = call_args(&call);
    call.receiver().is_some_and(|r| is_const(&r, "ActiveRecord::Migration"))
        && args.len() == 1
        && args[0].as_float_node().is_some()
}

/// MigrationsHelper#in_migration?
pub fn in_migration(ancestors: &[Node]) -> bool {
    ancestors.iter().filter_map(|a| a.as_class_node()).any(|c| is_migration_class(&c))
}

/// RoutesHelper#within_routes?: inside the block of `<x>.routes.draw`.
pub fn within_routes(ancestors: &[Node]) -> bool {
    ancestors.windows(2).any(|w| {
        let (Some(call), Some(_)) = (send(&w[0]), w[1].as_block_node()) else { return false };
        let numbered = w[1].as_block_node().unwrap().parameters().is_some_and(|p| {
            p.as_numbered_parameters_node().is_some() || p.as_it_parameters_node().is_some()
        });
        !numbered
            && method_name(&call) == "draw"
            && call.arguments().is_none()
            && call.receiver().and_then(|r| send(&r)).is_some_and(|r| {
                method_name(&r) == "routes" && r.arguments().is_none() && r.block().is_none()
            })
    })
}

/// EnforceSuperclass: `class X < <base>` (X not named `superclass`) and `Class.new(<base>)` (not
/// assigned to the constant `superclass`), corrected to `superclass`. `base` is a constant path.
pub fn enforce_superclass(cx: &mut Cx, node: &Node, superclass: &str, base: &str, msg: &str) {
    if let Some(class) = node.as_class_node() {
        // (class (const _ !:SUPERCLASS) BASE ...)
        if const_named(&class.constant_path(), superclass) || !is_const_path(&class.constant_path()) {
            return;
        }
        if let Some(sup) = class.superclass().filter(|s| is_const(s, base)) {
            let (s, e) = (sup.location().start_offset(), sup.location().end_offset());
            cx.add_fix(s, e, msg, |c| c.replace(s, e, superclass));
        }
        return;
    }
    let Some(call) = send(node) else { return };
    // (send (const {nil? cbase} :Class) :new BASE)
    let args = call_args(&call);
    if method_name(&call) != "new"
        || !call.receiver().is_some_and(|r| is_const(&r, "Class"))
        || args.len() != 1
        || call.block().is_some_and(|b| b.as_block_argument_node().is_some())
        || !is_const(&args[0], base)
    {
        return;
    }
    // !^(casgn {nil? cbase} :SUPERCLASS ...) and !^^(casgn ... (block ...)): the parent, or the
    // grandparent through a block, assigns the constant `superclass`.
    let assigned = |n: &Node| match n {
        Node::ConstantWriteNode { .. } => n.as_constant_write_node().unwrap().name().as_slice() == superclass.as_bytes(),
        Node::ConstantPathWriteNode { .. } => {
            let t = n.as_constant_path_write_node().unwrap().target();
            t.parent().is_none() && t.name().is_some_and(|x| x.as_slice() == superclass.as_bytes())
        }
        _ => false,
    };
    // Visiting a call that owns a block, the parser parent found is the block's.
    let anc = cx.ancestors;
    if helpers::parser_parent::parser_parent_index(anc, anc.len()).is_some_and(|i| assigned(&anc[i])) {
        return;
    }
    let (s, e) = (args[0].location().start_offset(), args[0].location().end_offset());
    cx.add_fix(s, e, msg, |c| c.replace(s, e, superclass));
}

/// A `const` node (not an expression namespace such as `foo::Bar`).
fn is_const_path(node: &Node) -> bool {
    node.as_constant_read_node().is_some() || node.as_constant_path_node().is_some()
}

/// Parser range of a `sym` node: a label (`a:`, `"a":`) excludes its colon.
pub fn sym_range(node: &Node) -> (usize, usize) {
    let loc = node.location();
    let colon = node.as_symbol_node().and_then(|s| s.closing_loc()).is_some_and(|c| c.as_slice().ends_with(b":"));
    (loc.start_offset(), loc.end_offset() - usize::from(colon))
}

/// The Prism ancestors that are parser `send`/`csend` ancestors, innermost first: a CallNode
/// counts unless the next ancestor is its own block (then the parser ancestor is the block).
pub fn call_ancestors<'a, 'pr>(ancestors: &'a [Node<'pr>]) -> impl Iterator<Item = CallNode<'pr>> + 'a {
    (0..ancestors.len()).rev().filter_map(move |i| {
        let c = ancestors[i].as_call_node()?;
        let owns_block = ancestors.get(i + 1).is_some_and(|n| n.as_block_node().is_some());
        (!owns_block).then_some(c)
    })
}

/// ActiveRecordHelper#in_where?: the nearest enclosing call is `where`/`rewhere` (with `node`,
/// at `range`, not its receiver), or `where(...).not`.
pub fn in_where(ancestors: &[Node], range: (usize, usize)) -> bool {
    let Some(send) = call_ancestors(ancestors).next() else { return false };
    let name = method_name(&send);
    let recv = send.receiver();
    if matches!(name, "where" | "rewhere")
        && !recv.as_ref().is_some_and(|r| (r.location().start_offset(), r.location().end_offset()) == range)
    {
        return true;
    }
    let Some(r) = recv.as_ref().and_then(send_node) else { return false };
    name == "not" && matches!(method_name(&r), "where" | "rewhere")
}

/// `send_type?`: a call without `&.` and without a literal block (parser wraps that in a block).
pub fn send_node<'pr>(node: &Node<'pr>) -> Option<CallNode<'pr>> {
    send(node).filter(|c| c.block().is_none_or(|b| b.as_block_node().is_none()))
}

/// A parser `block` node's send: a call owning a literal block with ordinary (or no) parameters,
/// not numbered/`it` ones (those are numblock/itblock).
pub fn block_call<'pr>(node: &Node<'pr>) -> Option<CallNode<'pr>> {
    let call = node.as_call_node()?;
    let b = call.block()?.as_block_node()?;
    let numbered = b.parameters().is_some_and(|p| p.as_numbered_parameters_node().is_some() || p.as_it_parameters_node().is_some());
    (!numbered).then_some(call)
}

/// RangeHelp#range_with_surrounding_comma: widen over adjacent commas.
pub fn range_with_surrounding_comma(src: &str, start: usize, end: usize, left: bool, right: bool) -> (usize, usize) {
    let b = src.as_bytes();
    let mut s = start;
    while left && s > 0 && b[s - 1] == b',' {
        s -= 1;
    }
    let mut e = end;
    while right && e < b.len() && b[e] == b',' {
        e += 1;
    }
    (s, e)
}

/// `range_with_surrounding_comma(range_with_surrounding_space(range, side: :left), :left)`: the
/// range to remove for a hash pair or argument.
pub fn left_space_and_comma(src: &str, start: usize, end: usize) -> (usize, usize) {
    use helpers::range_help::{Side, Surrounding, range_with_surrounding_space};
    let (s, e) = range_with_surrounding_space(src, start, end, Surrounding { side: Side::Left, ..Default::default() });
    range_with_surrounding_comma(src, s, e, true, false)
}

/// The parser parent of the visited node, as an index into `ancestors`.
pub fn parent_index(ancestors: &[Node]) -> Option<usize> {
    helpers::parser_parent::parser_parent_index(ancestors, ancestors.len())
}

/// Ruby's String#capitalize: first character upcased, the rest downcased.
pub fn capitalize(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(f) => f.to_uppercase().chain(chars.flat_map(char::to_lowercase)).collect(),
        None => String::new(),
    }
}

/// A hash's `pair` children (kwsplats left out); empty for a non-hash.
pub fn hash_pairs<'pr>(node: &Node<'pr>) -> Vec<ruby_prism::AssocNode<'pr>> {
    hash_elements(node).unwrap_or_default().iter().filter_map(|e| e.as_assoc_node()).collect()
}

/// `(const {nil? cbase} :Class)` / `:Module` (rubocop-ast `global_const?`).
fn global_const(n: &Node, names: &[&str]) -> bool {
    names.iter().any(|x| helpers::parser_parent::is_top_const(n, x))
}

fn numbered_block(b: &ruby_prism::BlockNode) -> bool {
    b.parameters().is_some_and(|p| p.as_numbered_parameters_node().is_some() || p.as_it_parameters_node().is_some())
}

/// rubocop-ast `defined_module_name` of a class, module or casgn (`X = Class.new ...`) node.
pub fn defined_module_name(n: &Node) -> Option<String> {
    let path = match n {
        Node::ClassNode { .. } => n.as_class_node().unwrap().constant_path(),
        Node::ModuleNode { .. } => n.as_module_node().unwrap().constant_path(),
        Node::ConstantWriteNode { .. } | Node::ConstantPathWriteNode { .. } => {
            let value = match n.as_constant_write_node() {
                Some(w) => w.value(),
                None => n.as_constant_path_write_node().unwrap().value(),
            };
            // (send #global_const?({:Class :Module}) :new ...), or that send with a block.
            let ok = value.as_call_node().is_some_and(|c| {
                !is_csend(&c)
                    && method_name(&c) == "new"
                    && c.receiver().is_some_and(|r| global_const(&r, &["Class", "Module"]))
                    && c.block().is_none_or(|b| b.as_block_node().is_none_or(|b| !numbered_block(&b)))
            });
            if !ok {
                return None;
            }
            match n.as_constant_write_node() {
                Some(w) => return Some(String::from_utf8_lossy(w.name().as_slice()).into_owned()),
                None => n.as_constant_path_write_node().unwrap().target().as_node(),
            }
        }
        _ => return None,
    };
    if let Some(c) = path.as_constant_read_node() {
        return Some(String::from_utf8_lossy(c.name().as_slice()).into_owned());
    }
    let p = path.as_constant_path_node()?;
    let name = String::from_utf8_lossy(p.name()?.as_slice()).into_owned();
    Some(match p.parent() {
        None => name,
        Some(ns) => format!("{}::{name}", helpers::const_name::const_name(&ns).unwrap_or_default()),
    })
}

/// parent_module_name_for_block: Ok(part) to go on, Err to make the whole name nil. `call` is the
/// block's send, `parent` the block's parser parent.
fn block_part(call: &CallNode, parent: Option<&Node>) -> Result<Option<String>, ()> {
    if method_name(call) == "class_eval" {
        return match call.receiver() {
            None => Ok(None),
            Some(r) if r.as_constant_read_node().is_some() || r.as_constant_path_node().is_some() => {
                Ok(Some(helpers::const_name::const_name(&r).unwrap_or_default()))
            }
            Some(_) => Err(()),
        };
    }
    // new_class_or_module_block?: ^(casgn _ _ (block (send (const _ {:Class :Module}) :new) ...))
    let casgn = parent.is_some_and(|p| p.as_constant_write_node().is_some() || p.as_constant_path_write_node().is_some());
    let new_block = !is_csend(call)
        && method_name(call) == "new"
        && call.arguments().is_none()
        && call.receiver().is_some_and(|r| const_named(&r, "Class") || const_named(&r, "Module"));
    if casgn && new_block { Ok(None) } else { Err(()) }
}

/// rubocop-ast `parent_module_name` of the visited node (its Prism `ancestors`); `own`: the node
/// is this call and it owns a literal block, which is then its parser parent.
pub fn parent_module_name(ancestors: &[Node], own: Option<&CallNode>) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    if let Some(c) = own.filter(|c| c.block().and_then(|b| b.as_block_node()).is_some_and(|b| !numbered_block(&b))) {
        let parent = helpers::parser_parent::parser_parent_index(ancestors, ancestors.len()).map(|k| &ancestors[k]);
        parts.extend(block_part(c, parent).ok()?);
    }
    for i in (0..ancestors.len()).rev() {
        let a = &ancestors[i];
        match a {
            Node::ClassNode { .. } | Node::ModuleNode { .. } | Node::ConstantWriteNode { .. } | Node::ConstantPathWriteNode { .. } => {
                parts.extend(defined_module_name(a));
            }
            Node::SingletonClassNode { .. } => {
                let subject = a.as_singleton_class_node().unwrap().expression();
                if subject.as_constant_read_node().is_some() || subject.as_constant_path_node().is_some() {
                    parts.push(format!("#<Class:{}>", helpers::const_name::const_name(&subject).unwrap_or_default()));
                } else if subject.as_self_node().is_some() {
                    parts.push(format!("#<Class:{}>", parent_module_name(&ancestors[..i], None).unwrap_or_default()));
                } else {
                    return None;
                }
            }
            Node::BlockNode { .. } => {
                if numbered_block(&a.as_block_node().unwrap()) {
                    continue;
                }
                let call = i.checked_sub(1).and_then(|k| ancestors[k].as_call_node())?;
                let parent = helpers::parser_parent::parser_parent_index(ancestors, i).map(|k| &ancestors[k]);
                parts.extend(block_part(&call, parent).ok()?);
            }
            // A lambda literal is a parser `block` on `lambda`: never class_eval or Class.new.
            Node::LambdaNode { .. } => return None,
            Node::CallNode { .. } => {
                // Reached through the receiver or arguments of a call with a block: parser's
                // block is an ancestor too.
                let c = a.as_call_node().unwrap();
                let Some(b) = c.block().and_then(|b| b.as_block_node()) else { continue };
                if ancestors.get(i + 1).is_some_and(|x| x.as_block_node().is_some()) || numbered_block(&b) {
                    continue;
                }
                let parent = helpers::parser_parent::parser_parent_index(ancestors, i).map(|k| &ancestors[k]);
                parts.extend(block_part(&c, parent).ok()?);
            }
            _ => {}
        }
    }
    parts.reverse();
    let r = parts.join("::");
    Some(if r.is_empty() { "Object".to_string() } else { r })
}

/// A pair's value as parser sees it: an omitted value (`key:`) is the lvar or send it stands for.
pub fn pair_value<'pr>(p: &ruby_prism::AssocNode<'pr>) -> Node<'pr> {
    let v = p.value();
    match v.as_implicit_node() {
        Some(i) => i.value(),
        None => v,
    }
}

/// A key for rubocop-ast structural node equality: parser's s-expression of the node, or the
/// token shape where the s-expression is not available.
pub fn node_key(src: &crate::source::Source, node: &Node) -> String {
    helpers::parser_sexp::sexp(node, src).unwrap_or_else(|| {
        helpers::node_equality::shape(src, node.location().start_offset(), node.location().end_offset()).join("\u{1}")
    })
}

/// Ruby's `String#split(sep)` for a one-character separator: trailing empty fields dropped.
pub fn ruby_split(s: &str, sep: char) -> Vec<&str> {
    let mut parts: Vec<&str> = s.split(sep).collect();
    while parts.last().is_some_and(|p| p.is_empty()) {
        parts.pop();
    }
    parts
}

/// Text from a string or symbol that a correction may splice into a quoted literal: no quotes,
/// backslashes, interpolation or line breaks (rbcop withholds the correction otherwise, a
/// deliberate difference from RuboCop, docs/known-gaps.md).
pub fn inert_text(s: &str) -> bool {
    !s.contains(['\'', '"', '\\', '\n', '\r', '\0']) && !["#{", "#@", "#$"].iter().any(|i| s.contains(i))
}

/// `inert_text` for a known target literal: `quote` is `'` or `"`; the other quote is allowed.
pub fn inert_in(s: &str, quote: char) -> bool {
    let other = if quote == '"' { '\'' } else { '"' };
    inert_text(&s.replace(other, ""))
}

/// Source of a pair's value as parser sees it: an omitted value (`key:`) is just `key`.
pub fn pair_value_source<'pr>(p: &ruby_prism::AssocNode<'pr>) -> &'pr str {
    let v = p.value();
    let t = node_text(&v);
    if v.as_implicit_node().is_some() { t.strip_suffix(':').unwrap_or(t) } else { t }
}

/// The nearest parser `any_block` ancestor of the visited node, as the call that owns it (None for
/// a lambda literal, whose send has no receiver). `Some(None)` when the nearest block is a lambda.
/// A call with a block counts when the node is inside its block, receiver or arguments.
pub fn nearest_block_owner<'pr>(ancestors: &[Node<'pr>]) -> Option<Option<CallNode<'pr>>> {
    for i in (0..ancestors.len()).rev() {
        match &ancestors[i] {
            Node::BlockNode { .. } => return Some(i.checked_sub(1).and_then(|k| ancestors[k].as_call_node())),
            Node::LambdaNode { .. } => return Some(None),
            Node::CallNode { .. } => {
                let c = ancestors[i].as_call_node().unwrap();
                if c.block().is_some_and(|b| b.as_block_node().is_some())
                    && !ancestors.get(i + 1).is_some_and(|n| n.as_block_node().is_some())
                {
                    return Some(Some(c));
                }
            }
            _ => {}
        }
    }
    None
}

/// Parser children of an if/unless: (condition, true branch, false branch), bodies as single nodes
/// (a multi-statement body is the StatementsNode, parser's `begin`); `unless` swaps the branches.
pub fn if_children<'pr>(node: &Node<'pr>) -> Option<(Node<'pr>, Option<Node<'pr>>, Option<Node<'pr>>)> {
    let body = |s: Option<ruby_prism::StatementsNode<'pr>>| -> Option<Node<'pr>> {
        let s = s?;
        let mut v: Vec<Node<'pr>> = s.body().iter().collect();
        if v.len() == 1 { v.pop() } else { Some(s.as_node()) }
    };
    let els = |e: Option<Node<'pr>>| -> Option<Node<'pr>> {
        let e = e?;
        match e.as_else_node() {
            Some(el) => body(el.statements()),
            None => Some(e),
        }
    };
    if let Some(n) = node.as_if_node() {
        return Some((n.predicate(), body(n.statements()), els(n.subsequent())));
    }
    let n = node.as_unless_node()?;
    Some((n.predicate(), els(n.else_clause().map(|e| e.as_node())), body(n.statements())))
}

/// Rack::Utils::SYMBOL_TO_STATUS_CODE (rack 3.2.7, as installed with rubocop-rails 2.38.0).
pub const SYMBOL_TO_STATUS_CODE: &[(&str, i64)] = &[
    ("continue", 100),
    ("switching_protocols", 101),
    ("processing", 102),
    ("early_hints", 103),
    ("ok", 200),
    ("created", 201),
    ("accepted", 202),
    ("non_authoritative_information", 203),
    ("no_content", 204),
    ("reset_content", 205),
    ("partial_content", 206),
    ("multi_status", 207),
    ("already_reported", 208),
    ("im_used", 226),
    ("multiple_choices", 300),
    ("moved_permanently", 301),
    ("found", 302),
    ("see_other", 303),
    ("not_modified", 304),
    ("use_proxy", 305),
    ("temporary_redirect", 307),
    ("permanent_redirect", 308),
    ("bad_request", 400),
    ("unauthorized", 401),
    ("payment_required", 402),
    ("forbidden", 403),
    ("not_found", 404),
    ("method_not_allowed", 405),
    ("not_acceptable", 406),
    ("proxy_authentication_required", 407),
    ("request_timeout", 408),
    ("conflict", 409),
    ("gone", 410),
    ("length_required", 411),
    ("precondition_failed", 412),
    ("content_too_large", 413),
    ("uri_too_long", 414),
    ("unsupported_media_type", 415),
    ("range_not_satisfiable", 416),
    ("expectation_failed", 417),
    ("misdirected_request", 421),
    ("unprocessable_content", 422),
    ("locked", 423),
    ("failed_dependency", 424),
    ("too_early", 425),
    ("upgrade_required", 426),
    ("precondition_required", 428),
    ("too_many_requests", 429),
    ("request_header_fields_too_large", 431),
    ("unavailable_for_legal_reasons", 451),
    ("internal_server_error", 500),
    ("not_implemented", 501),
    ("bad_gateway", 502),
    ("service_unavailable", 503),
    ("gateway_timeout", 504),
    ("http_version_not_supported", 505),
    ("variant_also_negotiates", 506),
    ("insufficient_storage", 507),
    ("loop_detected", 508),
    ("network_authentication_required", 511),
];

/// The parser parent of the visited node when it is a plain `send` that has the node as its
/// receiver (Date/TimeZone `method_send?`).
pub fn receiver_send_parent<'pr>(ancestors: &[Node<'pr>], node: &Node) -> Option<(usize, CallNode<'pr>)> {
    let i = parent_index(ancestors)?;
    let c = send(&ancestors[i])?;
    let r = c.receiver()?;
    (r.location().start_offset() == node.location().start_offset() && r.location().end_offset() == node.location().end_offset())
        .then_some((i, c))
}

/// Method names of a send and of every `send` ancestor above it (`[node, *node.each_ancestor(:send)]`),
/// the send being `ancestors[i]`.
pub fn send_chain_names(ancestors: &[Node], i: usize, first: &CallNode) -> Vec<String> {
    let mut out = vec![method_name(first).to_string()];
    out.extend(call_ancestors(&ancestors[..i]).filter(|c| !is_csend(c)).map(|c| method_name(&c).to_string()));
    out
}

/// DatabaseTypeResolvable#database: the cop's `Database`, else the adapter of `development` in
/// ./config/database.yml (mysql2/trilogy: mysql, postgresql/postgis: postgresql; read with
/// `YAML.unsafe_load_file`), else DATABASE_URL's scheme.
pub fn database(cx: &Cx) -> Option<String> {
    if let Some(d) = cx.cfg.str("Database") {
        return Some(d.to_string());
    }
    let from_yaml = || -> Option<Option<&'static str>> {
        let text = std::fs::read_to_string(crate::config::cwd().join("config/database.yml")).ok()?;
        let Ok(Value::Map(yaml)) = crate::config::value::unsafe_load(&text) else { return None };
        let dev = yaml.map("development")?;
        let adapter = match dev.get("adapter") {
            Some(a) if !matches!(a, Value::Null | Value::Bool(false)) => a.as_string(),
            _ => yaml.map("shared").and_then(|s| s.get("adapter")).and_then(|a| a.as_string()).or_else(|| {
                // `database_yaml.first.last['adapter']`
                let (_, first) = dev.0.first()?;
                first.as_map()?.get("adapter")?.as_string()
            }),
        };
        Some(match adapter.as_deref() {
            Some("mysql2" | "trilogy") => Some("mysql"),
            Some("postgresql" | "postgis") => Some("postgresql"),
            _ => None,
        })
    };
    if let Some(Some(d)) = from_yaml() {
        return Some(d.to_string());
    }
    let url = std::env::var("DATABASE_URL").unwrap_or_default();
    if url.starts_with("mysql2://") || url.starts_with("trilogy://") {
        Some("mysql".to_string())
    } else if url.starts_with("postgres://") || url.starts_with("postgresql://") {
        Some("postgresql".to_string())
    } else {
        None
    }
}

/// rubocop-ast `value` of a node that has one (sym, str, dstr, numbers), as a string.
pub fn node_value(n: &Node) -> Option<String> {
    if let Some(v) = sym_value(n).or_else(|| str_value(n)) {
        return Some(v);
    }
    if let Some(d) = n.as_interpolated_string_node() {
        // DstrNode#value: child values, or source for children without one.
        return Some(d.parts().iter().map(|p| node_value(&p).unwrap_or_else(|| node_text(&p).to_string())).collect());
    }
    // ponytail: Integer#to_s / Float#to_s taken as the source (differs for `1_000`, `0x10`).
    (n.as_integer_node().is_some() || n.as_float_node().is_some()).then(|| node_text(n).to_string())
}
