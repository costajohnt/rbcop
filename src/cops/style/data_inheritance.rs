use crate::helpers::parser_parent::is_top_const;
use crate::helpers::range_help::{Side, Surrounding, range_with_surrounding_space};
use crate::helpers::whole_lines::range_by_whole_lines;
use crate::prelude::*;
use crate::cops::style_struct_inheritance::DefinesConstants;
use ruby_prism::Visit;

pub const NAME: &str = "Style/DataInheritance";
pub static COP: DataInheritance = DataInheritance;
pub struct DataInheritance;

impl Cop for DataInheritance {
    fn correctable(&self) -> bool {
        true
    }
    fn ruby_range(&self) -> (f64, f64) {
        (3.2, f64::MAX)
    }
    fn kinds(&self) -> &'static [usize] {
        &[kind::CLASS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let class = node.as_class_node().unwrap();
        let (Some(parent), Some(op)) = (class.superclass(), class.inheritance_operator_loc()) else { return };
        let Some(call) = parent.as_call_node() else { return };
        // {(send (const {nil? cbase} :Data) :define ...) (block (send ...) ...)}
        let block = call.block().filter(|b| b.as_block_node().is_some());
        if is_csend(&call)
            || method_name(&call) != "define"
            || !call.receiver().is_some_and(|r| is_top_const(&r, "Data"))
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
            Block(Option<(usize, usize)>, (usize, usize)),
            Remove(usize, usize),
            InsertAfter,
        }
        let fix = if let Some(b) = &block {
            let b = b.as_block_node().unwrap();
            let (open, close) = (b.opening_loc(), b.closing_loc());
            let braces = text(&open) == "{";
            let o = Surrounding { newlines: false, ..Default::default() };
            let end = range_with_surrounding_space(src, close.start_offset(), close.end_offset(), o);
            Parent::Block(braces.then(|| (open.start_offset(), open.end_offset())), end)
        } else if class.body().is_none() {
            if cx.src.line_of(loc.start_offset()) == cx.src.line_of(loc.end_offset()) {
                Parent::Remove(pe, loc.end_offset())
            } else {
                let (s, e) = range_by_whole_lines(src, class_end.start_offset(), class_end.end_offset(), true);
                Parent::Remove(s, e)
            }
        } else {
            Parent::InsertAfter
        };
        let msg = "Don't extend an instance initialized by `Data.define`. Use a block to customize the class.";
        cx.add_fix(ps, pe, msg, |c| {
            c.remove(kw_range.0, kw_range.1);
            c.replace(op.start_offset(), op.end_offset(), "=");
            match &fix {
                Parent::Block(open, (s, e)) => {
                    if let Some((os, oe)) = open {
                        c.replace(*os, *oe, "do");
                    }
                    c.remove(*s, *e);
                }
                Parent::Remove(s, e) => c.remove(*s, *e),
                Parent::InsertAfter => c.insert_after(ps, pe, " do"),
            }
        });
    }
}
