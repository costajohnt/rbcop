use crate::prelude::*;

pub const NAME: &str = "Rails/UnusedRenderContent";
pub static COP: UnusedRenderContent = UnusedRenderContent;
pub struct UnusedRenderContent;

/// Rack::Utils::SYMBOL_TO_STATUS_CODE (rack 3.2) codes in 100..199, 204, 205, 304, and their names.
const CODES: &[i128] = &[100, 101, 102, 103, 204, 205, 304];
const STATUSES: &[&str] =
    &["continue", "switching_protocols", "processing", "early_hints", "no_content", "reset_content", "not_modified"];
const BODY_OPTIONS: &[&str] = &[
    "action", "body", "content_type", "file", "html", "inline", "json", "js", "layout", "plain", "raw", "template",
    "text", "xml",
];

fn non_content_status(pair: &ruby_prism::AssocNode) -> bool {
    if helpers::rails::sym_value(&pair.key()).as_deref() != Some("status") {
        return false;
    }
    let v = pair.value();
    helpers::rails::sym_value(&v).is_some_and(|s| STATUSES.contains(&s.as_str()))
        || (v.as_integer_node().is_some() && int_literal_value(&v).is_some_and(|i| CODES.contains(&i)))
}

impl Cop for UnusedRenderContent {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = helpers::rails::send(node) else { return };
        if !helpers::rails::is_command(&call, "render") {
            return;
        }
        let args = send_args(&call);
        let pairs = helpers::rails::hash_pairs;
        let target = match args.as_slice() {
            [hash] if helpers::rails::hash_elements(hash).is_some() => {
                let ps = pairs(hash);
                if !ps.iter().any(non_content_status) {
                    return;
                }
                let Some(body) = ps.iter().find(|p| {
                    helpers::rails::sym_value(&p.key()).is_some_and(|k| BODY_OPTIONS.contains(&k.as_str()))
                }) else {
                    return;
                };
                (body.location().start_offset(), body.location().end_offset())
            }
            [content, hash] if helpers::rails::hash_elements(hash).is_some() => {
                if helpers::rails::str_or_sym(content).is_none() || !pairs(hash).iter().any(non_content_status) {
                    return;
                }
                (content.location().start_offset(), content.location().end_offset())
            }
            _ => return,
        };
        cx.add(target.0, target.1, "Do not specify body content for a response with a non-content status code");
    }
}
