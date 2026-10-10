use crate::prelude::*;
use ruby_prism::CallNode;

pub const NAME: &str = "Style/AccessModifierDeclarations";
pub static COP: AccessModifierDeclarations = AccessModifierDeclarations;
pub struct AccessModifierDeclarations;

const MODIFIERS: &[&str] = &["private", "protected", "public", "module_function"];

/// The parser-gem parent of the access-modifier send.
enum Parent<'pr> {
    /// A statement list: parser `begin` (or `kwbegin` when `begin_type` is false).
    Stmts { stmts: Vec<Node<'pr>>, idx: usize, begin_type: bool, header: Bound },
    /// The only statement of a class/def/... body (no siblings that matter).
    Single { header: Bound },
    Root,
    Block,
    Pair,
    If,
    Other,
}

/// Comments leading a statement start at or after `.0` and on a line below `.1`.
type Bound = (usize, Option<usize>);

/// The bound for statement `i` of a list whose first statement gets `header`.
fn bound(src: &crate::source::Source, stmts: &[Node], i: usize, header: Bound) -> Bound {
    if i == 0 {
        return header;
    }
    let prev_end = stmts[i - 1].location().end_offset();
    (prev_end, Some(src.line_of(prev_end)))
}

fn span(n: &Node) -> (usize, usize) {
    (n.location().start_offset(), n.location().end_offset())
}

/// Where comments leading the first statement of a body may start: after the header line.
fn header_bound(src: &crate::source::Source, owner: &Node) -> (usize, Option<usize>) {
    if let Some(d) = owner.as_def_node()
        && d.parameters().is_none() && d.lparen_loc().is_none() {
            // `def foo # c`: parser's empty args have no location; the comment leads the body.
            return (d.name_loc().end_offset(), None);
        }
    (0, Some(src.line_of(owner.location().start_offset())))
}

fn parser_parent<'pr>(cx: &Cx<'_, 'pr>, call: &CallNode<'pr>) -> Parent<'pr> {
    if call.block().is_some_and(|b| b.as_block_node().is_some()) {
        return Parent::Block;
    }
    let anc = cx.ancestors;
    let Some(p) = anc.last() else { return Parent::Root };
    let me = span(&call.as_node());
    match p {
        Node::StatementsNode { .. } => {
            let stmts: Vec<Node> = p.as_statements_node().unwrap().body().iter().collect();
            let idx = stmts.iter().position(|s| span(s) == me).unwrap_or(0);
            let owner = anc.len().checked_sub(2).map(|i| &anc[i]);
            let header = match owner {
                Some(o) if o.as_program_node().is_none() => header_bound(cx.src, o),
                _ => (0, None),
            };
            if stmts.len() >= 2 || owner.is_some_and(|o| o.as_parentheses_node().is_some()) {
                let kwbegin = owner.and_then(|o| o.as_begin_node()).is_some_and(|b| {
                    b.begin_keyword_loc().is_some() && b.rescue_clause().is_none() && b.ensure_clause().is_none()
                });
                return Parent::Stmts { stmts, idx, begin_type: !kwbegin, header };
            }
            match owner {
                None | Some(Node::ProgramNode { .. }) => Parent::Root,
                Some(Node::BlockNode { .. } | Node::LambdaNode { .. }) => Parent::Block,
                Some(Node::IfNode { .. } | Node::UnlessNode { .. }) => Parent::If,
                Some(Node::ElseNode { .. }) => {
                    let gp = anc.len().checked_sub(3).map(|i| &anc[i]);
                    if gp.is_some_and(|g| matches!(g, Node::IfNode { .. } | Node::UnlessNode { .. })) {
                        Parent::If
                    } else {
                        Parent::Single { header }
                    }
                }
                _ => Parent::Single { header },
            }
        }
        Node::AssocNode { .. } => Parent::Pair,
        Node::IfNode { .. } | Node::UnlessNode { .. } => Parent::If,
        _ => Parent::Other,
    }
}

/// (send nil? MODIFIER {(sym _)+ (splat {#percent_symbol_array? const send})})
fn with_symbol(call: &CallNode) -> bool {
    let args = send_args(call);
    if args.is_empty() {
        return false;
    }
    if args.iter().all(|a| a.as_symbol_node().is_some()) {
        return true;
    }
    let [a] = args.as_slice() else { return false };
    let Some(splat) = a.as_splat_node() else { return false };
    splat.expression().is_some_and(|e| match &e {
        Node::ArrayNode { .. } => e
            .as_array_node()
            .unwrap()
            .opening_loc()
            .is_some_and(|o| o.as_slice().starts_with(b"%i") || o.as_slice().starts_with(b"%I")),
        Node::ConstantReadNode { .. } | Node::ConstantPathNode { .. } => true,
        Node::CallNode { .. } => {
            let c = e.as_call_node().unwrap();
            !is_csend(&c) && c.block().is_none_or(|b| b.as_block_node().is_none())
        }
        _ => false,
    })
}

