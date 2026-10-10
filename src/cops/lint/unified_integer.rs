use crate::prelude::*;

pub const NAME: &str = "Lint/UnifiedInteger";
pub static COP: UnifiedInteger = UnifiedInteger;
pub struct UnifiedInteger;

impl Cop for UnifiedInteger {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CONSTANT_READ_NODE, kind::CONSTANT_PATH_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let (name, name_loc) = if let Some(c) = node.as_constant_read_node() {
            (c.name().as_slice(), node.location())
        } else {
            let p = node.as_constant_path_node().unwrap();
            // `::Fixnum = 1` is a casgn, not a const.
            if p.parent().is_some() || cx.parent().is_some_and(|x| kind::of(x) == kind::CONSTANT_PATH_WRITE_NODE) {
                return;
            }
            (p.name().map_or(&b""[..], |n| n.as_slice()), p.name_loc())
        };
        let klass = match name {
            b"Fixnum" => "Fixnum",
            b"Bignum" => "Bignum",
            _ => return,
        };
        let loc = node.location();
        let correct = cx.config.target_ruby > 2.3;
        let (ns, ne) = (name_loc.start_offset(), name_loc.end_offset());
        cx.add_fix(loc.start_offset(), loc.end_offset(), format!("Use `Integer` instead of `{klass}`."), |c| {
            if correct {
                c.replace(ns, ne, "Integer");
            }
        });
    }
}
