use crate::prelude::*;

pub const NAME: &str = "Lint/UriEscapeUnescape";
pub static COP: UriEscapeUnescape = UriEscapeUnescape;
pub struct UriEscapeUnescape;

/// `(const {nil? cbase} :URI)`: Some(true) for `::URI`, Some(false) for `URI`.
fn uri_const(node: &Node) -> Option<bool> {
    if let Some(c) = node.as_constant_read_node() {
        return (c.name().as_slice() == b"URI").then_some(false);
    }
    let p = node.as_constant_path_node()?;
    (p.parent().is_none() && p.name().is_some_and(|n| n.as_slice() == b"URI")).then_some(true)
}

impl Cop for UriEscapeUnescape {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if is_csend(&call) {
            return;
        }
        let name = method_name(&call);
        let replacements = match name {
            "escape" | "encode" => ["CGI.escape", "URI.encode_www_form", "URI.encode_www_form_component"],
            "unescape" | "decode" => ["CGI.unescape", "URI.decode_www_form", "URI.decode_www_form_component"],
            _ => return,
        };
        let Some(top_level) = call.receiver().and_then(|r| uri_const(&r)) else { return };
        let msg = format!(
            "`{}URI.{name}` method is obsolete and should not be used. Instead, use `{}`, `{}` or `{}` depending on your specific use case.",
            if top_level { "::" } else { "" },
            replacements[0],
            replacements[1],
            replacements[2]
        );
        let (s, e) = send_range(&call);
        cx.add(s, e, msg);
    }
}
