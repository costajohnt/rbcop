use crate::prelude::*;

pub const NAME: &str = "Rails/TimeZone";
pub static COP: TimeZone = TimeZone;
pub struct TimeZone;

const GOOD: &[&str] = &["zone", "zone_default", "find_zone", "find_zone!"];
const DANGEROUS: &[&str] = &["now", "local", "new", "parse", "at"];
const ACCEPTED: &[&str] =
    &["in_time_zone", "utc", "getlocal", "xmlschema", "iso8601", "jisx0301", "rfc3339", "httpdate", "to_i", "to_f"];

static SPECIFIER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"([A-Za-z]|[+-][0-9]{2}:?[0-9]{2})\z").unwrap());

fn intersect<'a>(chain: &'a [String], set: &[&str]) -> Vec<&'a str> {
    let mut out: Vec<&str> = Vec::new();
    for c in chain {
        if set.contains(&c.as_str()) && !out.contains(&c.as_str()) {
            out.push(c);
        }
    }
    out
}

/// attach_timezone_specifier?: a node with a `value` whose `to_s` ends in a zone specifier.
fn attach_specifier(n: Option<&Node>) -> bool {
    let Some(n) = n else { return false };
    // `!value.to_s.valid_encoding?` counts as a specifier.
    let v = if let Some(s) = n.as_string_node() {
        match std::str::from_utf8(s.unescaped()) {
            Ok(v) => v.to_string(),
            Err(_) => return true,
        }
    } else if let Some(d) = n.as_interpolated_string_node() {
        let mut out = Vec::new();
        for p in d.parts().iter() {
            match p.as_string_node() {
                Some(s) => out.extend_from_slice(s.unescaped()),
                None => out.extend_from_slice(node_text(&p).as_bytes()),
            }
        }
        match String::from_utf8(out) {
            Ok(v) => v,
            Err(_) => return true,
        }
    } else if let Some(v) = helpers::rails::sym_value(n) {
        v
    } else if n.as_integer_node().is_some() {
        match int_literal_value(n) {
            Some(i) => i.to_string(),
            None => return false,
        }
    } else if let Some(f) = n.as_float_node() {
        helpers::ruby_string::float_to_s(f.value())
    } else {
        return false;
    };
    SPECIFIER.is_match(&v)
}

/// method_from_time_class?: the receiver chain (first children) bottoms out at `Time`/`::Time`.
fn from_time_class(n: &Node) -> bool {
    if let Some(c) = n.as_call_node() {
        return c.receiver().is_some_and(|r| from_time_class(&r));
    }
    if let Some(p) = n.as_constant_path_node() {
        return match p.parent() {
            Some(ns) => from_time_class(&ns),
            None => p.name().is_some_and(|x| x.as_slice() == b"Time"),
        };
    }
    n.as_constant_read_node().is_some_and(|c| c.name().as_slice() == b"Time")
}

/// extract_method_chain from the send at `ancestors[i]` (or the visited node when i == len) up
/// through parent sends.
fn chain_from(anc: &[Node], first: &ruby_prism::CallNode, i: usize) -> (Vec<String>, Vec<(usize, Option<usize>)>) {
    let mut names = Vec::new();
    // (index into anc of each send in the chain, its parser parent index)
    let mut sends: Vec<(usize, Option<usize>)> = Vec::new();
    let mut cur = Some(first.as_node().as_call_node().unwrap());
    let mut idx = i;
    while let Some(c) = cur {
        if is_csend(&c) {
            break;
        }
        if from_time_class(&c.as_node()) {
            names.push(method_name(&c).to_string());
        }
        let p = helpers::parser_parent::parser_parent_index(anc, idx);
        sends.push((idx, p));
        // A send with a block has the block as its parent.
        if c.block().is_some_and(|b| b.as_block_node().is_some()) {
            break;
        }
        cur = p.and_then(|k| anc[k].as_call_node());
        idx = p.unwrap_or(0);
        if p.is_none() {
            break;
        }
    }
    (names, sends)
}

fn replacement(c: &ruby_prism::CallNode) -> &'static str {
    let args = send_args(c);
    match args.first() {
        None => "now",
        Some(a) if helpers::rails::str_value(a).is_some() => "parse",
        Some(_) => "local",
    }
}

