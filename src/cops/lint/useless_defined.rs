use crate::prelude::*;

pub const NAME: &str = "Lint/UselessDefined";
pub static COP: UselessDefined = UselessDefined;
pub struct UselessDefined;

impl Cop for UselessDefined {
    fn kinds(&self) -> &'static [usize] {
        &[kind::DEFINED_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let arg = node.as_defined_node().unwrap().value();
        // `__FILE__` is a parser `str`.
        let ty = match kind::of(&arg) {
            kind::STRING_NODE | kind::INTERPOLATED_STRING_NODE | kind::SOURCE_FILE_NODE => "string",
            kind::SYMBOL_NODE | kind::INTERPOLATED_SYMBOL_NODE => "symbol",
            _ => return,
        };
        let loc = node.location();
        cx.add(
            loc.start_offset(),
            loc.end_offset(),
            format!("Calling `defined?` with a {ty} argument will always return a truthy value."),
        );
    }
}
