use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Rails/HttpStatusNameConsistency";
pub static COP: HttpStatusNameConsistency = HttpStatusNameConsistency;
pub struct HttpStatusNameConsistency;

fn preferred(sym: &str) -> Option<&'static str> {
    match sym {
        "unprocessable_entity" => Some("unprocessable_content"),
        "payload_too_large" => Some("content_too_large"),
        _ => None,
    }
}

impl Cop for HttpStatusNameConsistency {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // requires_gem 'rack', '>= 3.1.0'
        if !cx.config.gem_at_least("rack", "3.1.0") {
            return;
        }
        let Some(call) = helpers::rails::send(node) else { return };
        if call.receiver().is_some() {
            return;
        }
        let args = send_args(&call);
        let is_hash = |n: &Node| helpers::rails::hash_elements(n).is_some();
        let idx = match (method_name(&call), args.as_slice()) {
            ("render" | "redirect_to", [_, h]) if is_hash(h) => 1,
            ("render" | "redirect_to", [h]) if is_hash(h) => 0,
            ("head" | "assert_response", [_, ..]) => 0,
            ("assert_redirected_to", [_, h, ..]) if is_hash(h) => 1,
            ("assert_redirected_to", [h, ..]) if is_hash(h) => 0,
            _ => return,
        };
        let status: Node = args.into_iter().nth(idx).unwrap();
        let target = if is_hash(&status) {
            match helpers::rails::hash_value(&status, "status") {
                Some(v) => v,
                None => return,
            }
        } else {
            status
        };
        // check_status_name_consistency: the node, else every descendant sym (not below a match).
        struct Syms(Vec<(usize, usize, String)>);
        impl<'pr> Visit<'pr> for Syms {
            fn visit_symbol_node(&mut self, s: &ruby_prism::SymbolNode<'pr>) {
                let v = String::from_utf8_lossy(s.unescaped()).into_owned();
                if preferred(&v).is_some() {
                    let (a, b) = helpers::rails::sym_range(&s.as_node());
                    self.0.push((a, b, v));
                }
            }
        }
        let mut syms = Syms(Vec::new());
        syms.visit(&target);
        for (s, e, v) in syms.0 {
            let p = preferred(&v).unwrap();
            let replacement = format!(":{p}");
            cx.add_fix(s, e, format!("Prefer `:{p}` over `:{v}`."), |c| c.replace(s, e, &replacement));
        }
    }
}
