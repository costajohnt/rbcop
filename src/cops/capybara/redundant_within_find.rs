use crate::prelude::*;

pub const NAME: &str = "Capybara/RedundantWithinFind";
pub static COP: RedundantWithinFind = RedundantWithinFind;
pub struct RedundantWithinFind;

impl Cop for RedundantWithinFind {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send nil? :within $(send nil? %FIND_METHODS ...))
        let call = node.as_call_node().unwrap();
        if call.receiver().is_some() || method_name(&call) != "within" {
            return;
        }
        let [arg] = &send_args(&call)[..] else { return };
        let Some(find) = arg.as_call_node() else { return };
        let name = method_name(&find);
        if find.receiver().is_some() || !matches!(name, "find" | "find_by_id") || helpers::rspec::block_of(&find).is_some() {
            return;
        }
        let args = send_args(&find);
        let sources = || args.iter().map(|a| node_text(a)).collect::<Vec<_>>().join(", ");
        let msg = format!("Redundant `within {name}(...)` call detected.");
        let (s, e) = send_range(&find);
        let replacement = match args.first() {
            _ if name == "find" => sources(),
            Some(first) if helpers::rspec::is_str(first) => {
                // build_escaped_selector: the id's value written between the literal's first
                // character, as RuboCop does; only an inert value in a quoted literal is written.
                let quote = node_text(first).chars().next().unwrap_or('\'');
                let value = String::from_utf8_lossy(first.as_string_node().unwrap().unescaped()).into_owned();
                if !matches!(quote, '\'' | '"') || !helpers::rails::inert_in(&value, quote) {
                    cx.add_withheld_fix(s, e, msg);
                    return;
                }
                let escaped = value.replace('.', if quote == '"' { "\\\\." } else { "\\." });
                let mut parts = vec![format!("{quote}#{escaped}{quote}")];
                parts.extend(args[1..].iter().map(|a| node_text(a).to_string()));
                parts.join(", ")
            }
            _ => {
                let src = sources();
                match src.chars().next() {
                    Some(q @ ('"' | '\'')) => format!("{q}#{}", &src[1..]),
                    _ => src,
                }
            }
        };
        cx.add_fix(s, e, msg, |c| c.replace(s, e, &replacement));
    }
}