/// (send nil? MODIFIER (send nil? NAMES ...)) with the inner send's argument count checked by `argc`.
fn with_inner_send(call: &CallNode, names: &[&str], argc: fn(usize) -> bool) -> bool {
    let args = send_args(call);
    let [a] = args.as_slice() else { return false };
    a.as_call_node().is_some_and(|c| {
        !is_csend(&c)
            && c.receiver().is_none()
            && c.block().is_none_or(|b| b.as_block_node().is_none())
            && names.contains(&method_name(&c))
            && argc(send_args(&c).len())
    })
}

fn modifier_call<'pr>(n: &Node<'pr>) -> Option<CallNode<'pr>> {
    n.as_call_node().filter(|c| {
        !is_csend(c) && c.block().is_none_or(|b| b.as_block_node().is_none()) && MODIFIERS.contains(&method_name(c))
    })
}

/// `bare_access_modifier_declaration?`
fn bare(n: &Node) -> bool {
    modifier_call(n).is_some_and(|c| c.receiver().is_none() && send_args(&c).is_empty())
}

struct Ctx<'a, 'pr> {
    cfg_symbols: bool,
    cfg_attrs: bool,
    cfg_alias: bool,
    group: bool,
    ancestors: &'a [Node<'pr>],
}

impl Ctx<'_, '_> {
    /// `allowed?` for a send whose parser parent is `parent`.
    fn allowed(&self, call: &CallNode, parent: &Parent, ancestors: &[Node]) -> bool {
        let access_modifier = helpers::macro_scope::is_macro(call, ancestors);
        !access_modifier
            || matches!(parent, Parent::Pair | Parent::Block)
            || (self.cfg_symbols && with_symbol(call))
            || (self.cfg_attrs && with_inner_send(call, &["attr", "attr_reader", "attr_writer", "attr_accessor"], |n| n > 0))
            || (self.cfg_alias && with_inner_send(call, &["alias_method"], |n| n == 2))
    }
}

/// `find_corresponding_def_nodes`: ranges of the defs (empty when there are none).
fn corresponding_defs<'pr>(call: &CallNode<'pr>, parent: &Parent<'pr>) -> Vec<Node<'pr>> {
    if with_symbol(call) {
        let names: Vec<Vec<u8>> =
            send_args(call).iter().filter_map(|a| a.as_symbol_node().map(|s| s.unescaped().to_vec())).collect();
        let defs: Vec<Node> = match parent {
            Parent::Stmts { stmts, .. } => stmts
                .iter()
                .filter(|s| s.as_def_node().is_some_and(|d| d.receiver().is_none() && names.iter().any(|n| n == d.name().as_slice())))
                .map(|s| s.as_def_node().unwrap().as_node())
                .collect(),
            _ => Vec::new(),
        };
        if defs.len() == names.len() { defs } else { Vec::new() }
    } else {
        send_args(call).into_iter().take(1).collect()
    }
}

