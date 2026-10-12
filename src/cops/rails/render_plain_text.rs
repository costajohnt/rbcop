use crate::prelude::*;

pub const NAME: &str = "Rails/RenderPlainText";
pub static COP: RenderPlainText = RenderPlainText;
pub struct RenderPlainText;

impl Cop for RenderPlainText {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send nil? :render $(hash <$(pair (sym :text) $_) ...>))
        let Some(call) = helpers::rails::send(node) else { return };
        if !helpers::rails::is_command(&call, "render") {
            return;
        }
        let args = send_args(&call);
        let [hash] = args.as_slice() else { return };
        if helpers::rails::hash_elements(hash).is_none() {
            return;
        }
        let pairs = helpers::rails::hash_pairs(hash);
        let Some(text_pair) = pairs.iter().find(|p| helpers::rails::sym_value(&p.key()).as_deref() == Some("text")) else {
            return;
        };
        // find_content_type: `p.key.value.to_sym`, which raises for keys without a str/sym value.
        let mut content_type = None;
        for p in &pairs {
            let k = p.key();
            let Some(v) = helpers::rails::str_or_sym(&k) else { return };
            if v == "content_type" {
                content_type = Some(p);
                break;
            }
        }
        let compatible = match content_type {
            None => !cx.cfg.get("ContentTypeCompatibility").is_some_and(|v| !matches!(v, Value::Null | Value::Bool(false))),
            Some(p) => helpers::rails::str_value(&p.value()).as_deref() == Some("text/plain"),
        };
        if !compatible {
            return;
        }
        let src = cx.src;
        let range = |p: &ruby_prism::AssocNode| (p.location().start_offset(), p.location().end_offset());
        let excluded: Vec<(usize, usize)> = [Some(text_pair), content_type].into_iter().flatten().map(range).collect();
        let rest: Vec<&str> = pairs
            .iter()
            .filter(|p| !excluded.iter().any(|&x| helpers::node_equality::same(src, x, range(p))))
            .map(|p| node_text(&p.as_node()))
            .collect();
        let value = node_text(&text_pair.value());
        let replacement = if rest.is_empty() {
            format!("render plain: {value}")
        } else {
            format!("render plain: {value}, {}", rest.join(", "))
        };
        let (s, e) = send_range(&call);
        cx.add_fix(s, e, "Prefer `render plain:` over `render text:`.", |c| c.replace(s, e, &replacement));
    }
}
