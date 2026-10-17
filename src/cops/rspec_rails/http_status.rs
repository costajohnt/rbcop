use crate::prelude::*;
use helpers::rails::SYMBOL_TO_STATUS_CODE as CODES;

pub const NAME: &str = "RSpecRails/HttpStatus";
pub static COP: HttpStatus = HttpStatus;
pub struct HttpStatus;

const ALLOWED: &[&str] = &["error", "success", "missing", "redirect"];

/// Ruby's String#to_i for a string of digits.
fn to_i(s: &str) -> Option<i64> {
    s.parse().ok()
}

impl Cop for HttpStatus {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send nil? :have_http_status ${int sym str})
        let call = node.as_call_node().unwrap();
        if call.receiver().is_some() || method_name(&call) != "have_http_status" {
            return;
        }
        let [arg] = &send_args(&call)[..] else { return };
        let int = arg.as_integer_node().is_some();
        let sym = helpers::rspec::is_sym(cx, arg);
        let str = helpers::rspec::is_str(arg);
        if !(int || sym || str) || (str && is_heredoc(arg)) {
            return;
        }
        let sym_value = if sym { helpers::rails::sym_value(arg) } else { None };
        let str_value = if str { Some(String::from_utf8_lossy(arg.as_string_node().unwrap().unescaped()).into_owned()) } else { None };
        let allowed_symbol = sym_value.as_deref().is_some_and(|v| ALLOWED.contains(&v));
        let known_symbol = sym_value.as_deref().is_some_and(|v| CODES.iter().any(|(k, _)| *k == v));
        let numeric_string = str_value.as_deref().is_some_and(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()));
        let numeric = int || numeric_string;
        let number = if int {
            int_literal_value(arg).and_then(|v| i64::try_from(v).ok())
        } else if numeric_string {
            to_i(str_value.as_deref().unwrap())
        } else {
            None
        };
        let known_code = numeric && number.is_some_and(|n| CODES.iter().any(|(_, c)| *c == n));
        let custom_code = numeric && !known_code;
        let code_symbol = || number.and_then(|n| CODES.iter().find(|(_, c)| *c == n).map(|(k, _)| *k));
        let style = cx.cfg.str("EnforcedStyle");
        let (offensive, prefer, range) = match style {
            Some("symbolic") => {
                let offensive = !(allowed_symbol || known_symbol || custom_code);
                let prefer = known_code.then(code_symbol).flatten().map(|s| helpers::ruby_string::symbol_inspect(s.as_bytes()));
                (offensive, prefer, helpers::rspec::node_range(arg))
            }
            Some("numeric") => {
                let offensive = !(int || allowed_symbol || custom_code);
                let n = if known_code {
                    number
                } else {
                    let name = sym_value.clone().or(str_value.clone()).unwrap_or_default();
                    CODES.iter().find(|(k, _)| *k == name).map(|(_, c)| *c)
                };
                (offensive, n.map(|n| n.to_string()), helpers::rspec::node_range(arg))
            }
            Some("be_status") => {
                let offensive = !(allowed_symbol || custom_code);
                let status = if known_symbol {
                    sym_value.clone()
                } else if known_code {
                    code_symbol().map(String::from)
                } else {
                    // normalize_str: `node.value.to_s` when it names a status
                    let v = sym_value.clone().or(str_value.clone()).or_else(|| number.map(|n| n.to_string())).unwrap_or_default();
                    CODES.iter().any(|(k, _)| *k == v).then_some(v)
                };
                (offensive, status.map(|s| format!("be_{s}")), send_range(&call))
            }
            _ => return,
        };
        if !offensive {
            return;
        }
        let (s, e) = range;
        let msg = match &prefer {
            Some(p) => format!("Prefer `{p}` over `{}` to describe HTTP status code.", cx.src.slice(s, e)),
            None => "Unknown status code.".to_string(),
        };
        cx.add_fix(s, e, msg, |c| {
            if let Some(p) = &prefer {
                c.replace(s, e, p);
            }
        });
    }
}
