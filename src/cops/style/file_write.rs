use crate::helpers::file_open_patterns::{is_sym_block_pass, mode_str, single_arg_block_send};
use crate::helpers::parser_parent::{is_top_const, parser_parent};
use crate::prelude::*;
use crate::source::Source;
use ruby_prism::{InterpolatedStringNode, InterpolatedXStringNode, StringNode, Visit, XStringNode};

pub const NAME: &str = "Style/FileWrite";
pub static COP: FileWrite = FileWrite;
pub struct FileWrite;

const MODES: &[&[u8]] = &[b"w", b"wt", b"wb", b"w+", b"w+t", b"w+b"];

/// Heredocs in a subtree as `(heredoc_body.begin_pos, heredoc_end.end_pos)`.
struct Heredocs<'s, 'a> {
    src: &'s Source<'a>,
    out: Vec<(usize, usize)>,
}

impl Heredocs<'_, '_> {
    fn add(&mut self, node: &Node, first: Option<usize>) {
        let Some((_, (end_start, end_end), _)) = helpers::heredoc::parts(node) else { return };
        let begin = first.map_or(end_start, |f| self.src.line_start(self.src.line_of(f)));
        self.out.push((begin.min(end_start), end_end));
    }
}

impl<'pr> Visit<'pr> for Heredocs<'_, '_> {
    fn visit_string_node(&mut self, node: &StringNode<'pr>) {
        let c = node.content_loc();
        self.add(&node.as_node(), (!c.as_slice().is_empty()).then(|| c.start_offset()));
    }
    fn visit_x_string_node(&mut self, node: &XStringNode<'pr>) {
        let c = node.content_loc();
        self.add(&node.as_node(), (!c.as_slice().is_empty()).then(|| c.start_offset()));
    }
    fn visit_interpolated_string_node(&mut self, node: &InterpolatedStringNode<'pr>) {
        self.add(&node.as_node(), node.parts().iter().next().map(|p| p.location().start_offset()));
        ruby_prism::visit_interpolated_string_node(self, node);
    }
    fn visit_interpolated_x_string_node(&mut self, node: &InterpolatedXStringNode<'pr>) {
        self.add(&node.as_node(), node.parts().iter().next().map(|p| p.location().start_offset()));
        ruby_prism::visit_interpolated_x_string_node(self, node);
    }
}

impl Cop for FileWrite {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if method_name(&call) != "open" || is_csend(&call) {
            return;
        }
        if !call.receiver().is_some_and(|r| is_top_const(&r, "File")) {
            return;
        }
        let args = send_args(&call);
        let (filename, mode) = match args.as_slice() {
            [f, m] => (f, m),
            [f, m, b] if is_sym_block_pass(b, "write") => (f, m),
            _ => return,
        };
        let Some(mode) = mode_str(mode, MODES) else { return };
        let start = node.location().start_offset();
        let (content, write_start, write_end) = if call.block().is_some_and(|b| b.as_block_node().is_some()) {
            let Some(send) = single_arg_block_send(&call, "write") else { return };
            let mut a = send_args(&send);
            if a.len() != 1 {
                return;
            }
            (a.pop().unwrap(), start, node.location().end_offset())
        } else {
            let Some(p) = parser_parent(cx.ancestors).and_then(|p| p.as_call_node()) else { return };
            if is_csend(&p) || method_name(&p) != "write" {
                return;
            }
            let mut a = send_args(&p);
            if a.len() != 1 {
                return;
            }
            let (ps, pe) = send_range(&p);
            (a.pop().unwrap(), ps, pe)
        };
        if content.as_splat_node().is_some() {
            return;
        }
        let method = if mode.ends_with(b"b") { "binwrite" } else { "write" };
        let mut replacement = format!("{method}({}, {})", node_text(filename), node_text(&content));
        let mut h = Heredocs { src: cx.src, out: Vec::new() };
        h.visit(filename);
        h.visit(&content);
        let mut removed: Vec<(usize, usize)> = h.out.into_iter().filter(|&(_, e)| e <= write_end).collect();
        removed.sort_by_key(|&(b, _)| b);
        for (b, e) in removed {
            replacement.push('\n');
            replacement.push_str(&cx.src.text[b..e]);
        }
        let sel = call.message_loc().unwrap().start_offset();
        cx.add_fix(write_start, write_end, format!("Use `File.{method}`."), |c| c.replace(sel, write_end, &replacement));
    }
}
