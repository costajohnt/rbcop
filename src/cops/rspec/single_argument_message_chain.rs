use crate::prelude::*;

pub const NAME: &str = "RSpec/SingleArgumentMessageChain";
pub static COP: SingleArgumentMessageChain = SingleArgumentMessageChain;
pub struct SingleArgumentMessageChain;

static SYMBOL_NAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A[\p{L}_][\p{L}\p{N}_]*[?!=]?\z").unwrap());

impl Cop for SingleArgumentMessageChain {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send _ {:receive_message_chain :stub_chain} $_)
        let call = node.as_call_node().unwrap();
        let method = method_name(&call);
        if is_csend(&call) || !matches!(method, "receive_message_chain" | "stub_chain") {
            return;
        }
        let [arg] = &send_args(&call)[..] else { return };
        // valid_usage?
        let hash_pairs = helpers::rails::hash_elements(arg);
        let single_pair = hash_pairs.as_ref().is_some_and(|p| p.len() == 1 && p[0].as_assoc_node().is_some());
        let array = arg.as_array_node();
        if !helpers::literal_node::literal(arg) {
            return;
        }
        let valid = if hash_pairs.is_some() {
            !single_pair
        } else if let Some(a) = &array {
            a.elements().iter().count() != 1
        } else {
            // `node.to_s.include?('.')`: the s-expression
            helpers::rspec::skey(cx.src, Some(arg)).contains('.')
        };
        if valid {
            return;
        }
        let replacement = if method == "receive_message_chain" { "receive" } else { "stub" };
        let msg = format!("Use `{replacement}` instead of calling `{method}` with a single argument.");
        let sel = call.message_loc().unwrap();
        let (ss, se) = (sel.start_offset(), sel.end_offset());
        let (as_, ae) = helpers::rspec::node_range(arg);
        let mut edits: Vec<(usize, usize, String)> = vec![(ss, se, replacement.to_string())];
        if single_pair {
            let pair = hash_pairs.unwrap().remove(0);
            let pair = pair.as_assoc_node().unwrap();
            let key = pair.key();
            let key_text = if helpers::rspec::is_sym(cx, &key) {
                let v = helpers::rails::sym_value(&key).unwrap_or_default();
                // The symbol's text is written as a new `:name` literal: only a plain name.
                if !SYMBOL_NAME.is_match(&v) {
                    let (s, e) = (ss, se);
                    cx.add_withheld_fix(s, e, msg);
                    return;
                }
                format!(":{v}")
            } else {
                node_text(&key).to_string()
            };
            edits.push((as_, ae, key_text));
            // insert_after(arg.parent.loc.end): RuboCop raises without parentheses
            let Some(close) = call.closing_loc() else { return };
            edits.push((close.start_offset(), close.end_offset(), format!(".and_return({})", node_text(&pair.value()))));
        } else if let Some(a) = array {
            let first = a.elements().iter().next().unwrap();
            edits.push((as_, ae, node_text(&first).to_string()));
        }
        let n = edits.len();
        cx.add_fix(ss, se, msg, |c| {
            for (i, (s, e, t)) in edits.iter().enumerate() {
                if i == 2 && n == 3 {
                    c.insert_after(*s, *e, t);
                } else {
                    c.replace(*s, *e, t);
                }
            }
        });
    }
}
