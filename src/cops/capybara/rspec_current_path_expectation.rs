use crate::prelude::*;
use helpers::range_help::{Side, Surrounding, range_with_surrounding_space};
use ruby_prism::CallNode;

pub const NAME: &str = "Capybara/RSpec/CurrentPathExpectation";
pub static COP: CurrentPathExpectation = CurrentPathExpectation;
pub struct CurrentPathExpectation;

const MSG: &str = "Do not set an RSpec expectation on `current_path` in Capybara feature specs - instead, use the `have_current_path` matcher on `page`";

/// `(send nil? name)` with no arguments and no block.
fn bare(node: &Node, name: &str) -> bool {
    node.as_call_node().is_some_and(|c| {
        !is_csend(&c)
            && method_name(&c) == name
            && send_args(&c).is_empty()
            && helpers::rspec::block_of(&c).is_none()
            && (name == "page" && c.receiver().is_none() || name == "current_path" && c.receiver().is_none_or(|r| bare(&r, "page")))
    })
}

/// `(send nil? name ...)` without a block.
fn matcher<'pr>(node: &Node<'pr>, names: &[&str]) -> Option<CallNode<'pr>> {
    let c = node.as_call_node()?;
    (!is_csend(&c) && c.receiver().is_none() && names.contains(&method_name(&c)) && helpers::rspec::block_of(&c).is_none()).then_some(c)
}

/// `Regexp.new(value).inspect` for a value whose regexp is always valid and whose inspect is the
/// value with `/` escaped: printable ASCII without backslashes, interpolation or grouping/quantifier
/// characters. Other values are withheld (docs/known-gaps.md).
fn regexp_literal(value: &str) -> Option<String> {
    let ok = value.bytes().all(|b| (0x20..0x7f).contains(&b) && !b"\\#()[]{}*+?|".contains(&b));
    ok.then(|| format!("/{}/", value.replace('/', "\\/")))
}

impl Cop for CurrentPathExpectation {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send nil? :expect (send {(send nil? :page) nil?} :current_path))
        let call = node.as_call_node().unwrap();
        if call.receiver().is_some() || method_name(&call) != "expect" {
            return;
        }
        let [arg] = &send_args(&call)[..] else { return };
        if !bare(arg, "current_path") {
            return;
        }
        let sel = call.message_loc().unwrap();
        let (s, e) = (sel.start_offset(), sel.end_offset());
        // node.chained?: the receiver of the parent call
        let parent = helpers::rspec::block_of(&call)
            .is_none()
            .then(|| helpers::parser_parent::parser_parent(cx.ancestors).and_then(|p| p.as_call_node()))
            .flatten()
            .filter(|p| p.receiver().is_some_and(|r| r.location().start_offset() == node.location().start_offset()));
        let Some(parent) = parent else { return cx.add_fix(s, e, MSG, |_| {}) };
        // as_is_matcher / regexp_node_matcher
        let to = method_name(&parent);
        let pargs = send_args(&parent);
        let m = match &pargs[..] {
            [m] if !is_csend(&parent) && matches!(to, "to" | "to_not" | "not_to") => matcher(m, &["eq", "match"]),
            _ => None,
        };
        let Some(m) = m else { return cx.add_fix(s, e, MSG, |_| {}) };
        let margs = send_args(&m);
        let is_match = method_name(&m) == "match";
        let regexp = |n: &Node| n.as_regular_expression_node().is_some() || n.as_interpolated_regular_expression_node().is_some();
        let any_str = |n: &Node| {
            n.as_string_node().is_some()
                || n.as_interpolated_string_node().is_some()
                || n.as_x_string_node().is_some()
                || n.as_interpolated_x_string_node().is_some()
        };
        let from_str = match &margs[..] {
            _ if !is_match => None,
            [r] if regexp(r) => None,
            [r] if any_str(r) => Some(r),
            _ => return cx.add_fix(s, e, MSG, |_| {}),
        };
        let Some(first) = margs.first() else {
            // matcher_node.first_argument is nil: RuboCop raises
            return crate::runner::cop_error(NAME, cx.src, node.location().start_offset());
        };
        // convert_regexp_node_to_literal: the string's value as a regexp literal
        let mut literal = None;
        if let Some(r) = &from_str {
            let value = if let Some(x) = r.as_x_string_node() {
                // "/\#{`#{value}`}/": only a plain command is written
                let v = String::from_utf8_lossy(x.unescaped()).into_owned();
                v.bytes().all(|b| (0x20..0x7f).contains(&b) && !b"\\#`".contains(&b)).then(|| format!("/#{{`{v}`}}/"))
            } else if r.as_interpolated_x_string_node().is_some() {
                None
            } else if let Some(sn) = r.as_string_node() {
                regexp_literal(&String::from_utf8_lossy(sn.unescaped()))
            } else {
                // a dstr: its parts' values (interpolations as source) joined
                let parts = r.as_interpolated_string_node().unwrap().parts();
                let joined: Option<String> =
                    parts.iter().map(|p| p.as_string_node().map(|s| String::from_utf8_lossy(s.unescaped()).into_owned())).collect();
                joined.as_deref().and_then(regexp_literal)
            };
            match value {
                Some(v) => literal = Some((helpers::rspec::node_range(r), v)),
                None => return cx.add_withheld_fix(s, e, MSG),
            }
        }
        let (as_, ae) = helpers::rspec::node_range(arg);
        let psel = parent.message_loc().unwrap();
        let msel = m.message_loc().unwrap();
        let matcher_method = if to == "to" { "have_current_path" } else { "have_no_current_path" };
        // add_argument_parentheses: a first argument that is a call with unparenthesized arguments
        let parens = first.as_call_node().filter(|c| !is_csend(c) && c.opening_loc().is_none()).and_then(|c| {
            let args = send_args(&c);
            let (fs, fe) = helpers::rspec::node_range(args.first()?);
            let (ls, le) = helpers::rspec::node_range(args.last()?);
            let opts = Surrounding { side: Side::Left, ..Default::default() };
            let (b, _) = range_with_surrounding_space(cx.src.text, fs, fe, opts);
            Some((b, ls, le))
        });
        let last = margs.last().map(helpers::rspec::node_range);
        cx.add_fix(s, e, MSG, |c| {
            c.replace(as_, ae, "page");
            c.replace(psel.start_offset(), psel.end_offset(), "to");
            c.replace(msel.start_offset(), msel.end_offset(), matcher_method);
            if let Some((b, ls, le)) = parens {
                c.insert_before(b, b, "(");
                c.insert_after(ls, le, ")");
            }
            if !is_match && let Some((ls, le)) = last {
                c.insert_after(ls, le, ", ignore_query: true");
            }
            if let Some(((rs, re), v)) = &literal {
                c.replace(*rs, *re, v);
            }
        });
    }
}
