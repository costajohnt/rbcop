use crate::helpers::parser_parent::is_top_const;
use crate::helpers::range_help::{Side, Surrounding, range_with_surrounding_space};
use crate::helpers::whole_lines::range_by_whole_lines;
use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Style/StructInheritance";
pub static COP: StructInheritance = StructInheritance;
pub struct StructInheritance;

/// `class_body.each_node(:casgn, :class, :module).any?`
#[derive(Default)]
pub struct DefinesConstants(pub bool);

impl<'pr> Visit<'pr> for DefinesConstants {
    fn visit_branch_node_enter(&mut self, node: Node<'pr>) {
        self.check(&node);
    }
    fn visit_leaf_node_enter(&mut self, node: Node<'pr>) {
        self.check(&node);
    }
}

impl DefinesConstants {
    fn check(&mut self, node: &Node) {
        self.0 |= matches!(
            kind::of(node),
            kind::CONSTANT_WRITE_NODE
                | kind::CONSTANT_PATH_WRITE_NODE
                | kind::CONSTANT_OR_WRITE_NODE
                | kind::CONSTANT_AND_WRITE_NODE
                | kind::CONSTANT_OPERATOR_WRITE_NODE
                | kind::CONSTANT_PATH_OR_WRITE_NODE
                | kind::CONSTANT_PATH_AND_WRITE_NODE
                | kind::CONSTANT_PATH_OPERATOR_WRITE_NODE
                | kind::CONSTANT_TARGET_NODE
                | kind::CONSTANT_PATH_TARGET_NODE
                | kind::CLASS_NODE
                | kind::MODULE_NODE
        );
    }
}

impl Cop for StructInheritance {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CLASS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let class = node.as_class_node().unwrap();
        let (Some(parent), Some(op)) = (class.superclass(), class.inheritance_operator_loc()) else { return };
        let Some(call) = parent.as_call_node() else { return };
        // {(send (const {nil? cbase} :Struct) :new ...) (block (send ...) ...)}
        let block = call.block().filter(|b| b.as_block_node().is_some());
        if is_csend(&call)
            || method_name(&call) != "new"
            || !call.receiver().is_some_and(|r| is_top_const(&r, "Struct"))
            || block.as_ref().is_some_and(|b| {
                b.as_block_node().unwrap().parameters().is_some_and(|p| p.as_block_parameters_node().is_none())
            })
        {
            return;
        }
        if let Some(body) = class.body() {
            let mut v = DefinesConstants::default();
            v.visit(&body);
            if v.0 {
                return;
            }
        }
        let src = cx.src.text;
        let kw = class.class_keyword_loc();
        let right = Surrounding { side: Side::Right, newlines: false, ..Default::default() };
        let kw_range = range_with_surrounding_space(src, kw.start_offset(), kw.end_offset(), right);
        let pl = parent.location();
        let (ps, pe) = (pl.start_offset(), pl.end_offset());
        let loc = node.location();
        let class_end = class.end_keyword_loc();
        enum Parent {
            Remove(usize, usize),
            Replace(usize, usize, String),
            InsertAfter,
        }
        let fix = if let Some(b) = &block {
            let close = b.as_block_node().unwrap().closing_loc();
            let o = Surrounding { newlines: false, ..Default::default() };
            let (s, e) = range_with_surrounding_space(src, close.start_offset(), close.end_offset(), o);
            Parent::Remove(s, e)
        } else if class.body().is_none() {
            if cx.src.line_of(loc.start_offset()) == cx.src.line_of(loc.end_offset()) {
                Parent::Remove(pe, loc.end_offset())
            } else {
                let (s, e) = range_by_whole_lines(src, class_end.start_offset(), class_end.end_offset(), true);
                Parent::Remove(s, e)
            }
        } else if call.opening_loc().is_none() && call.arguments().is_some() {
            let args: Vec<&str> = send_args(&call).iter().map(node_text).collect();
            let sel_end = call.message_loc().map_or(pe, |m| m.end_offset());
            Parent::Replace(sel_end, pe, format!("({}) do", args.join(", ")))
        } else {
            Parent::InsertAfter
        };
        let msg = "Don't extend an instance initialized by `Struct.new`. Use a block to customize the struct.";
        cx.add_fix(ps, pe, msg, |c| {
            c.remove(kw_range.0, kw_range.1);
            c.replace(op.start_offset(), op.end_offset(), "=");
            match &fix {
                Parent::Remove(s, e) => c.remove(*s, *e),
                Parent::Replace(s, e, t) => c.replace(*s, *e, t),
                Parent::InsertAfter => c.insert_after(ps, pe, " do"),
            }
        });
    }
}
