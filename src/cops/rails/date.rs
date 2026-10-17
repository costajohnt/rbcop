use crate::prelude::*;

pub const NAME: &str = "Rails/Date";
pub static COP: Date = Date;
pub struct Date;

const ACCEPTED: &[&str] =
    &["in_time_zone", "utc", "getlocal", "xmlschema", "iso8601", "jisx0301", "rfc3339", "httpdate", "to_i", "to_f"];

static ZONE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"([+-][0-9:]+|[0-9]Z)\z").unwrap());

/// Array#& keeping the first array's order, without duplicates.
fn intersect<'a>(chain: &'a [String], set: &[&str]) -> Vec<&'a str> {
    let mut out: Vec<&str> = Vec::new();
    for c in chain {
        if set.contains(&c.as_str()) && !out.contains(&c.as_str()) {
            out.push(c);
        }
    }
    out
}

impl Cop for Date {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CONSTANT_READ_NODE, kind::CONSTANT_PATH_NODE, kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let strict = cx.cfg.str("EnforcedStyle") == Some("strict");
        let bad_days: &[&str] = if strict { &["today", "current", "yesterday", "tomorrow"] } else { &["today"] };
        let anc = cx.ancestors;
        if let Some(call) = node.as_call_node() {
            // on_send / on_csend
            let name = method_name(&call);
            if call.receiver().is_none() || !matches!(name, "to_time" | "to_time_in_current_zone") {
                return;
            }
            let allow_to_time = !matches!(cx.cfg.get("AllowToTime"), Some(Value::Bool(false)) | Some(Value::Null));
            if allow_to_time && name == "to_time" {
                return;
            }
            // safe_chain?: no bad method (never: the node is one) or a good method in the chain.
            let mut chain = vec![name.to_string()];
            chain.extend(helpers::rails::call_ancestors(anc).filter(|c| !is_csend(c)).map(|c| method_name(&c).to_string()));
            if !strict && !intersect(&chain, ACCEPTED).is_empty() {
                return;
            }
            // safe_to_time?
            if name == "to_time" {
                let recv = call.receiver().unwrap();
                let safe = if recv.as_string_node().is_some() && !is_parser_dstr(&recv) {
                    let v = String::from_utf8_lossy(recv.as_string_node().unwrap().unescaped()).into_owned();
                    ZONE.is_match(&v)
                } else {
                    send_args(&call).len() == 1
                };
                if safe {
                    return;
                }
            }
            let sel = call.message_loc().unwrap();
            let (s, e) = (sel.start_offset(), sel.end_offset());
            if name == "to_time_in_current_zone" {
                cx.add_fix(s, e, "`to_time_in_current_zone` is deprecated. Use `in_time_zone` instead.", |c| {
                    c.replace(s, e, "in_time_zone")
                });
            }
            cx.add_with(s, e, format!("Do not use `{name}` on Date objects, because they know nothing about the time zone in use."), false);
            return;
        }
        // on_const: (const {nil? cbase} :Date) that is a send's receiver.
        if !helpers::parser_parent::is_top_const(node, "Date") {
            return;
        }
        let Some((i, send)) = helpers::rails::receiver_send_parent(anc, node) else { return };
        let chain = helpers::rails::send_chain_names(anc, i, &send);
        let days = intersect(&chain, bad_days);
        if days.is_empty() {
            return;
        }
        let method = days.join(".");
        let day = if method == "current" { "today".to_string() } else { method.clone() };
        let sel = send.message_loc().unwrap();
        let (s, e) = (sel.start_offset(), sel.end_offset());
        let (ns, ne) = match node.as_constant_path_node() {
            Some(p) => (p.name_loc().start_offset(), p.name_loc().end_offset()),
            None => (node.location().start_offset(), node.location().end_offset()),
        };
        let msg = format!("Do not use `Date.{method}` without zone. Use `Time.zone.{day}` instead.");
        cx.add_fix(s, e, msg, |c| c.replace(ns, ne, "Time.zone"));
    }
}
