use crate::prelude::*;

pub const NAME: &str = "Performance/UriDefaultParser";
pub static COP: UriDefaultParser = UriDefaultParser;
pub struct UriDefaultParser;

impl Cop for UriDefaultParser {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send (const (const ${nil? cbase} :URI) :Parser) :new)
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || method_name(&call) != "new" || !send_args(&call).is_empty() {
            return;
        }
        let Some(path) = call.receiver().and_then(|r| r.as_constant_path_node()) else { return };
        if path.name().is_none_or(|n| n.as_slice() != b"Parser") {
            return;
        }
        let Some(uri) = path.parent() else { return };
        if !helpers::parser_parent::is_top_const(&uri, "URI") {
            return;
        }
        let colons = if uri.as_constant_path_node().is_some() { "::" } else { "" };
        let msg = format!("Use `{colons}URI::DEFAULT_PARSER` instead of `{colons}URI::Parser.new`.");
        let (start, end) = send_range(&call);
        cx.add_fix(start, end, msg, |c| c.replace(start, end, &format!("{colons}URI::DEFAULT_PARSER")));
    }
}