impl Cop for AccessModifierDeclarations {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = node.as_call_node().filter(|c| !is_csend(c) && MODIFIERS.contains(&method_name(c))) else {
            return;
        };
        let ctx = Ctx {
            cfg_symbols: cx.cfg.bool("AllowModifiersOnSymbols").unwrap_or(false),
            cfg_attrs: cx.cfg.bool("AllowModifiersOnAttrs").unwrap_or(false),
            cfg_alias: cx.cfg.bool("AllowModifiersOnAliasMethod").unwrap_or(false),
            group: cx.cfg.str("EnforcedStyle") != Some("inline"),
            ancestors: cx.ancestors,
        };
        let parent = parser_parent(cx, &call);
        if ctx.allowed(&call, &parent, ctx.ancestors) {
            return;
        }
        let name = method_name(&call);
        let inlined = !send_args(&call).is_empty();
        let siblings: &[Node] = match &parent {
            Parent::Stmts { stmts, idx, .. } => &stmts[idx + 1..],
            _ => &[],
        };
        let offense = if ctx.group {
            let blocked = match parent {
                Parent::Root => with_symbol(&call),
                Parent::If => true,
                _ => false,
            };
            !blocked
                && inlined
                && !siblings.iter().any(|s| {
                    s.as_call_node().is_some_and(|sc| {
                        // sibling.send_type? && correctable_group_offense?(sibling) && same method && args
                        !is_csend(&sc)
                            && sc.block().is_none_or(|b| b.as_block_node().is_none())
                            && MODIFIERS.contains(&method_name(&sc))
                            && !ctx.allowed(&sc, &parent, ctx.ancestors)
                            && !send_args(&sc).is_empty()
                            && !corresponding_defs(&sc, &parent).is_empty()
                            && method_name(&sc) == name
                    })
                })
        } else {
            !inlined && grouped_defs(siblings).next().is_some()
        };
        if !offense {
            return;
        }
        let sel = call.message_loc().unwrap();
        let msg = if ctx.group {
            format!("`{name}` should not be inlined in method definitions.")
        } else {
            format!("`{name}` should be inlined in method definitions.")
        };
        let src = cx.src;
        let me = span(node);
        let (after, after_line) = match &parent {
            Parent::Stmts { stmts, idx, header, .. } => bound(src, stmts, *idx, *header),
            Parent::Single { header } => *header,
            _ => (0, None),
        };
        let my_comments = helpers::comment_association::leading(src, me.0, after, after_line);
        let lines_with_comments = |start: usize, end: usize, first_comment: Option<usize>| {
            let s = first_comment.map_or(start, |c| c.min(start));
            helpers::whole_lines::range_by_whole_lines(src.text, s, end, true)
        };
        let mut edits: Vec<Edit> = Vec::new();
        if ctx.group {
            let defs = corresponding_defs(&call, &parent);
            if !defs.is_empty() {
                let comment_texts = my_comments.iter().map(|c| src.slice(c.start, c.end).to_string());
                let source = comment_texts.chain(defs.iter().map(|d| node_text(d).to_string())).collect::<Vec<_>>().join("\n");
                let argless = match &parent {
                    Parent::Stmts { stmts, .. } => stmts.iter().find(|s| {
                        s.as_call_node().is_some_and(|c| {
                            !is_csend(&c)
                                && c.block().is_none_or(|b| b.as_block_node().is_none())
                                && method_name(&c) == name
                                && send_args(&c).is_empty()
                        })
                    }),
                    _ => None,
                };
                let class_end = cx.ancestors.iter().rev().find_map(|a| match a {
                    Node::ClassNode { .. } => Some(a.as_class_node().unwrap().end_keyword_loc()),
                    Node::ModuleNode { .. } => Some(a.as_module_node().unwrap().end_keyword_loc()),
                    Node::SingletonClassNode { .. } => Some(a.as_singleton_class_node().unwrap().end_keyword_loc()),
                    _ => None,
                });
                let mut remove = true;
                if let Some(a) = argless {
                    let (s, e) = span(a);
                    edits.push(Edit::InsertAfter(s, e, format!("\n\n{source}")));
                } else if let Some(end) = class_end {
                    edits.push(Edit::InsertBefore(end.start_offset(), end.end_offset(), format!("{name}\n\n{source}\n")));
                } else {
                    edits.push(Edit::Replace(me.0, me.1, format!("{name}\n\n{source}")));
                    remove = false;
                }
                if remove {
                    for d in &defs {
                        let (s, e) = span(d);
                        let first = def_leading(src, &parent, d);
                        let (rs, re) = lines_with_comments(s, e, first);
                        edits.push(Edit::Remove(rs, re));
                    }
                    let (rs, re) = lines_with_comments(me.0, me.1, my_comments.first().map(|c| c.start));
                    edits.push(Edit::Remove(rs, re));
                }
            }
        } else {
            match &parent {
                Parent::Stmts { stmts, idx, begin_type: true, .. } => {
                    // remove_modifier_node_within_begin: up to the next sibling's first preceding comment.
                    let next = &stmts[idx + 1];
                    let next_line = src.line_of(next.location().start_offset());
                    let first = helpers::comment_association::leading(src, next.location().start_offset(), me.1, Some(src.line_of(me.1)))
                        .into_iter()
                        .find(|c| src.line_of(c.start) < next_line)
                        .map_or(next.location().start_offset(), |c| c.start);
                    edits.push(Edit::Remove(me.0, first));
                }
                _ => {
                    let (rs, re) = lines_with_comments(me.0, me.1, my_comments.first().map(|c| c.start));
                    edits.push(Edit::Remove(rs, re));
                }
            }
            for d in grouped_defs(siblings) {
                let (s, e) = span(&d);
                edits.push(Edit::InsertBefore(s, e, format!("{name} ")));
            }
        }
        cx.add_fix(sel.start_offset(), sel.end_offset(), msg, |c| {
            for e in &edits {
                match e {
                    Edit::Remove(s, t) => c.remove(*s, *t),
                    Edit::Replace(s, t, x) => c.replace(*s, *t, x),
                    Edit::InsertBefore(s, t, x) => c.insert_before(*s, *t, x),
                    Edit::InsertAfter(s, t, x) => c.insert_after(*s, *t, x),
                }
            }
        });
    }
}

enum Edit {
    Remove(usize, usize),
    Replace(usize, usize, String),
    InsertBefore(usize, usize, String),
    InsertAfter(usize, usize, String),
}

/// `select_grouped_def_nodes`: defs among the right siblings up to the next bare modifier.
fn grouped_defs<'a, 'pr>(siblings: &'a [Node<'pr>]) -> impl Iterator<Item = Node<'pr>> + 'a {
    siblings.iter().take_while(|s| !bare(s)).filter_map(|s| s.as_def_node().filter(|d| d.receiver().is_none()).map(|d| d.as_node()))
}

/// Start of the first comment associated with a def statement, if any.
fn def_leading(src: &crate::source::Source, parent: &Parent, def: &Node) -> Option<usize> {
    let Parent::Stmts { stmts, header, .. } = parent else { return None };
    let start = def.location().start_offset();
    let i = stmts.iter().position(|s| s.location().start_offset() == start)?;
    let (a, al) = bound(src, stmts, i, *header);
    helpers::comment_association::leading(src, start, a, al).first().map(|c| c.start)
}
