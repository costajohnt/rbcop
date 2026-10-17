use crate::prelude::*;

pub const NAME: &str = "Rails/OrderArguments";
pub static COP: OrderArguments = OrderArguments;
pub struct OrderArguments;

static EXPR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\A([A-Za-z0-9_]+) ?(asc|desc)?\z").unwrap());

fn replacement(exprs: &[String]) -> Option<String> {
    let mut args = Vec::new();
    for e in exprs {
        for part in helpers::rails::ruby_split(e, ',') {
            let c = EXPR.captures(helpers::ruby_string::strip(part))?;
            let column = c[1].to_lowercase();
            let dir = c.get(2).map_or("asc".to_string(), |d| d.as_str().to_lowercase());
            args.push((column, dir));
        }
    }
    if args.iter().any(|(c, _)| c.bytes().all(|b| b.is_ascii_digit())) {
        return None;
    }
    let mut use_hash = false;
    let out: Vec<String> = args
        .into_iter()
        .map(|(c, d)| {
            if d == "asc" && !use_hash {
                format!(":{c}")
            } else {
                use_hash = true;
                format!("{c}: :{d}")
            }
        })
        .collect();
    Some(out.join(", "))
}

impl Cop for OrderArguments {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (call _ :order (str $_value)+)
        let call = node.as_call_node().unwrap();
        if method_name(&call) != "order" {
            return;
        }
        let args = send_args(&call);
        if args.is_empty() {
            return;
        }
        let Some(values) = args.iter().map(helpers::rails::str_value).collect::<Option<Vec<String>>>() else { return };
        let Some(preferred) = replacement(&values) else { return };
        let s = args[0].location().start_offset();
        let e = args.last().unwrap().location().end_offset();
        cx.add_fix(s, e, format!("Prefer `{preferred}` instead."), |c| c.replace(s, e, &preferred));
    }
}
