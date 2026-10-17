use crate::prelude::*;

pub const NAME: &str = "RSpecRails/MinitestAssertions";
pub static COP: MinitestAssertions = MinitestAssertions;
pub struct MinitestAssertions;

static WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A[A-Za-z0-9_]+\z").unwrap());

impl Cop for MinitestAssertions {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if call.receiver().is_some() {
            return;
        }
        let name = method_name(&call);
        let args = send_args(&call);
        let src = |n: &Node| node_text(n).to_string();
        let base = name
            .strip_prefix("assert_not_")
            .or_else(|| name.strip_prefix("refute_"))
            .or_else(|| name.strip_prefix("assert_"))
            .unwrap_or("");
        let negated = name.starts_with("assert_not_") || name.starts_with("refute_");
        // (expected, actual, failure message, assertion)
        let (actual, message, assertion): (String, Option<String>, String) = match (base, &args[..]) {
            ("equal", [e, a, m @ ..]) if m.len() <= 1 => (src(a), m.first().map(src), format!("eq({})", src(e))),
            ("kind_of", [e, a, m @ ..]) if m.len() <= 1 => (src(a), m.first().map(src), format!("be_a_kind_of({})", src(e))),
            ("instance_of", [e, a, m @ ..]) if m.len() <= 1 => {
                (src(a), m.first().map(src), format!("be_an_instance_of({})", src(e)))
            }
            ("includes", [c, e, m @ ..]) if m.len() <= 1 => (src(c), m.first().map(src), format!("include({})", src(e))),
            ("in_delta", [e, a, rest @ ..]) if rest.len() <= 2 => {
                let delta = rest.first().map_or("0.001".to_string(), src);
                (src(a), rest.get(1).map(src), format!("be_within({delta}).of({})", src(e)))
            }
            ("predicate", [s, p, m @ ..]) if m.len() <= 1 && helpers::rspec::is_sym(cx, p) => {
                let pred = helpers::rails::sym_value(p).unwrap_or_default();
                let Some(matcher) = pred.strip_suffix('?') else { return };
                if !WORD.is_match(matcher) {
                    return;
                }
                (src(s), m.first().map(src), format!("be_{matcher}"))
            }
            ("match", [e, a, m @ ..]) if m.len() <= 1 && name != "assert_not_match" => {
                (src(a), m.first().map(src), format!("match({})", src(e)))
            }
            ("nil", [a, m @ ..]) if m.len() <= 1 => (src(a), m.first().map(src), "eq(nil)".into()),
            ("empty", [a, m @ ..]) if m.len() <= 1 => (src(a), m.first().map(src), "be_empty".into()),
            ("true", [a, m @ ..]) if m.len() <= 1 && name == "assert_true" => (src(a), m.first().map(src), "be(true)".into()),
            ("false", [a, m @ ..]) if m.len() <= 1 && name == "assert_false" => {
                (src(a), m.first().map(src), "be(false)".into())
            }
            ("response", [e, m @ ..]) if m.len() <= 1 && name == "assert_response" => {
                ("response".into(), m.first().map(src), format!("have_http_status({})", src(e)))
            }
            ("redirected_to", [e, m @ ..]) if m.len() <= 1 && name == "assert_redirected_to" => {
                ("response".into(), m.first().map(src), format!("redirect_to({})", src(e)))
            }
            _ => return,
        };
        let runner = if negated { "not_to" } else { "to" };
        let preferred = match message {
            None => format!("expect({actual}).{runner} {assertion}"),
            Some(m) => format!("expect({actual}).{runner}({assertion}, {m})"),
        };
        let (s, e) = send_range(&call);
        cx.add_fix(s, e, format!("Use `{preferred}`."), |c| c.replace(s, e, &preferred));
    }
}
