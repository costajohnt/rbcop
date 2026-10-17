use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Rails/FilePath";
pub static COP: FilePath = FilePath;
pub struct FilePath;

/// rails_root_nodes?: a `Rails.root` send anywhere in the node.
fn has_rails_root(n: &Node) -> bool {
    struct F(bool);
    impl<'pr> Visit<'pr> for F {
        fn visit_call_node(&mut self, c: &ruby_prism::CallNode<'pr>) {
            if !is_csend(c)
                && method_name(c) == "root"
                && c.arguments().is_none()
                && c.block().is_none()
                && c.receiver().is_some_and(|r| helpers::rails::is_const(&r, "Rails"))
            {
                self.0 = true;
            }
            ruby_prism::visit_call_node(self, c);
        }
    }
    let mut f = F(false);
    f.visit(n);
    f.0
}

/// A plain parser `str` and its value.
fn str_val(n: &Node) -> Option<String> {
    helpers::rails::str_value(n)
}

fn message(cx: &Cx, to_s: bool) -> String {
    let to_s = if to_s { ".to_s" } else { "" };
    if cx.cfg.str("EnforcedStyle") == Some("arguments") {
        format!("Prefer `Rails.root.join('path', 'to'){to_s}`.")
    } else {
        format!("Prefer `Rails.root.join('path/to'){to_s}`.")
    }
}

/// register_offense's range: on the node's first line, from its column to its last column.
fn offense_range(src: &crate::source::Source, s: usize, e: usize) -> (usize, usize) {
    let col = src.column(s);
    let last_col = src.column(e);
    let line_start = src.line_start(src.line_of(s));
    let rest = &src.text[line_start..];
    let byte_at = |chars: usize| rest.char_indices().nth(chars).map_or(rest.len(), |(i, _)| i) + line_start;
    let a = byte_at(col);
    let b = byte_at(last_col);
    if b < a { (a, a) } else { (a, b) }
}

impl Cop for FilePath {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE, kind::INTERPOLATED_STRING_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if let Some(d) = node.as_interpolated_string_node() {
            return on_dstr(cx, node, &d);
        }
        let Some(call) = helpers::rails::send(node) else { return };
        if method_name(&call) != "join" {
            return;
        }
        let args = send_args(&call);
        let (s, e) = send_range(&call);
        let src = cx.src;
        let r = |n: &Node| (n.location().start_offset(), n.location().end_offset());
        // check_for_file_join_with_rails_root
        if call.receiver().is_some_and(|rc| helpers::rails::is_const(&rc, "File"))
            && args.iter().any(has_rails_root)
            && !args.iter().any(|a| {
                matches!(
                    a,
                    Node::LocalVariableReadNode { .. }
                        | Node::InstanceVariableReadNode { .. }
                        | Node::ClassVariableReadNode { .. }
                        | Node::GlobalVariableReadNode { .. }
                        | Node::ConstantReadNode { .. }
                        | Node::ConstantPathNode { .. }
                ) || str_val(a).is_some_and(|v| v.contains("//"))
            })
        {
            let (os, oe) = offense_range(src, s, e);
            let first_array = args[0].as_array_node().is_some();
            let recv = r(&call.receiver().unwrap());
            // remove_first_argument_with_comma: the first argument, a comma and spaces after it.
            let (fs, fe) = r(&args[0]);
            let (_, fe) = helpers::rails::range_with_surrounding_comma(src.text, fs, fe, false, true);
            let (_, fe) = helpers::range_help::range_with_surrounding_space(
                src.text,
                fs,
                fe,
                helpers::range_help::Surrounding { side: helpers::range_help::Side::Right, ..Default::default() },
            );
            let edits: Vec<((usize, usize), String)> = args
                .iter()
                .filter_map(|a| {
                    if let Some(v) = str_val(a) {
                        let v = v.strip_prefix('/').unwrap_or(&v);
                        Some((r(a), helpers::ruby_string::inspect(v.as_bytes())))
                    } else {
                        a.as_array_node().map(|_| (r(a), format!("*{}", node_text(a))))
                    }
                })
                .collect();
            cx.add_fix(os, oe, message(cx, true), |c| {
                if first_array {
                    return;
                }
                c.replace(recv.0, recv.1, "Rails.root");
                c.remove(fs, fe);
                for ((a, b), t) in &edits {
                    c.replace(*a, *b, t);
                }
                c.insert_after(s, e, ".to_s");
            });
        }
        let Some(recv) = call.receiver() else { return };
        // rails_root_join_nodes?: (send #rails_root_nodes? :join ...)
        if !has_rails_root(&recv) {
            return;
        }
        let leading = |a: &Node| str_val(a).is_some_and(|v| v.starts_with('/') || v.contains("//"));
        if cx.cfg.str("EnforcedStyle") == Some("arguments") {
            // check_for_rails_root_join_with_slash_separated_path
            if !args.iter().any(|a| str_val(a).is_some_and(|v| v.contains('/'))) || args.iter().any(leading) {
                return;
            }
            let mut edits: Vec<(usize, usize, (usize, usize), String)> = Vec::new();
            let mut safe = true;
            for a in &args {
                if !str_val(a).is_some_and(|v| v.contains('/')) {
                    continue;
                }
                let st = a.as_string_node().unwrap();
                let (Some(open), Some(close)) = (st.opening_loc(), st.closing_loc()) else { safe = false; continue };
                let text = node_text(a);
                let Some(index) = text.find('/') else { continue };
                let from = a.location().start_offset() + index;
                let to = close.start_offset();
                let _ = open;
                let rest = src.slice(from, to);
                let rest = rest.strip_prefix('/').unwrap_or(rest).to_string();
                // Deliberate difference: the path text is spliced into a "..." literal only when plain.
                safe &= helpers::rails::inert_in(&rest, '"');
                edits.push((from, to, r(a), format!(", \"{rest}\"")));
            }
            let (os, oe) = offense_range(src, s, e);
            if !safe {
                return cx.add_withheld_fix(os, oe, message(cx, false));
            }
            cx.add_fix(os, oe, message(cx, false), |c| {
                for (from, to, (a, b), ins) in &edits {
                    c.remove(*from, *to);
                    c.insert_after(*a, *b, ins);
                }
            });
        } else {
            // check_for_rails_root_join_with_string_arguments
            if args.len() <= 1 || !args.iter().all(|a| str_val(a).is_some()) || args.iter().any(leading) {
                return;
            }
            let joined: Vec<String> = args.iter().filter_map(str_val).collect();
            let joined = joined.join("/");
            let (os, oe) = offense_range(src, s, e);
            if !helpers::rails::inert_in(&joined, '"') {
                return cx.add_withheld_fix(os, oe, message(cx, false));
            }
            let first = r(&args[0]);
            let removals: Vec<(usize, usize)> =
                args[1..].iter().map(|a| helpers::rails::left_space_and_comma(src.text, r(a).0, r(a).1)).collect();
            let replacement = format!("\"{joined}\"");
            cx.add_fix(os, oe, message(cx, false), |c| {
                c.replace(first.0, first.1, &replacement);
                for &(a, b) in &removals {
                    c.remove(a, b);
                }
            });
        }
    }
}

