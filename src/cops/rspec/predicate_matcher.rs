use crate::prelude::*;
use ruby_prism::CallNode;

pub const NAME: &str = "RSpec/PredicateMatcher";
pub static COP: PredicateMatcher = PredicateMatcher;
pub struct PredicateMatcher;

const BUILT_IN_MATCHERS: &[&str] =
    &["be_truthy", "be_falsey", "be_falsy", "have_attributes", "have_received", "be_between", "be_within"];

/// InflectedHelper#to_predicate_matcher
fn to_predicate_matcher(name: &str) -> String {
    match name {
        "is_a?" => "be_a".into(),
        "instance_of?" => "be_an_instance_of".into(),
        "include?" | "respond_to?" => name[..name.len() - 1].into(),
        "exist?" | "exists?" => "exist".into(),
        _ if name.starts_with("has_") => {
            let s = name.replacen("has_", "have_", 1);
            s[..s.len() - 1].to_string()
        }
        _ => format!("be_{}", &name[..name.len() - 1]),
    }
}

/// ExplicitHelper#to_predicate_method
fn to_predicate_method(name: &str) -> String {
    match name {
        "be_a" | "be_an" | "be_a_kind_of" | "a_kind_of" | "be_kind_of" => "is_a?".into(),
        "be_an_instance_of" | "be_instance_of" | "an_instance_of" => "instance_of?".into(),
        "include" => "include?".into(),
        "respond_to" => "respond_to?".into(),
        _ => match (name.strip_prefix("have_"), name.strip_prefix("be_")) {
            (Some(rest), _) if !rest.is_empty() => format!("has_{rest}?"),
            (_, Some(rest)) if !rest.is_empty() => format!("{rest}?"),
            _ => "?".into(),
        },
    }
}

/// LocationHelp.arguments_with_whitespace: from the selector's end to the send's end.
fn arguments_with_whitespace(call: &CallNode) -> (usize, usize) {
    (call.message_loc().map_or(call.location().start_offset(), |l| l.end_offset()), send_range(call).1)
}

/// LocationHelp.block_with_whitespace: from the send's end to the end of its block, if any.
fn block_with_whitespace(call: &CallNode) -> Option<(usize, usize)> {
    let b = helpers::rspec::block_of(call)?;
    Some((send_range(call).1, b.location().end_offset()))
}

impl PredicateMatcher {
    fn inflected(&self, cx: &mut Cx, node: &Node) {
        let to = node.as_call_node().unwrap();
        if is_csend(&to) || !helpers::rspec::is_runner(method_name(&to)) {
            return;
        }
        // (send (send nil? :expect {(block $(send !nil? #predicate? ...) ...) $(send !nil? #predicate? ...)})
        //   $#Runners.all $#boolean_matcher? ...)
        let Some(expect) = to.receiver().and_then(|r| helpers::rspec::nil_send(&r, "expect")) else { return };
        let [actual] = &send_args(&expect)[..] else { return };
        let Some(predicate) = actual.as_call_node() else { return };
        if helpers::rspec::block_of(&predicate).is_some_and(|b| !helpers::rspec::is_plain(&b)) {
            return;
        }
        let pname = method_name(&predicate);
        if is_csend(&predicate) || predicate.receiver().is_none() || !pname.ends_with('?') {
            return;
        }
        let targs = send_args(&to);
        let Some(matcher) = targs.first().and_then(|m| helpers::rspec::send_call(m)).filter(|m| !is_csend(m) && m.receiver().is_none()) else {
            return;
        };
        let margs = send_args(&matcher);
        let mname = method_name(&matcher);
        let strict = cx.cfg.bool("Strict") == Some(true);
        let boolthy = margs.is_empty()
            && matches!(mname, "be_truthy" | "be_falsey" | "be_falsy" | "a_truthy_value" | "a_falsey_value" | "a_falsy_value");
        let be_bool = matches!(mname, "be" | "eq" | "eql" | "equal")
            && matches!(&margs[..], [a] if a.as_true_node().is_some() || a.as_false_node().is_some());
        if !(boolthy || (!strict && be_bool)) {
            return;
        }
        // cannot_replace_predicate?
        if pname == "respond_to?" && send_args(&predicate).len() > 1 {
            return;
        }
        let msg = format!("Prefer using `{}` matcher over `{pname}`.", to_predicate_matcher(pname));
        let src = cx.src;
        let positive = match mname {
            "be" | "eq" | "eql" | "equal" => margs[0].as_true_node().is_some(),
            "be_truthy" | "a_truthy_value" => true,
            _ => false,
        };
        let positive = if method_name(&to) == "to" { positive } else { !positive };
        let Some(dot) = predicate.call_operator_loc() else { return };
        let remove = (dot.start_offset(), send_range(&predicate).1);
        let block = block_with_whitespace(&predicate);
        let (as_, ae) = arguments_with_whitespace(&predicate);
        let replacement = format!(
            "{}{}{}",
            to_predicate_matcher(pname),
            src.slice(as_, ae),
            block.map_or("", |(bs, be)| src.slice(bs, be))
        );
        let sel = to.message_loc().unwrap();
        let (ss, se) = (sel.start_offset(), sel.end_offset());
        let (ms, me) = helpers::rspec::node_range(&matcher.as_node());
        let (s, e) = send_range(&to);
        cx.add_fix(s, e, msg, |c| {
            c.remove(remove.0, remove.1);
            if let Some((bs, be)) = block {
                c.remove(bs, be);
            }
            c.replace(ss, se, if positive { "to" } else { "not_to" });
            c.replace(ms, me, &replacement);
        });
    }

