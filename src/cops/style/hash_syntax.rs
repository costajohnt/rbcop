use crate::helpers::parser_parent::{parser_parent, parser_parent_index as parent_index};
use crate::helpers::range_help::{Side, Surrounding, range_with_surrounding_space};
use crate::prelude::*;

pub const NAME: &str = "Style/HashSyntax";
pub static COP: HashSyntax = HashSyntax;
pub struct HashSyntax;

const MSG_19: &str = "Use the new Ruby 1.9 hash syntax.";
const MSG_NO_MIXED_KEYS: &str = "Don't mix styles in the same hash.";
const MSG_HASH_ROCKETS: &str = "Use hash rockets syntax.";
const OMIT: &str = "Omit the hash value.";
const EXPLICIT: &str = "Include the hash value.";
const MIX_OMIT: &str = "Do not mix explicit and implicit hash values. Omit the hash value.";
const MIX_EXPLICIT: &str = "Do not mix explicit and implicit hash values. Include the hash value.";

static WORD_SYM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\A[_a-z][a-z0-9_]*[?!]?\z").unwrap());
static ALNUM_END: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"[\p{Alphabetic}\p{Nd}"']\z"#).unwrap());
static RUBY19: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^:(.*\S)\s*=>\s*$").unwrap());

/// A parser `pair`.
struct Pair {
    range: (usize, usize),
    key: (usize, usize),
    op: (usize, usize),
    rocket: bool,
    /// key `sym_type?` / `any_sym_type?`
    sym: bool,
    any_sym: bool,
    value: (usize, usize),
    omitted: bool,
    value_sym: bool,
    /// value is a `send` or `lvar`
    value_send_or_lvar: bool,
}