fn on_dstr(cx: &mut Cx, node: &Node, d: &ruby_prism::InterpolatedStringNode) {
    if !has_rails_root(node) {
        return;
    }
    let parts: Vec<Node> = d.parts().iter().collect();
    // dstr_separated_by_colon?
    if parts.iter().skip(1).any(|p| str_val(p).is_some() && node_text(p).starts_with(':')) {
        return;
    }
    let Some(idx) = parts.iter().position(has_rails_root) else { return };
    let src = cx.src;
    let (s, e) = (node.location().start_offset(), node.location().end_offset());
    let (os, oe) = offense_range(src, s, e);
    // children[idx].children.first: the interpolation's first statement (a send).
    let inner: Option<ruby_prism::CallNode> = parts[idx]
        .as_embedded_statements_node()
        .and_then(|es| es.statements())
        .and_then(|st| st.body().iter().next())
        .and_then(|n| helpers::rails::send_node(&n));
    let next = parts.get(idx + 1);
    // check_for_slash_after_rails_root_in_dstr
    if let Some(n) = next
        && n.as_string_node().is_some()
        && node_text(n).starts_with('/')
        && let Some(root) = inner.as_ref()
    {
        let arg: String = parts[idx + 1..].iter().map(|p| node_text(p)).collect::<String>();
        let arg = arg.strip_prefix('/').unwrap_or(&arg).to_string();
        let removals: Vec<(usize, usize)> = parts[idx + 1..].iter().map(|p| (p.location().start_offset(), p.location().end_offset())).collect();
        let edit = if method_name(root) == "join" {
            // `insert_after(node.last_argument)` raises without arguments: the offense is lost.
            let Some(l) = send_args(root).last().map(|l| (l.location().start_offset(), l.location().end_offset(), true)) else {
                return;
            };
            Some(l)
        } else {
            let (rs, re) = send_range(root);
            Some((rs, re, false))
        };
        // Deliberate difference: only a plain "..." string's text is spliced into a new "..." literal.
        // Source moved out of another interpolating literal (`%{...}`, a heredoc) keeps its meaning
        // in "..." when it has no `"`, backslash or line break.
        let plain = d.opening_loc().is_some_and(|o| o.as_slice() == b"\"") || !arg.contains(['"', '\\', '\n', '\r']);
        if !plain {
            cx.add_withheld_fix(os, oe, message(cx, false));
        } else {
            cx.add_fix(os, oe, message(cx, false), |c| {
                match edit {
                    Some((a, b, true)) => c.insert_after(a, b, &format!(", \"{arg}\"")),
                    Some((a, b, false)) => c.replace(a, b, &format!("Rails.root.join(\"{arg}\")")),
                    None => {}
                }
                for &(a, b) in &removals {
                    c.remove(a, b);
                }
            });
        }
    }
    // check_for_extension_after_rails_root_join_in_dstr
    static EXT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A\.[A-Za-z]+").unwrap());
    if let Some(n) = next
        && n.as_string_node().is_some()
        && EXT.is_match(node_text(n))
    {
        let Some(root) = inner.as_ref() else { return };
        // `rails_root_node.last_argument.str_type?` (a Rails.root without arguments raises).
        let rargs = send_args(root);
        let Some(last) = rargs.last() else { return };
        let ext = node_text(n).to_string();
        let last_str = last.as_string_node().filter(|_| str_val(last).is_some());
        let target = last_str.as_ref().and_then(|st| st.closing_loc());
        let (ns, ne) = (n.location().start_offset(), n.location().end_offset());
        // Deliberate difference: the extension text is spliced in only when inert in that literal.
        let quote = last_str.as_ref().and_then(|st| st.opening_loc()).map(|o| o.as_slice());
        let inert = match quote {
            Some(b"'") => helpers::rails::inert_in(&ext, '\''),
            Some(b"\"") => helpers::rails::inert_in(&ext, '"'),
            _ => helpers::rails::inert_text(&ext),
        };
        if target.is_some() && !inert {
            return cx.add_withheld_fix(os, oe, message(cx, false));
        }
        cx.add_fix(os, oe, message(cx, false), |c| {
            if let Some(close) = target {
                c.insert_before(close.start_offset(), close.end_offset(), &ext);
                c.remove(ns, ne);
            }
        });
    }
}
