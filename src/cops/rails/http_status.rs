use crate::prelude::*;
use helpers::rails::SYMBOL_TO_STATUS_CODE as CODES;

pub const NAME: &str = "Rails/HttpStatus";
pub static COP: HttpStatus = HttpStatus;
pub struct HttpStatus;

impl Cop for HttpStatus {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = helpers::rails::send(node) else { return };
        if call.receiver().is_some() {
            return;
        }
        let args = send_args(&call);
        let is_hash = |n: &Node| helpers::rails::hash_elements(n).is_some();
        let int_or_sym = |n: &Node| n.as_integer_node().is_some() || helpers::rails::sym_value(n).is_some();
        let idx = match (method_name(&call), args.as_slice()) {
            ("render" | "redirect_to", [_, h]) if is_hash(h) => 1,
            ("render" | "redirect_to", [h]) if is_hash(h) => 0,
            ("head" | "assert_response", [f, ..]) if int_or_sym(f) => 0,
            ("assert_redirected_to", [_, h, ..]) if is_hash(h) => 1,
            ("assert_redirected_to", [h, ..]) if is_hash(h) => 0,
            _ => return,
        };
        let arg = args.into_iter().nth(idx).unwrap();
        // status_code: (hash <(pair (sym :status) ${int sym str}) ...>)
        let status = if is_hash(&arg) {
            let found = helpers::rails::hash_pairs(&arg).into_iter().find_map(|p| {
                let v = helpers::rails::pair_value(&p);
                let ok = v.as_integer_node().is_some() || helpers::rails::str_or_sym(&v).is_some();
                (helpers::rails::sym_value(&p.key()).as_deref() == Some("status") && ok).then_some(v)
            });
            match found {
                Some(v) => v,
                None => return,
            }
        } else {
            arg
        };
        let (s, e) = (status.location().start_offset(), status.location().end_offset());
        let (msg, preferred) = if cx.cfg.str("EnforcedStyle") == Some("numeric") {
            // NumericStyleChecker
            let Some(sym) = helpers::rails::sym_value(&status) else { return };
            if matches!(sym.as_str(), "error" | "success" | "missing" | "redirect") {
                return;
            }
            let Some((_, code)) = CODES.iter().find(|(k, _)| *k == sym) else { return };
            let shown = helpers::ruby_string::symbol_inspect(sym.as_bytes());
            (format!("Prefer `{code}` over `{shown}` to define HTTP status code."), code.to_string())
        } else {
            // SymbolicStyleChecker
            if helpers::rails::sym_value(&status).is_some() {
                return;
            }
            let (number, to_i): (String, Option<i128>) = if status.as_integer_node().is_some() {
                let Some(v) = int_literal_value(&status) else { return };
                if !CODES.iter().any(|(_, c)| *c as i128 == v) {
                    return;
                }
                (v.to_string(), Some(v))
            } else {
                let Some(v) = helpers::rails::str_value(&status) else { return };
                let to_i = ruby_to_i(&v);
                (v, Some(to_i))
            };
            let symbol = to_i.and_then(|n| CODES.iter().find(|(_, c)| *c as i128 == n).map(|(k, _)| *k));
            let preferred = match symbol {
                Some(k) => helpers::ruby_string::symbol_inspect(k.as_bytes()),
                None => "nil".to_string(),
            };
            (format!("Prefer `{preferred}` over `{number}` to define HTTP status code."), preferred)
        };
        cx.add_fix(s, e, msg, |c| c.replace(s, e, &preferred));
    }
}

/// Ruby's String#to_i (base 10).
fn ruby_to_i(s: &str) -> i128 {
    let t = s.trim_start_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c']);
    let (neg, t) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    let mut v: i128 = 0;
    let mut last_us = true;
    for b in t.bytes() {
        if b.is_ascii_digit() {
            v = v.saturating_mul(10).saturating_add((b - b'0') as i128);
            last_us = false;
        } else if b == b'_' && !last_us {
            last_us = true;
        } else {
            break;
        }
    }
    if neg { -v } else { v }
}
