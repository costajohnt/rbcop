use crate::prelude::*;

pub const NAME: &str = "Rails/Env";
pub static COP: Env = Env;
pub struct Env;

const ALLOWED: &[&str] = &[
    "unicode_normalized?", "exclude?", "empty?", "acts_like_string?", "include?", "is_utf8?", "casecmp?", "match?",
    "starts_with?", "ends_with?", "start_with?", "end_with?", "valid_encoding?", "ascii_only?", "between?", "local?",
];

impl Cop for Env {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = helpers::rails::send(node) else { return };
        if method_name(&call) != "env"
            || !call.receiver().is_some_and(|r| helpers::const_name::const_name(&r).as_deref() == Some("Rails"))
        {
            return;
        }
        // parent.respond_to?(:predicate_method?) && parent.predicate_method?: a send, block or def.
        let anc = cx.ancestors;
        let Some(i) = helpers::rails::parent_index(anc) else { return };
        let p = &anc[i];
        let (name, range) = if let Some(c) = p.as_call_node() {
            (method_name(&c).to_string(), send_range(&c))
        } else if p.as_block_node().is_some() && i > 0 {
            let Some(c) = anc[i - 1].as_call_node() else { return };
            (method_name(&c).to_string(), (c.location().start_offset(), c.location().end_offset()))
        } else if p.as_defined_node().is_some() {
            ("defined?".to_string(), (p.location().start_offset(), p.location().end_offset()))
        } else if let Some(d) = p.as_def_node() {
            (String::from_utf8_lossy(d.name().as_slice()).into_owned(), (p.location().start_offset(), p.location().end_offset()))
        } else {
            return;
        };
        if !name.ends_with('?') || ALLOWED.contains(&name.as_str()) {
            return;
        }
        cx.add(range.0, range.1, "Use Feature Flags or config instead of `Rails.env`.");
    }
}