fn pair_of(node: &Node) -> Option<Pair> {
    let a = node.as_assoc_node()?;
    let key = a.key();
    let kl = key.location();
    let (key_r, op, rocket) = match a.operator_loc() {
        Some(op) => ((kl.start_offset(), kl.end_offset()), (op.start_offset(), op.end_offset()), true),
        // `a:` / `"a":`: the label's trailing colon is the operator.
        None => ((kl.start_offset(), kl.end_offset() - 1), (kl.end_offset() - 1, kl.end_offset()), false),
    };
    let value = a.value();
    let (omitted, inner) = match value.as_implicit_node() {
        Some(i) => (true, i.value()),
        None => (false, value),
    };
    let vl = inner.location();
    let value_r = if omitted { key_r } else { (vl.start_offset(), vl.end_offset()) };
    let value_send_or_lvar = inner.as_local_variable_read_node().is_some()
        || inner.as_call_node().is_some_and(|c| !is_csend(&c) && c.block().is_none_or(|b| b.as_block_argument_node().is_some()));
    Some(Pair {
        range: (a.location().start_offset(), a.location().end_offset()),
        key: key_r,
        op,
        rocket,
        sym: key.as_symbol_node().is_some(),
        any_sym: key.as_symbol_node().is_some() || key.as_interpolated_symbol_node().is_some(),
        value: value_r,
        omitted,
        value_sym: !omitted && inner.as_symbol_node().is_some(),
        value_send_or_lvar,
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Style {
    Ruby19,
    HashRockets,
    NoMixedKeys,
    Ruby19NoMixedKeys,
}

/// `parentheses?(node)`: `loc.end` is `)`.
fn parenthesized(node: &Node) -> bool {
    match node {
        Node::CallNode { .. } => node.as_call_node().unwrap().closing_loc().is_some_and(|l| l.as_slice() == b")"),
        Node::SuperNode { .. } => node.as_super_node().unwrap().rparen_loc().is_some(),
        Node::YieldNode { .. } => node.as_yield_node().unwrap().rparen_loc().is_some(),
        Node::ParenthesesNode { .. } => true,
        _ => false,
    }
}

/// `modifier_form?` on if/unless/while/until.
fn modifier_form(node: &Node) -> bool {
    match node {
        Node::IfNode { .. } => {
            let i = node.as_if_node().unwrap();
            i.if_keyword_loc().is_some_and(|k| k.as_slice() == b"if") && i.end_keyword_loc().is_none()
        }
        Node::UnlessNode { .. } => node.as_unless_node().unwrap().end_keyword_loc().is_none(),
        Node::WhileNode { .. } => node.as_while_node().unwrap().closing_loc().is_none(),
        Node::UntilNode { .. } => node.as_until_node().unwrap().closing_loc().is_none(),
        _ => false,
    }
}

/// `right_sibling` is truthy.
// ponytail: covers statement lists, call receivers/arguments and if branches; other parents count
// as having no right sibling.
fn has_right_sibling(anc: &[Node], i: usize) -> bool {
    let (s, e) = (anc[i].location().start_offset(), anc[i].location().end_offset());
    // A call with a `{}`/`do` block is the first child of a parser `block` (its arguments follow).
    let block = match &anc[i] {
        Node::CallNode { .. } => anc[i].as_call_node().unwrap().block(),
        Node::SuperNode { .. } => anc[i].as_super_node().unwrap().block(),
        Node::ForwardingSuperNode { .. } => anc[i].as_forwarding_super_node().unwrap().block().map(|b| b.as_node()),
        _ => None,
    };
    if block.is_some_and(|b| b.as_block_node().is_some()) {
        return true;
    }
    // A Prism StatementsNode with 2+ statements is parser's `begin`.
    if let Some(st) = anc[..i].last().and_then(|n| n.as_statements_node())
        && st.body().iter().count() > 1
    {
        return st.body().iter().any(|n| n.location().start_offset() >= e);
    }
    let Some(p) = parent_index(anc, i) else { return false };
    match &anc[p] {
        Node::CallNode { .. } => {
            let c = anc[p].as_call_node().unwrap();
            if c.receiver().is_some_and(|r| r.location().start_offset() == s) {
                return true;
            }
            send_args(&c).iter().any(|a| a.location().start_offset() >= e)
        }
        Node::IfNode { .. } => {
            let n = anc[p].as_if_node().unwrap();
            let in_cond = n.predicate().location().start_offset() == s;
            in_cond || (n.subsequent().is_some() && n.statements().is_some_and(|b| b.location().start_offset() <= s))
        }
        _ => false,
    }
}

/// HashShorthandSyntax#last_expression?
fn last_expression(anc: &[Node], i: usize) -> bool {
    if has_right_sibling(anc, i) {
        return false;
    }
    let Some(a) = (0..i).rev().find(|&j| is_assignment(&anc[j])) else { return true };
    if let Some(p) = parent_index(anc, a)
        && is_assignment(&anc[p])
    {
        return last_expression(anc, p);
    }
    !has_right_sibling(anc, a)
}

/// HashShorthandSyntax::KEYWORDS: a keyword parameter (`def foo(if:)`) can only be passed as an
/// omitted value, `{ if: }`, since `{ if: if }` is invalid Ruby.
const KEYWORDS: &[&str] = &[
    "__ENCODING__", "__FILE__", "__LINE__", "BEGIN", "END", "alias", "and", "begin", "break", "case", "class",
    "def", "do", "else", "elsif", "end", "ensure", "false", "for", "if", "in", "module", "next", "nil", "not",
    "or", "redo", "rescue", "retry", "return", "self", "super", "then", "true", "undef", "unless", "until",
    "when", "while", "yield",
];

/// keyword_value?: `pair.key.value` is a keyword (only an omitted value can carry one).
fn keyword_value(p: &Pair, src: &str) -> bool {
    p.omitted && KEYWORDS.contains(&&src[p.key.0..p.key.1])
}

struct Ctx<'a> {
    style: Style,
    shorthand: &'a str,
    ruby: f64,
}

impl Cop for HashSyntax {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::HASH_NODE, kind::KEYWORD_HASH_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let elements: Vec<Node> = match node.as_hash_node() {
            Some(h) => h.elements().iter().collect(),
            None => node.as_keyword_hash_node().unwrap().elements().iter().collect(),
        };
        let pairs: Vec<Pair> = elements.iter().filter_map(pair_of).collect();
        if pairs.is_empty() {
            return;
        }
        let braces = node.as_hash_node().is_some();
        let style = match cx.cfg.str("EnforcedStyle") {
            Some("hash_rockets") => Style::HashRockets,
            Some("no_mixed_keys") => Style::NoMixedKeys,
            Some("ruby19_no_mixed_keys") => Style::Ruby19NoMixedKeys,
            _ => Style::Ruby19,
        };
        let force_rockets = cx.cfg.bool("UseHashRocketsWithSymbolValues") == Some(true) && pairs.iter().any(|p| p.value_sym);
        let ctx = Ctx {
            style,
            shorthand: cx.cfg.str("EnforcedShorthandSyntax").unwrap_or("always"),
            ruby: cx.config.target_ruby,
        };
        let rockets_enforced = style == Style::HashRockets || force_rockets;
        let src = cx.src;
        let text = |r: (usize, usize)| src.slice(r.0, r.1);

        // The method dispatch around the hash (find_ancestor_method_dispatch_node).
        let anc = cx.ancestors;
        let dispatch = parent_index(anc, anc.len()).filter(|&d| match &anc[d] {
            Node::CallNode { .. } => !matches!(method_name(&anc[d].as_call_node().unwrap()), "[]" | "[]="),
            Node::SuperNode { .. } | Node::YieldNode { .. } | Node::ForwardingSuperNode { .. } => true,
            _ => false,
        });

        // require_hash_value?
        let around_literal = !braces
            && dispatch.is_some_and(|d| !parenthesized(&anc[d]) && anc[..d].iter().any(modifier_form));
        let require_value = |p: &Pair| {
            if !p.sym || around_literal || !p.value_send_or_lvar {
                return true;
            }
            let key = text(p.key);
            key != text(p.value) || key.ends_with(['!', '?'])
        };

        // Shorthand corrections: replace the pair, maybe parenthesize the call
        // (def_node_that_require_parentheses).
        let last = pairs.last().unwrap();
        let parens = (|| {
            if text(last.key) != text(last.value) {
                return None;
            }
            let d = dispatch?;
            let dn = &anc[d];
            let name = match dn.as_call_node() {
                Some(c) => method_name(&c).to_string(),
                None => String::new(),
            };
            let comparison = matches!(name.as_str(), "==" | "===" | "!=" | "<=" | ">=" | "<" | ">");
            if (!comparison && name.ends_with('=')) || parenthesized(dn) {
                return None;
            }
            if parent_index(anc, d).is_some_and(|p| parenthesized(&anc[p])) {
                return None;
            }
            let requires_context = parent_index(anc, d).is_some_and(|p| {
                matches!(
                    anc[p],
                    Node::CallNode { .. }
                        | Node::IfNode { .. }
                        | Node::UnlessNode { .. }
                        | Node::SuperNode { .. }
                        | Node::UntilNode { .. }
                        | Node::WhileNode { .. }
                        | Node::YieldNode { .. }
                )
            });
            if last_expression(anc, d) && !requires_context {
                return None;
            }
            // DefNode: selector, arguments
            let (sel_end, args): (usize, Vec<Node>) = match dn {
                Node::CallNode { .. } => {
                    let c = dn.as_call_node().unwrap();
                    (c.message_loc().map_or(0, |l| l.end_offset()), send_args(&c))
                }
                Node::SuperNode { .. } => {
                    let s = dn.as_super_node().unwrap();
                    (s.keyword_loc().end_offset(), s.arguments().map(|a| a.arguments().iter().collect()).unwrap_or_default())
                }
                Node::YieldNode { .. } => {
                    let y = dn.as_yield_node().unwrap();
                    (y.keyword_loc().end_offset(), y.arguments().map(|a| a.arguments().iter().collect()).unwrap_or_default())
                }
                _ => return None,
            };
            if args.is_empty() {
                return None;
            }
            let last_arg = args.last().unwrap();
            let hash_last = matches!(last_arg, Node::HashNode { .. } | Node::KeywordHashNode { .. });
            let last_arg_r = (last_arg.location().start_offset(), last_arg.location().end_offset());
            let last_pair_of_last_arg = if hash_last {
                let els: Vec<Node> = match last_arg.as_hash_node() {
                    Some(h) => h.elements().iter().collect(),
                    None => last_arg.as_keyword_hash_node().unwrap().elements().iter().collect(),
                };
                els.iter().rev().find(|e| e.as_assoc_node().is_some()).map(|e| (e.location().start_offset(), e.location().end_offset()))
            } else {
                None
            };
            Some((hash_last, sel_end, args[0].location().start_offset(), last_arg_r, last_pair_of_last_arg))
        })();

        let shorthand_offense = |cx: &mut Cx, p: &Pair, msg: &str, replacement: String| {
            let range = p.range;
            cx.add_fix(p.value.0, p.value.1, msg, |c| {
                c.replace(range.0, range.1, &replacement);
                let Some((hash_last, sel_end, first_arg, last_arg, last_pair)) = parens else { return };
                if !hash_last {
                    c.replace(range.0, range.1, &replacement);
                    return;
                }
                if braces {
                    return;
                }
                c.replace(sel_end, first_arg, "(");
                if last_pair == Some(range) {
                    c.insert_after(last_arg.0, last_arg.1, ")");
                }
            });
        };

        let prefer_rockets_non_alnum = cx.cfg.bool("PreferHashRocketsForNonAlnumEndingSymbols") == Some(true);
        let word_symbol_pair = |p: &Pair| {
            if !p.any_sym {
                return false;
            }
            let k = text(p.key);
            let k = k.strip_prefix(':').unwrap_or(k);
            if prefer_rockets_non_alnum && !ALNUM_END.is_match(k) {
                return false;
            }
            if WORD_SYM.is_match(k) {
                return true;
            }
            if ctx.ruby <= 2.1 {
                return false;
            }
            (k.starts_with('\'') && k.ends_with('\'')) || (k.starts_with('"') && k.ends_with('"'))
        };
        let sym_indices = pairs.iter().all(word_symbol_pair);
        // hash_rockets_enforced? as HashShorthandSyntax sees it: no_mixed_keys styles that will
        // turn the keys into hash rockets enforce them too.
        let shorthand_rockets = rockets_enforced
            || match style {
                Style::NoMixedKeys => !sym_indices || pairs[0].rocket,
                Style::Ruby19NoMixedKeys => !sym_indices,
                _ => false,
            };

        // on_hash_for_mixed_shorthand
        if ctx.ruby > 3.0 && matches!(ctx.shorthand, "consistent" | "either_consistent") && !shorthand_rockets {
            let omitted: Vec<&Pair> = pairs.iter().filter(|p| p.omitted).collect();
            let needed: Vec<&Pair> = pairs.iter().filter(|p| !p.omitted && require_value(p)).collect();
            let omittable: Vec<&Pair> = pairs.iter().filter(|p| !p.omitted && !require_value(p)).collect();
            let kinds = [!omitted.is_empty(), !needed.is_empty(), !omittable.is_empty()].iter().filter(|&&b| b).count();
            if kinds > 1 {
                if !needed.is_empty() {
                    for p in omitted.into_iter().filter(|p| !keyword_value(p, src.text)) {
                        let k = text(p.key);
                        shorthand_offense(cx, p, MIX_EXPLICIT, format!("{k}: {k}"));
                    }
                } else {
                    for p in omittable {
                        shorthand_offense(cx, p, MIX_OMIT, format!("{}:", text(p.key)));
                    }
                }
            } else if needed.is_empty() && !(kinds == 1 && !omittable.is_empty() && ctx.shorthand == "either_consistent") {
                for p in omittable {
                    shorthand_offense(cx, p, OMIT, format!("{}:", text(p.key)));
                }
            }
        }

        // Key style.
        let first_colon = !pairs[0].rocket;
        let (delim_rocket, msg) = if rockets_enforced {
            (false, MSG_HASH_ROCKETS)
        } else {
            match ctx.style {
                Style::Ruby19NoMixedKeys if sym_indices => (true, MSG_19),
                Style::Ruby19NoMixedKeys => (false, MSG_NO_MIXED_KEYS),
                // pairs.first.inverse_delimiter
                Style::NoMixedKeys if sym_indices => (first_colon, MSG_NO_MIXED_KEYS),
                Style::NoMixedKeys => (false, MSG_NO_MIXED_KEYS),
                _ if sym_indices => (true, MSG_19),
                _ => (false, ""),
            }
        };
        if !msg.is_empty() {
            // argument_without_space? / return parent (autocorrect_ruby19)
            let hash_r = (node.location().start_offset(), node.location().end_offset());
            let parent = parser_parent(anc);
            let no_space = parent.and_then(|p| p.as_call_node()).is_some_and(|c| {
                !is_csend(&c)
                    && call_args(&c).iter().any(|a| a.location().start_offset() == hash_r.0)
                    && c.message_loc().is_some_and(|m| m.end_offset() == hash_r.0)
            });
            let in_return = parent.is_some_and(|p| p.as_return_node().is_some()) && !braces;
            let first_pair = pairs[0].range;
            let no_mixed = matches!(ctx.style, Style::NoMixedKeys | Style::Ruby19NoMixedKeys);
            // HashSyntax#alternative_style (nil for no_mixed_keys: no acceptable style).
            let alternative: &[&str] = match ctx.style {
                Style::HashRockets => &["ruby19"],
                Style::Ruby19 | Style::Ruby19NoMixedKeys => &["hash_rockets"],
                Style::NoMixedKeys => &[],
            };
            for p in pairs.iter() {
                if p.rocket != delim_rocket {
                    cx.correct_style_detected();
                    continue;
                }
                if keyword_value(p, src.text) {
                    continue;
                }
                if cx.block_runs(p.key.0, p.op.1) {
                    cx.style_detected(alternative);
                }
                let rockets_fix = rockets_enforced || (no_mixed && !p.rocket);
                let (key, op) = (p.key, p.op);
                let key_src = text(key);
                let fix = if rockets_fix {
                    let mut k = format!(":{key_src} => ");
                    if p.omitted {
                        k.push_str(key_src);
                    }
                    // operator_range: an omitted value keeps the newline after the colon.
                    let op_r = range_with_surrounding_space(
                        src.text,
                        op.0,
                        op.1,
                        Surrounding { newlines: !p.omitted, ..Default::default() },
                    );
                    (k, op_r, None)
                } else {
                    let r = range_with_surrounding_space(
                        src.text,
                        key.0,
                        op.1,
                        Surrounding { side: Side::Right, ..Default::default() },
                    );
                    let space = if no_space { " " } else { "" };
                    let replaced = RUBY19.replace(text(r), format!("{space}${{1}}: ").as_str()).into_owned();
                    let wrap = (in_return && p.range == first_pair).then_some(hash_r);
                    (replaced, r, wrap)
                };
                cx.add_fix(key.0, op.1, msg, |c| {
                    let (replacement, r, wrap) = &fix;
                    if rockets_fix {
                        c.replace(key.0, key.1, replacement);
                        c.remove(r.0, r.1);
                    } else {
                        c.replace(r.0, r.1, replacement);
                        if let Some(h) = wrap {
                            c.wrap(h.0, h.1, "{", "}");
                        }
                    }
                });
            }
        }

        // on_pair (HashShorthandSyntax)
        if ctx.ruby > 3.0 && matches!(ctx.shorthand, "always" | "never") && !shorthand_rockets {
            for p in &pairs {
                let k = text(p.key);
                if ctx.shorthand == "always" {
                    if p.omitted || require_value(p) {
                        continue;
                    }
                    cx.no_acceptable_style();
                    shorthand_offense(cx, p, OMIT, format!("{k}:"));
                } else if p.omitted && !keyword_value(p, src.text) {
                    shorthand_offense(cx, p, EXPLICIT, format!("{k}: {k}"));
                }
            }
        }
    }
}