    fn matcher_name_ok(cx: &Cx, name: &str) -> bool {
        let allowed = cx.cfg.strings("AllowedExplicitMatchers");
        if allowed.iter().any(|a| a == name) || BUILT_IN_MATCHERS.contains(&name) {
            return false;
        }
        ((name.starts_with("be_") || name.starts_with("have_")) && !name.ends_with('?')) || matches!(name, "include" | "respond_to")
    }

    fn explicit(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || !helpers::rspec::is_runner(method_name(&call)) {
            return;
        }
        let Some(expect) = call.receiver().and_then(|r| helpers::rspec::nil_send(&r, "expect")) else { return };
        let [actual] = &send_args(&expect)[..] else { return };
        let strict = cx.cfg.bool("Strict") == Some(true);
        let replacement_matcher = match (strict, method_name(&call) == "to") {
            (true, true) => "be(true)",
            (true, false) => "be(false)",
            (false, true) => "be_truthy",
            (false, false) => "be_falsey",
        };
        let src = cx.src;
        let actual_end = actual.location().end_offset();
        let sel = call.message_loc().unwrap();
        let (ss, se) = (sel.start_offset(), sel.end_offset());
        // predicate_matcher_block?: `expect(x).to be_foo do ... end` (the block belongs to `to`)
        if let Some(b) = helpers::rspec::block_of(&call)
            && helpers::rspec::is_plain(&b)
            && let [m] = &send_args(&call)[..]
            && let Some(matcher) = helpers::rspec::send_call(m).filter(|m| m.receiver().is_none() && send_args(m).is_empty())
            && Self::matcher_name_ok(cx, method_name(&matcher))
        {
            let mname = method_name(&matcher).to_string();
            let msg = format!("Prefer using `{}` over `{mname}` matcher.", to_predicate_method(&mname));
            let predicate = to_predicate_method(&mname);
            let (ms, me) = helpers::rspec::node_range(&matcher.as_node());
            let (as_, ae) = arguments_with_whitespace(&matcher);
            let block = block_with_whitespace(&call);
            let insert = format!(".{predicate}{}{}", src.slice(as_, ae), block.map_or("", |(bs, be)| src.slice(bs, be)));
            let (s, e) = helpers::rspec::node_range(node);
            let astart = actual.location().start_offset();
            cx.add_fix(s, e, msg, |c| {
                c.replace(ms, me, replacement_matcher);
                if let Some((bs, be)) = block {
                    c.remove(bs, be);
                }
                c.insert_after(astart, actual_end, &insert);
                c.replace(ss, se, "to");
            });
            // ignore_node(node.children.first): the `to` send gets no offense of its own
            return;
        }
        // predicate_matcher?
        let [m, ..] = &send_args(&call)[..] else { return };
        let Some(matcher) = m.as_call_node().filter(|c| !is_csend(c) && c.receiver().is_none()) else { return };
        if helpers::rspec::block_of(&matcher).is_some_and(|b| !helpers::rspec::is_plain(&b)) {
            return;
        }
        let mname = method_name(&matcher).to_string();
        if !Self::matcher_name_ok(cx, &mname) {
            return;
        }
        let margs = send_args(&matcher);
        if mname == "include" && margs.len() != 1 {
            return;
        }
        let msg = format!("Prefer using `{}` over `{mname}` matcher.", to_predicate_method(&mname));
        let (s, e) = send_range(&call);
        // uncorrectable_matcher?: a heredoc argument not on the expectation's line
        let heredoc = margs.iter().any(is_heredoc);
        let same_line = src.line_of(s) == src.line_of(matcher.location().start_offset());
        if heredoc && !same_line {
            cx.add_fix(s, e, msg, |_| {});
            return;
        }
        let predicate = to_predicate_method(&mname);
        let (ms, me) = send_range(&matcher);
        let (as_, ae) = arguments_with_whitespace(&matcher);
        let block = block_with_whitespace(&matcher);
        let insert = format!(".{predicate}{}{}", src.slice(as_, ae), block.map_or("", |(bs, be)| src.slice(bs, be)));
        let astart = actual.location().start_offset();
        cx.add_fix(s, e, msg, |c| {
            c.replace(ms, me, replacement_matcher);
            if let Some((bs, be)) = block {
                c.remove(bs, be);
            }
            c.insert_after(astart, actual_end, &insert);
            c.replace(ss, se, "to");
        });
    }
}

impl Cop for PredicateMatcher {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        match cx.cfg.str("EnforcedStyle") {
            Some("inflected") => self.inflected(cx, node),
            Some("explicit") => self.explicit(cx, node),
            _ => {}
        }
    }
}
