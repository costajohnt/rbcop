use crate::prelude::*;

pub const NAME: &str = "Rails/SafeNavigation";
pub static COP: SafeNavigation = SafeNavigation;
pub struct SafeNavigation;

static SETTER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A[A-Za-z0-9_]+=\z").unwrap());

#[derive(Default)]
struct Ignored(Vec<(usize, usize)>);

impl Cop for SafeNavigation {
    fn correctable(&self) -> bool {
        true
    }

    fn ruby_range(&self) -> (f64, f64) {
        (2.3, f64::MAX)
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send _ ${:try :try!} $_ ...)
        let Some(call) = helpers::rails::send(node) else { return };
        let m = method_name(&call);
        if !matches!(m, "try" | "try!") {
            return;
        }
        let args = send_args(&call);
        let Some(dispatch) = args.first() else { return };
        if m == "try" && !cx.cfg.get("ConvertTry").is_some_and(|v| !matches!(v, Value::Null | Value::Bool(false))) {
            return;
        }
        let sym = helpers::rails::sym_value(dispatch).is_some();
        let block_sym = dispatch
            .as_block_argument_node()
            .and_then(|b| b.expression())
            .and_then(|e| helpers::rails::sym_value(&e));
        if !sym && block_sym.is_none() {
            return;
        }
        let (s, e) = send_range(&call);
        // part_of_ignored_node?: inside an earlier offense's node.
        if cx.state::<Ignored>().0.iter().any(|&(a, b)| a <= s && e <= b) {
            return;
        }
        let replacement = match block_sym {
            Some(name) => format!("&.{name}"),
            None => {
                let method = &node_text(dispatch)[1..];
                let params: Vec<&str> = args[1..].iter().map(|a| node_text(a)).collect();
                let params = params.join(", ");
                if SETTER.is_match(method) {
                    format!("&.{} = {params}", &method[..method.len() - 1])
                } else if args.len() == 1 {
                    format!("&.{method}")
                } else {
                    format!("&.{method}({params})")
                }
            }
        };
        let dot = call.call_operator_loc().map(|d| d.start_offset());
        let has_recv = call.receiver().is_some();
        cx.add_fix(s, e, format!("Use safe navigation (`&.`) instead of `{m}`."), |c| {
            if has_recv {
                c.replace(dot.unwrap_or(s), e, &replacement);
            } else {
                c.insert_before(s, e, "self");
                c.replace(s, e, &replacement);
            }
        });
        cx.state::<Ignored>().0.push((s, e));
    }
}