impl Cop for TimeZone {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CONSTANT_READ_NODE, kind::CONSTANT_PATH_NODE, kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let strict = cx.cfg.str("EnforcedStyle") == Some("strict");
        let anc = cx.ancestors;
        if let Some(call) = node.as_call_node() {
            // on_send / on_csend: "...".to_time
            let Some(recv) = call.receiver() else { return };
            if method_name(&call) != "to_time" || recv.as_string_node().is_none() || is_parser_dstr(&recv) {
                return;
            }
            if attach_specifier(Some(&recv)) {
                return;
            }
            let sel = call.message_loc().unwrap();
            let (ss, se) = (sel.start_offset(), sel.end_offset());
            let (s, e) = send_range(&call);
            let replacement = format!("Time.zone.parse({})", node_text(&recv));
            let csend = is_csend(&call);
            cx.add_fix(ss, se, "Do not use `String#to_time` without zone. Use `Time.zone.parse` instead.", |c| {
                if !csend {
                    c.replace(s, e, &replacement);
                }
            });
            return;
        }
        if !helpers::parser_parent::is_top_const(node, "Time") {
            return;
        }
        let Some((i, send)) = helpers::rails::receiver_send_parent(anc, node) else { return };
        let args = send_args(&send);
        if attach_specifier(args.first()) {
            return;
        }
        let (chain, sends) = chain_from(anc, &send, i);
        let good: Vec<&str> = if strict { GOOD.to_vec() } else { GOOD.iter().chain(["current"].iter()).chain(ACCEPTED.iter()).copied().collect() };
        if intersect(&chain, DANGEROUS).is_empty() || !intersect(&chain, &good).is_empty() {
            return;
        }
        let name = method_name(&send);
        // offset_option_provided?
        let offset_option = || {
            args.last().is_some_and(|l| {
                helpers::rails::hash_pairs(l).iter().any(|p| {
                    helpers::rails::sym_value(&p.key()).as_deref() == Some("in") && p.value().as_nil_node().is_none()
                })
            })
        };
        let flexible = !strict;
        let localtime = flexible && chain.iter().any(|m| m == "localtime");
        if !localtime {
            let provided = match name {
                "new" => args.len() == 7 || offset_option(),
                "at" | "now" => offset_option(),
                _ => false,
            };
            if provided {
                return;
            }
        } else {
            // check_localtime: the first `localtime` send up the chain must have no arguments.
            let lt = sends.iter().find_map(|&(k, _)| {
                let c = if k == i { Some(send.as_node().as_call_node().unwrap()) } else { anc[k].as_call_node() };
                c.filter(|c| method_name(c) == "localtime")
            });
            match lt {
                Some(c) if send_args(&c).is_empty() => {}
                _ => return,
            }
        }
        let method = intersect(&chain, DANGEROUS).join(".");
        let safe_method = if matches!(method.as_str(), "new" | "current") { replacement(&send).to_string() } else { method.clone() };
        let msg = if localtime {
            "Do not use `Time.localtime` without offset or zone.".to_string()
        } else if flexible {
            let mut acceptable = vec![format!("`Time.zone.{safe_method}`"), "`Time.current`".to_string()];
            acceptable.extend(ACCEPTED.iter().map(|am| format!("`Time.{method}.{am}`")));
            format!("Do not use `Time.{method}` without zone. Use one of {} instead.", acceptable.join(", "))
        } else {
            format!("Do not use `Time.{method}` without zone. Use `Time.zone.{safe_method}` instead.")
        };
        // autocorrect
        let (rs, re) = (node.location().start_offset(), node.location().end_offset());
        let sel = send.message_loc().unwrap();
        let (ss, se) = (sel.start_offset(), sel.end_offset());
        let new_sel = match name {
            "current" => Some("now"),
            "new" => Some(replacement(&send)),
            _ => None,
        };
        // remove_redundant_in_time_zone: `.in_time_zone` sends up the chain (when the chain has
        // in_time_zone or zone).
        let mut removals: Vec<(usize, usize)> = Vec::new();
        if chain.iter().any(|m| m == "in_time_zone" || m == "zone") {
            for &(k, _) in &sends {
                let c = if k == i { send.as_node().as_call_node().unwrap() } else { anc[k].as_call_node().unwrap() };
                if method_name(&c) == "in_time_zone" && send_args(&c).is_empty() {
                    let l = c.message_loc().unwrap();
                    removals.push((l.start_offset() - 1, l.end_offset()));
                }
            }
        }
        cx.add_fix(ss, se, msg, |c| {
            c.insert_after(rs, re, ".zone");
            if let Some(n) = new_sel {
                c.replace(ss, se, n);
            }
            if strict {
                c.replace(rs, re, "Time");
            }
            for &(a, b) in &removals {
                c.remove(a, b);
            }
        });
    }
}
