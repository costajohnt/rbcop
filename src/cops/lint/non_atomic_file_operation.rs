use crate::prelude::*;
use helpers::if_node::{parts, Else};
use ruby_prism::{CallNode, Visit};

pub const NAME: &str = "Lint/NonAtomicFileOperation";
pub static COP: NonAtomicFileOperation = NonAtomicFileOperation;
pub struct NonAtomicFileOperation;

const MAKE_FORCE: &[&str] = &["makedirs", "mkdir_p", "mkpath"];
const REMOVE: &[&str] = &["remove", "delete", "unlink", "remove_file", "rm", "rmdir", "safe_unlink"];
const RECURSIVE_REMOVE: &[&str] = &["remove_dir", "remove_entry", "remove_entry_secure"];
const REMOVE_FORCE: &[&str] = &["rm_f", "rm_rf"];

/// Finds `(pair (sym :force) (:true|:false))` and the first
/// `(send (const {cbase nil?} {:FileTest :File :Dir :Shell}) {:exist? :exists?} ...)`.
#[derive(Default)]
struct Search {
    any_force: [bool; 2],
    /// `Const.method` of the first existence check and its first argument range.
    exist: Option<(String, Option<(usize, usize)>)>,
}

impl<'pr> Visit<'pr> for Search {
    fn visit_assoc_node(&mut self, node: &ruby_prism::AssocNode<'pr>) {
        if node.key().as_symbol_node().is_some_and(|s| s.unescaped() == b"force") {
            let v = node.value();
            if v.as_true_node().is_some() {
                self.any_force[1] = true;
            } else if v.as_false_node().is_some() {
                self.any_force[0] = true;
            }
        }
        ruby_prism::visit_assoc_node(self, node);
    }
    fn visit_call_node(&mut self, node: &CallNode<'pr>) {
        if self.exist.is_none()
            && !is_csend(node)
            && !node.block().is_some_and(|b| b.as_block_node().is_some())
            && matches!(method_name(node), "exist?" | "exists?")
            && node.receiver().is_some_and(|r| ["FileTest", "File", "Dir", "Shell"].iter().any(|c| helpers::parser_parent::is_top_const(&r, c)))
        {
            let recv = node.receiver().and_then(|r| const_short(&r)).map(|c| c.0).unwrap_or_default();
            let first = send_args(node).first().map(|a| (a.location().start_offset(), a.location().end_offset()));
            self.exist = Some((format!("{recv}.{}", method_name(node)), first));
        }
        ruby_prism::visit_call_node(self, node);
    }
}

fn search(node: &Node) -> Search {
    let mut s = Search::default();
    s.visit(node);
    s
}

fn replacement_method(name: &str) -> &str {
    if name == "mkdir" {
        "mkdir_p"
    } else if REMOVE.contains(&name) {
        "rm_f"
    } else if RECURSIVE_REMOVE.contains(&name) {
        "rm_rf"
    } else {
        name
    }
}

/// `(const _ $_)`: the short name of a constant.
fn const_short(node: &Node) -> Option<(String, (usize, usize))> {
    if let Some(c) = node.as_constant_read_node() {
        let l = c.location();
        return Some((String::from_utf8_lossy(c.name().as_slice()).into(), (l.start_offset(), l.end_offset())));
    }
    let p = node.as_constant_path_node()?;
    let l = p.name_loc();
    Some((String::from_utf8_lossy(p.name()?.as_slice()).into(), (l.start_offset(), l.end_offset())))
}

impl Cop for NonAtomicFileOperation {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        if is_csend(&call)
            || !(name == "mkdir"
                || MAKE_FORCE.contains(&name)
                || REMOVE.contains(&name)
                || RECURSIVE_REMOVE.contains(&name)
                || REMOVE_FORCE.contains(&name))
        {
            return;
        }
        let Some(receiver) = call.receiver() else { return };
        let Some((recv_name, recv_name_range)) = const_short(&receiver) else { return };
        // if_node_child?
        let anc = cx.ancestors;
        let Some(pi) = helpers::parser_parent::parser_parent_index(anc, anc.len()) else { return };
        let parent = &anc[pi];
        if parent.as_if_node().is_none() && parent.as_unless_node().is_none() {
            return;
        }
        if is_pattern_guard(parent, &anc[..pi]) {
            return;
        }
        let (condition, _, els) = parts(parent);
        let has_else = match els {
            Some(Else::Elsif(_)) => true,
            Some(Else::Body(b)) => b.is_some(),
            None => false,
        };
        if helpers::parser_parent::is_operator_keyword(&condition) || has_else {
            return;
        }
        let own = search(node);
        if own.any_force[0] {
            return;
        }
        let Some((exist_name, exist_first)) = search(parent).exist else { return };
        let args = send_args(&call);
        let same_first = match (args.first(), exist_first) {
            (None, None) => true,
            (Some(a), Some(b)) => {
                helpers::node_equality::same(cx.src, (a.location().start_offset(), a.location().end_offset()), b)
            }
            _ => false,
        };
        if !same_first {
            return;
        }
        // register_offense
        let replacement = replacement_method(name).to_string();
        let force_option = args.iter().any(|a| search(a).any_force[1]);
        let force_method = MAKE_FORCE.contains(&name) || REMOVE_FORCE.contains(&name) || force_option;
        let (ns, ne) = send_range(&call);
        if !force_method {
            cx.add_with(ns, ne, format!("Use atomic file operation method `FileUtils.{replacement}`."), false);
        }
        let (keyword, end_kw) = match (parent.as_if_node(), parent.as_unless_node()) {
            (Some(i), _) => (i.if_keyword_loc(), i.end_keyword_loc()),
            (_, Some(u)) => (Some(u.keyword_loc()), u.end_keyword_loc()),
            _ => return,
        };
        let Some(keyword) = keyword else { return };
        let (rs, re) = (keyword.start_offset(), condition.location().end_offset());
        let msg = format!("Remove unnecessary existence check `{exist_name}`.");
        let elsif = keyword.as_slice() == b"elsif";
        let modifier = end_kw.is_none() && !elsif;
        let kw_start = keyword.start_offset();
        let end_range = end_kw.map(|e| (e.start_offset(), e.end_offset()));
        let sel = call.message_loc().map(|m| (m.start_offset(), m.end_offset()));
        let mode = recv_name == "Dir"
            && (receiver.as_constant_read_node().is_some() || receiver.as_constant_path_node().is_some_and(|p| p.parent().is_none()))
            && replacement == "mkdir_p"
            && args.len() == 2;
        let last_arg = args.last().map(|a| (a.location().start_offset(), a.location().end_offset()));
        cx.add_fix(rs, re, msg, |c| {
            if elsif {
                return;
            }
            c.remove(rs, re);
            if !force_method {
                c.replace(recv_name_range.0, recv_name_range.1, "FileUtils");
                if let Some((s, e)) = sel {
                    c.replace(s, e, &replacement);
                }
                if mode && let Some((s, e)) = last_arg {
                    c.insert_before(s, e, "mode: ");
                }
            }
            if modifier {
                c.remove(ne, kw_start);
            } else if let Some((s, e)) = end_range {
                c.remove(s, e);
            }
        });
    }
}
