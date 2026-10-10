use crate::prelude::*;

pub const NAME: &str = "Lint/HashNewWithKeywordArgumentsAsDefault";
pub static COP: HashNewWithKeywordArgumentsAsDefault = HashNewWithKeywordArgumentsAsDefault;
pub struct HashNewWithKeywordArgumentsAsDefault;

impl Cop for HashNewWithKeywordArgumentsAsDefault {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send (const {nil? (cbase)} :Hash) :new $[hash !braces?])
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || method_name(&call) != "new" {
            return;
        }
        if !call.receiver().is_some_and(|r| helpers::parser_parent::is_top_const(&r, "Hash")) {
            return;
        }
        let args = send_args(&call);
        let [arg] = &args[..] else { return };
        let Some(hash) = arg.as_keyword_hash_node() else { return };
        let pairs: Vec<Node> = hash.elements().iter().filter(|e| e.as_assoc_node().is_some()).collect();
        if let [pair] = &pairs[..] {
            let key = pair.as_assoc_node().unwrap().key();
            if key.as_symbol_node().is_some_and(|s| s.unescaped() == b"capacity") {
                return;
            }
        }
        let l = arg.location();
        let (start, end) = (l.start_offset(), l.end_offset());
        cx.add_fix(start, end, "Use a hash literal instead of keyword arguments.", |c| c.wrap(start, end, "{", "}"));
    }
}
