//! rubocop-rspec's shared pieces: `RuboCop::RSpec::Language` (the configurable DSL names under
//! `RSpec: Language`) and its node matchers, the ExampleGroup/Example/Hook concepts and the mixins
//! (TopLevelGroup, InsideExampleGroup, EmptyLineSeparation, FinalEndLocation, CommentsHelp,
//! MoveNode, Metadata, SkipOrPending, Variable).
//!
//! Parser-gem view: a Prism `CallNode` whose `block()` is a `BlockNode` stands for parser's
//! `block` / `numblock` / `itblock` node (the call is its `send_node`); any other `CallNode` is a
//! `send`. Matchers take the node a cop would match in RuboCop in that sense.

use crate::prelude::*;
use ruby_prism::{BlockNode, CallNode, Visit};
use std::sync::Arc;

/// `RSpec: Language` of one config (RuboCop::RSpec::Language.config).
#[derive(Default, Debug)]
pub struct Language {
    pub eg_regular: Vec<String>,
    pub eg_skipped: Vec<String>,
    pub eg_focused: Vec<String>,
    pub ex_regular: Vec<String>,
    pub ex_focused: Vec<String>,
    pub ex_skipped: Vec<String>,
    pub ex_pending: Vec<String>,
    pub expectations: Vec<String>,
    pub helpers: Vec<String>,
    pub hooks: Vec<String>,
    pub error_matchers: Vec<String>,
    pub inc_examples: Vec<String>,
    pub inc_context: Vec<String>,
    pub sg_examples: Vec<String>,
    pub sg_context: Vec<String>,
    pub subjects: Vec<String>,
}

fn has(list: &[String], name: &str) -> bool {
    list.iter().any(|s| s == name)
}

impl Language {
    fn from_map(m: &Map) -> Self {
        // `include?(element.to_s)`: only string entries can match a method name.
        let list = |m: Option<&Map>, k: &str| -> Vec<String> {
            match m.and_then(|m| m.get(k)) {
                Some(Value::Seq(items)) => {
                    items.iter().filter_map(|v| if let Value::Str(s) = v { Some(s.clone()) } else { None }).collect()
                }
                _ => Vec::new(),
            }
        };
        let eg = m.map("ExampleGroups");
        let ex = m.map("Examples");
        let inc = m.map("Includes");
        let sg = m.map("SharedGroups");
        Language {
            eg_regular: list(eg, "Regular"),
            eg_skipped: list(eg, "Skipped"),
            eg_focused: list(eg, "Focused"),
            ex_regular: list(ex, "Regular"),
            ex_focused: list(ex, "Focused"),
            ex_skipped: list(ex, "Skipped"),
            ex_pending: list(ex, "Pending"),
            expectations: list(Some(m), "Expectations"),
            helpers: list(Some(m), "Helpers"),
            hooks: list(Some(m), "Hooks"),
            error_matchers: list(Some(m), "ErrorMatchers"),
            inc_examples: list(inc, "Examples"),
            inc_context: list(inc, "Context"),
            sg_examples: list(sg, "Examples"),
            sg_context: list(sg, "Context"),
            subjects: list(Some(m), "Subjects"),
        }
    }

    pub fn example_group(&self, n: &str) -> bool {
        has(&self.eg_regular, n) || has(&self.eg_skipped, n) || has(&self.eg_focused, n)
    }
    pub fn example(&self, n: &str) -> bool {
        has(&self.ex_regular, n) || has(&self.ex_focused, n) || has(&self.ex_skipped, n) || has(&self.ex_pending, n)
    }
    pub fn expectation(&self, n: &str) -> bool {
        has(&self.expectations, n)
    }
    pub fn helper(&self, n: &str) -> bool {
        has(&self.helpers, n)
    }
    pub fn hook(&self, n: &str) -> bool {
        has(&self.hooks, n)
    }
    pub fn error_matcher(&self, n: &str) -> bool {
        has(&self.error_matchers, n)
    }
    pub fn include(&self, n: &str) -> bool {
        has(&self.inc_examples, n) || has(&self.inc_context, n)
    }
    pub fn shared_group(&self, n: &str) -> bool {
        has(&self.sg_examples, n) || has(&self.sg_context, n)
    }
    pub fn subject(&self, n: &str) -> bool {
        has(&self.subjects, n)
    }
    /// `Language::ALL.all`: any concept, Runners included.
    pub fn any(&self, n: &str) -> bool {
        self.error_matcher(n)
            || self.example_group(n)
            || self.example(n)
            || self.expectation(n)
            || self.helper(n)
            || self.hook(n)
            || self.include(n)
            || is_runner(n)
            || self.shared_group(n)
            || self.subject(n)
    }
}

/// `Runners.all`: `to`, `to_not`, `not_to`.
pub fn is_runner(n: &str) -> bool {
    matches!(n, "to" | "to_not" | "not_to")
}

/// `HookScopes.all`.
pub fn is_hook_scope(n: &str) -> bool {
    matches!(n, "each" | "example" | "context" | "all" | "suite")
}

/// The `RSpec: Language` config the running cop sees (RSpec::Base#on_new_investigation sets it from
/// `config['RSpec']['Language']`). Without rubocop-rspec's department config (only
/// rubocop-rspec_rails loaded, which RuboCop cannot run at all) the gem's defaults apply.
pub fn lang(cx: &Cx) -> Arc<Language> {
    cx.config
        .rspec_language
        .get_or_init(|| {
            static DEFAULT: LazyLock<Map> = LazyLock::new(|| {
                let yml = include_str!("../../config/plugins/rubocop-rspec.yml");
                match crate::config::value::parse_yaml(yml) {
                    Ok(Value::Map(m)) => m.map("RSpec").and_then(|r| r.map("Language")).cloned().unwrap_or_default(),
                    _ => Map::default(),
                }
            });
            let m = cx.config.hash.map("RSpec").and_then(|r| r.map("Language")).unwrap_or(&DEFAULT);
            Arc::new(Language::from_map(m))
        })
        .clone()
}

/// The block a parser `any_block` node has: a `BlockNode` (not `&blk`).
pub fn block_of<'pr>(call: &CallNode<'pr>) -> Option<BlockNode<'pr>> {
    call.block().and_then(|b| b.as_block_node())
}

/// Parser's type for the block of `call`: Some(true) for `block`, Some(false) for numblock/itblock.
pub fn is_plain(block: &BlockNode) -> bool {
    !matches!(block.parameters().map(|p| kind::of(&p)), Some(kind::NUMBERED_PARAMETERS_NODE | kind::IT_PARAMETERS_NODE))
}

/// The call when `node` is parser `(block (send ...))` (a plain block, not numblock/itblock).
pub fn plain_block_call<'pr>(node: &Node<'pr>) -> Option<CallNode<'pr>> {
    let call = node.as_call_node()?;
    block_of(&call).filter(is_plain).map(|_| call)
}

/// The call when `node` is parser `any_block` with a send (a call with a `BlockNode`).
pub fn any_block_call<'pr>(node: &Node<'pr>) -> Option<CallNode<'pr>> {
    let call = node.as_call_node()?;
    block_of(&call).map(|_| call)
}

/// The call when `node` is a parser `send`/`csend` (a call without a `BlockNode`).
pub fn send_call<'pr>(node: &Node<'pr>) -> Option<CallNode<'pr>> {
    let call = node.as_call_node()?;
    block_of(&call).is_none().then_some(call)
}

/// `(send nil? :name ...)`: a receiver-less plain send (no literal block, not `&.`).
pub fn nil_send<'pr>(node: &Node<'pr>, name: &str) -> Option<CallNode<'pr>> {
    send_call(node).filter(|c| c.receiver().is_none() && method_name(c) == name)
}

/// `(send nil? :name)`: as `nil_send`, without arguments.
pub fn bare_send<'pr>(node: &Node<'pr>, name: &str) -> Option<CallNode<'pr>> {
    nil_send(node, name).filter(|c| send_args(c).is_empty())
}

/// `(send _ :name ...)`: a plain send (no literal block, not `&.`) named `name`.
pub fn plain_send<'pr>(node: &Node<'pr>, name: &str) -> Option<CallNode<'pr>> {
    send_call(node).filter(|c| !is_csend(c) && method_name(c) == name)
}

/// Parser's `dsym` (interpolated or empty quoted symbol) rather than `sym`.
pub fn is_dsym(cx: &Cx, node: &Node) -> bool {
    is_parser_dsym(node, cx.src.target_ruby)
}

/// Parser's `sym` node.
pub fn is_sym(cx: &Cx, node: &Node) -> bool {
    node.as_symbol_node().is_some() && !is_dsym(cx, node)
}

/// Parser's `str` node (a non-interpolated string that parser does not make a `dstr`).
pub fn is_str(node: &Node) -> bool {
    node.as_string_node().is_some() && !is_parser_dstr(node)
}

/// Parser's `dstr` node.
pub fn is_dstr(node: &Node) -> bool {
    node.as_interpolated_string_node().is_some() || (node.as_string_node().is_some() && is_parser_dstr(node))
}

/// The value of parser's `str` node, or of the first child of a `dstr` when that is a `str`:
/// `{(str #pred?) (dstr (str #pred?) ...)}`.
pub fn str_or_dstr_head(node: &Node) -> Option<String> {
    if is_str(node) {
        return Some(String::from_utf8_lossy(node.as_string_node().unwrap().unescaped()).into_owned());
    }
    if let Some(s) = node.as_string_node() {
        // A multi-line literal: parser's first `str` child is its first line.
        let v = String::from_utf8_lossy(s.unescaped()).into_owned();
        return Some(match v.find('\n') {
            Some(i) => v[..=i].to_string(),
            None => v,
        });
    }
    let first = node.as_interpolated_string_node()?.parts().iter().next()?;
    let s = first.as_string_node()?;
    let v = String::from_utf8_lossy(s.unescaped()).into_owned();
    Some(match v.find('\n') {
        Some(i) => v[..=i].to_string(),
        None => v,
    })
}

/// rubocop-ast `single_line?` of a parser node: a block compares its `{`/`do` and `}`/`end` lines,
/// anything else its first and last lines.
pub fn single_line(src: &crate::source::Source, node: &Node) -> bool {
    if let Some(b) = node.as_call_node().as_ref().and_then(block_of) {
        return src.line_of(b.opening_loc().start_offset()) == src.line_of(b.closing_loc().start_offset());
    }
    let loc = node.location();
    src.line_of(loc.start_offset()) == src.line_of(loc.end_offset())
}

/// `node.right_sibling` for a statement in a parser `begin` (None otherwise).
pub fn right_sibling<'pr>(ancestors: &[Node<'pr>], node: &Node) -> Option<Node<'pr>> {
    let (items, i) = begin_siblings(ancestors, node)?;
    items.into_iter().nth(i + 1)
}

/// rubocop-ast `DstrNode#value` (and `StrNode#value`): str children's values, other children's
/// source (`#@x` without its `#`).
pub fn dstr_value(node: &Node) -> String {
    fn bytes(node: &Node, out: &mut Vec<u8>) {
        if let Some(s) = node.as_string_node() {
            out.extend(s.unescaped());
            return;
        }
        let Some(i) = node.as_interpolated_string_node() else {
            out.extend(node_text(node).as_bytes());
            return;
        };
        for part in i.parts().iter() {
            match part {
                Node::StringNode { .. } | Node::InterpolatedStringNode { .. } => bytes(&part, out),
                Node::EmbeddedVariableNode { .. } => {
                    out.extend(node_text(&part.as_embedded_variable_node().unwrap().variable()).as_bytes())
                }
                _ => out.extend(node_text(&part).as_bytes()),
            }
        }
    }
    let mut out = Vec::new();
    bytes(node, &mut out);
    String::from_utf8_lossy(&out).into_owned()
}

/// The first parser argument of a `|...|` block parameter list: its range and `name` (`""` when
/// anonymous; None for a destructuring `(a, b)`, which has no name). None without any argument.
pub fn first_block_param(p: &ruby_prism::BlockParametersNode) -> Option<(usize, usize, Option<String>)> {
    let named = |n: &Node, name: Option<ruby_prism::ConstantId>| {
        let (s, e) = node_range(n);
        Some((s, e, Some(name.map(|c| String::from_utf8_lossy(c.as_slice()).into_owned()).unwrap_or_default())))
    };
    if let Some(params) = p.parameters() {
        let first = params
            .requireds()
            .iter()
            .chain(params.optionals().iter())
            .chain(params.rest().filter(|r| r.as_implicit_rest_node().is_none()))
            .chain(params.posts().iter())
            .chain(params.keywords().iter())
            .chain(params.keyword_rest())
            .chain(params.block().map(|b| b.as_node()))
            .next();
        if let Some(n) = first {
            return match &n {
                Node::RequiredParameterNode { .. } => named(&n, Some(n.as_required_parameter_node().unwrap().name())),
                Node::OptionalParameterNode { .. } => named(&n, Some(n.as_optional_parameter_node().unwrap().name())),
                Node::RestParameterNode { .. } => named(&n, n.as_rest_parameter_node().unwrap().name()),
                Node::RequiredKeywordParameterNode { .. } => {
                    named(&n, Some(n.as_required_keyword_parameter_node().unwrap().name()))
                }
                Node::OptionalKeywordParameterNode { .. } => {
                    named(&n, Some(n.as_optional_keyword_parameter_node().unwrap().name()))
                }
                Node::KeywordRestParameterNode { .. } => named(&n, n.as_keyword_rest_parameter_node().unwrap().name()),
                Node::BlockParameterNode { .. } => named(&n, n.as_block_parameter_node().unwrap().name()),
                _ => {
                    let (s, e) = node_range(&n);
                    Some((s, e, None))
                }
            };
        }
    }
    let local = p.locals().iter().next()?;
    named(&local, local.as_block_local_variable_node().map(|l| l.name()))
}

/// The parser node a Prism statement list stands for: its single statement, nil when empty, the
/// list itself (a `begin`) otherwise.
pub fn parser_body<'pr>(stmts: Option<Node<'pr>>) -> Option<Node<'pr>> {
    let node = stmts?;
    match node.as_statements_node() {
        Some(s) => {
            let mut items: Vec<Node> = s.body().iter().collect();
            match items.len() {
                0 => None,
                1 => items.pop(),
                _ => Some(node),
            }
        }
        None => Some(node),
    }
}

/// The children of a parser `begin`: a 2+ statement list or parentheses (`(a; b)`, `(a)`).
pub fn begin_children<'pr>(node: &Node<'pr>) -> Option<Vec<Node<'pr>>> {
    if let Some(s) = node.as_statements_node() {
        return Some(s.body().iter().collect());
    }
    let p = node.as_parentheses_node()?;
    Some(match p.body() {
        None => Vec::new(),
        Some(b) => match b.as_statements_node() {
            Some(s) => s.body().iter().collect(),
            None => vec![b],
        },
    })
}

/// rubocop-ast `branches` of an `if` (IfNode/UnlessNode, elsif chains flattened) or `case`;
/// None for other nodes.
pub fn branches<'pr>(node: &Node<'pr>) -> Option<Vec<Option<Node<'pr>>>> {
    if let Some(i) = node.as_if_node() {
        let mut out = vec![parser_body(i.statements().map(|s| s.as_node()))];
        match i.subsequent() {
            None => {}
            Some(sub) => {
                if let Some(e) = sub.as_else_node() {
                    out.push(parser_body(e.statements().map(|s| s.as_node())));
                } else if sub.as_if_node().is_some() {
                    out.extend(branches(&sub)?);
                }
            }
        }
        return Some(out);
    }
    if let Some(u) = node.as_unless_node() {
        let mut out = vec![parser_body(u.statements().map(|s| s.as_node()))];
        if let Some(e) = u.else_clause() {
            out.push(parser_body(e.statements().map(|s| s.as_node())));
        }
        return Some(out);
    }
    let c = node.as_case_node()?;
    let mut out: Vec<Option<Node>> = c
        .conditions()
        .iter()
        .map(|w| parser_body(w.as_when_node().and_then(|w| w.statements()).map(|s| s.as_node())))
        .collect();
    if let Some(e) = c.else_clause() {
        out.push(parser_body(e.statements().map(|s| s.as_node())));
    }
    Some(out)
}

/// Whether a docstring correction (text built from the literal's value, written back between the
/// first opening and the last closing delimiter) only re-writes what the source already says.
/// RuboCop splices the unescaped value, which can close the literal early or start an
/// interpolation; rbcop withholds the correction otherwise (docs/known-gaps.md). Inert: every
/// string part reads exactly as written (no escapes), and an adjacent-literal concatenation
/// (`'a' \ 'b'`) is made of plain quoted literals opening and closing with the same quote whose
/// text has neither that quote nor (for `"`) `#{`, `#@` or `#$`.
pub fn docstring_inert(node: &Node) -> bool {
    fn as_written(node: &Node) -> bool {
        if let Some(s) = node.as_string_node() {
            return s.unescaped() == s.content_loc().as_slice();
        }
        if let Some(i) = node.as_interpolated_string_node() {
            return i.parts().iter().all(|p| match kind::of(&p) {
                kind::STRING_NODE | kind::INTERPOLATED_STRING_NODE => as_written(&p),
                _ => true,
            });
        }
        true
    }
    /// The literals of a concatenation, flattened.
    fn literals<'pr>(node: Node<'pr>, out: &mut Vec<Node<'pr>>) {
        match node.as_interpolated_string_node() {
            Some(i) if i.opening_loc().is_none() => i.parts().iter().for_each(|p| literals(p, out)),
            _ => out.push(node),
        }
    }
    fn opening<'a>(node: &Node<'a>) -> Option<&'a [u8]> {
        match node.as_interpolated_string_node() {
            Some(i) => i.opening_loc().map(|l| l.as_slice()),
            None => node.as_string_node()?.opening_loc().map(|l| l.as_slice()),
        }
    }
    fn closing<'a>(node: &Node<'a>) -> Option<&'a [u8]> {
        match node.as_interpolated_string_node() {
            Some(i) => i.closing_loc().map(|l| l.as_slice()),
            None => node.as_string_node()?.closing_loc().map(|l| l.as_slice()),
        }
    }
    /// The text as it is spliced, in runs between `#{...}` parts (adjacent parts' values join).
    fn values<'pr>(node: &Node<'pr>, out: &mut Vec<Vec<u8>>) {
        if let Some(s) = node.as_string_node() {
            out.last_mut().unwrap().extend(s.unescaped());
        } else if let Some(i) = node.as_interpolated_string_node() {
            i.parts().iter().for_each(|p| values(&p, out));
        } else if let Some(v) = node.as_embedded_variable_node() {
            out.last_mut().unwrap().extend(node_text(&v.variable()).as_bytes());
        } else {
            out.push(Vec::new());
        }
    }
    let single = match node.as_interpolated_string_node() {
        Some(i) => i.opening_loc().is_some(),
        None => node.as_string_node().is_some_and(|s| s.opening_loc().is_some()),
    };
    if single {
        return as_written(node);
    }
    let mut parts = Vec::new();
    for p in node.as_interpolated_string_node().map(|i| i.parts().iter().collect::<Vec<_>>()).unwrap_or_default() {
        literals(p, &mut parts);
    }
    let Some(q) = parts.first().and_then(opening).filter(|o| *o == b"'" || *o == b"\"") else { return false };
    if !parts.iter().all(|p| matches!(opening(p), Some(b"'" | b"\"")) && as_written(p)) {
        return false;
    }
    if parts.last().and_then(closing) != Some(q) {
        return false;
    }
    let mut vals = vec![Vec::new()];
    parts.iter().for_each(|p| values(p, &mut vals));
    vals.iter().all(|v| {
        !v.contains(&q[0])
            && !v.contains(&b'\\')
            && (q != b"\"" || ![b"#{", b"#@", b"#$"].iter().any(|seq| v.windows(2).any(|w| w == *seq)))
    })
}

/// A parser `dstr` whose first child is a `str` or `dstr`, or has a `sym` in it
/// (`(dstr ({str dstr `sym} ...) ...)`).
pub fn dstr_first_is_str(node: &Node) -> bool {
    if node.as_string_node().is_some() {
        return true;
    }
    let Some(first) = node.as_interpolated_string_node().and_then(|i| i.parts().iter().next()) else { return false };
    if matches!(kind::of(&first), kind::STRING_NODE | kind::INTERPOLATED_STRING_NODE) {
        return true;
    }
    struct V(bool);
    impl<'pr> Visit<'pr> for V {
        fn visit_symbol_node(&mut self, _: &ruby_prism::SymbolNode<'pr>) {
            self.0 = true;
        }
    }
    let mut v = V(false);
    v.visit(&first);
    v.0
}

/// def_node_search `(send nil? #Expectations.all ...)` under `node` (itself included), in order.
pub fn expectations<'pr>(l: &Language, node: &Node<'pr>) -> Vec<CallNode<'pr>> {
    struct V<'a, 'pr> {
        l: &'a Language,
        out: Vec<CallNode<'pr>>,
    }
    impl<'pr> Visit<'pr> for V<'_, 'pr> {
        fn visit_call_node(&mut self, n: &CallNode<'pr>) {
            if n.receiver().is_none() && self.l.expectation(method_name(n)) {
                self.out.push(n.as_node().as_call_node().unwrap());
            }
            ruby_prism::visit_call_node(self, n);
        }
    }
    let mut v = V { l, out: Vec::new() };
    v.visit(node);
    v.out
}

/// RuboCop::RSpec::Hook#scope of a hook call: "each", "context", "suite", or None.
pub fn hook_scope(call: &CallNode) -> Option<&'static str> {
    let Some(arg) = send_args(call).into_iter().next() else { return Some("each") };
    if arg.as_hash_node().is_some() || arg.as_keyword_hash_node().is_some() {
        return Some("each");
    }
    // scope_name: `scope_argument.to_a.first`
    let name: Option<String> = match kind::of(&arg) {
        kind::SYMBOL_NODE => Some(String::from_utf8_lossy(arg.as_symbol_node().unwrap().unescaped()).into_owned()),
        kind::CALL_NODE => {
            let c = arg.as_call_node().unwrap();
            if c.receiver().is_some() || block_of(&c).is_some() {
                return None;
            }
            None
        }
        kind::CONSTANT_READ_NODE => None,
        kind::LOCAL_VARIABLE_READ_NODE => {
            Some(String::from_utf8_lossy(arg.as_local_variable_read_node().unwrap().name().as_slice()).into_owned())
        }
        _ => return None,
    };
    match name.as_deref() {
        None | Some("each" | "example") => Some("each"),
        Some("context" | "all") => Some("context"),
        Some("suite") => Some("suite"),
        _ => None,
    }
}

/// Variable#variable_definition? of a let/subject (block or block-pass send): the name argument,
/// `(send nil? {#Subjects.all #Helpers.all} $({any_sym str dstr} ...) ...)`.
pub fn variable_definition<'pr>(_cx: &Cx, l: &Language, node: &Node<'pr>) -> Option<Node<'pr>> {
    let call = node.as_call_node()?;
    let n = method_name(&call);
    if call.receiver().is_some() || !(l.subject(n) || l.helper(n)) {
        return None;
    }
    let first = send_args(&call).into_iter().next()?;
    matches!(kind::of(&first), kind::SYMBOL_NODE | kind::INTERPOLATED_SYMBOL_NODE | kind::STRING_NODE | kind::INTERPOLATED_STRING_NODE)
        .then_some(first)
}

/// `node.child_nodes` for the parser node a body is, as far as cops look for blocks in it: a
/// `begin`'s statements, a block's (send, then) body, an `if`'s condition and branches.
pub fn child_nodes<'pr>(node: &Node<'pr>) -> Vec<Node<'pr>> {
    if let Some(cs) = begin_children(node) {
        return cs;
    }
    if let Some(c) = any_block_call(node) {
        let mut out: Vec<Node> = c.receiver().into_iter().chain(send_args(&c)).collect();
        out.extend(parser_body(block_body(&c)));
        return out;
    }
    if let Some(i) = node.as_if_node() {
        let mut out = vec![i.predicate()];
        out.extend(parser_body(i.statements().map(|s| s.as_node())));
        match i.subsequent() {
            Some(s) if s.as_else_node().is_some() => {
                out.extend(parser_body(s.as_else_node().unwrap().statements().map(|s| s.as_node())))
            }
            Some(s) => out.push(s),
            None => {}
        }
        return out;
    }
    Vec::new()
}

/// SkipOrPending#skipped_in_metadata?: `{(send _ _ <(sym {:skip :pending}) ...>)
/// (send _ _ ... (hash <(pair (sym {:skip :pending}) { true str dstr }) ...>))}`
pub fn skipped_in_metadata(cx: &Cx, call: &CallNode) -> bool {
    if is_csend(call) {
        return false;
    }
    let args = send_args(call);
    let key = |n: &Node| is_sym(cx, n) && matches!(helpers::rails::sym_value(n).as_deref(), Some("skip" | "pending"));
    if args.iter().any(&key) {
        return true;
    }
    let Some(pairs) = args.last().and_then(helpers::rails::hash_elements) else { return false };
    pairs.iter().filter_map(|p| p.as_assoc_node()).any(|p| {
        let v = p.value();
        key(&p.key()) && (v.as_true_node().is_some() || is_str(&v) || is_dstr(&v))
    })
}

/// SkipOrPending#skip_or_pending_inside_block?: `(block <(send nil? {:skip :pending} ...) ...>)`,
/// a direct child of the block: its own send or its single body statement.
pub fn skip_or_pending_inside_block(node: &Node) -> bool {
    let Some(c) = plain_block_call(node) else { return false };
    if !is_csend(&c) && c.receiver().is_none() && matches!(method_name(&c), "skip" | "pending") {
        return true;
    }
    parser_body(block_body(&c)).is_some_and(|b| {
        send_call(&b).is_some_and(|s| s.receiver().is_none() && matches!(method_name(&s), "skip" | "pending"))
    })
}

/// `Integer(value)` of a config value: an integer, or a string of decimal digits.
pub fn config_integer(v: Option<&Value>) -> Option<i64> {
    match v? {
        Value::Int(i) => Some(*i),
        Value::Str(s) => s.trim().replace('_', "").parse().ok(),
        _ => None,
    }
}

/// A node's byte range.
pub fn node_range(node: &Node) -> (usize, usize) {
    let l = node.location();
    (l.start_offset(), l.end_offset())
}

/// `(const {nil? cbase} :RSpec)`
pub fn is_explicit_rspec(node: &Node) -> bool {
    helpers::parser_parent::is_top_const(node, "RSpec")
}

/// `#rspec?` receiver of a `send` (not `&.`): none, or `RSpec` / `::RSpec`.
pub fn rspec_recv(call: &CallNode) -> bool {
    !is_csend(call) && call.receiver().is_none_or(|r| is_explicit_rspec(&r))
}

/// Body of a block (parser's third child), if any.
pub fn block_body<'pr>(call: &CallNode<'pr>) -> Option<Node<'pr>> {
    block_of(call).and_then(|b| b.body())
}

/// `example_group?`: `(any_block (send #rspec? #ExampleGroups.all ...) ...)`
pub fn example_group(l: &Language, node: &Node) -> bool {
    any_block_call(node).is_some_and(|c| rspec_recv(&c) && l.example_group(method_name(&c)))
}

/// `shared_group?`: `(block (send #rspec? #SharedGroups.all ...) ...)`
pub fn shared_group(l: &Language, node: &Node) -> bool {
    plain_block_call(node).is_some_and(|c| rspec_recv(&c) && l.shared_group(method_name(&c)))
}

/// `spec_group?`: `(any_block (send #rspec? {#SharedGroups.all #ExampleGroups.all} ...) ...)`
pub fn spec_group(l: &Language, node: &Node) -> bool {
    any_block_call(node).is_some_and(|c| {
        let n = method_name(&c);
        rspec_recv(&c) && (l.shared_group(n) || l.example_group(n))
    })
}

/// `example_group_with_body?`: `(block (send #rspec? #ExampleGroups.all ...) args !nil?)`
pub fn example_group_with_body(l: &Language, node: &Node) -> bool {
    plain_block_call(node)
        .is_some_and(|c| rspec_recv(&c) && l.example_group(method_name(&c)) && block_body(&c).is_some())
}

/// `example?`: `(block (send nil? #Examples.all ...) ...)`
pub fn example(l: &Language, node: &Node) -> bool {
    plain_block_call(node).is_some_and(|c| c.receiver().is_none() && l.example(method_name(&c)))
}

/// `hook?`: `(any_block (send nil? #Hooks.all ...) ...)`
pub fn hook(l: &Language, node: &Node) -> bool {
    any_block_call(node).is_some_and(|c| c.receiver().is_none() && l.hook(method_name(&c)))
}

/// `let?`: `{(block (send nil? #Helpers.all ...) ...) (send nil? #Helpers.all _ block_pass)}`
pub fn let_(l: &Language, node: &Node) -> bool {
    let Some(call) = node.as_call_node() else { return false };
    if call.receiver().is_some() || !l.helper(method_name(&call)) {
        return false;
    }
    match call.block() {
        Some(b) if b.as_block_node().is_some() => is_plain(&b.as_block_node().unwrap()),
        Some(_) => call.arguments().is_some_and(|a| a.arguments().iter().count() == 1),
        None => false,
    }
}

/// `include?`: `{(block (send nil? #Includes.all ...) ...) (send nil? #Includes.all ...)}`
pub fn include_(l: &Language, node: &Node) -> bool {
    let Some(call) = node.as_call_node() else { return false };
    if call.receiver().is_some() || !l.include(method_name(&call)) {
        return false;
    }
    block_of(&call).is_none_or(|b| is_plain(&b))
}

/// `subject?`: `(block (send nil? #Subjects.all ...) ...)`
pub fn subject(l: &Language, node: &Node) -> bool {
    plain_block_call(node).is_some_and(|c| c.receiver().is_none() && l.subject(method_name(&c)))
}

/// ExampleGroup#scope_change?: `(block {(send #rspec? {#SharedGroups.all #ExampleGroups.all} ...)
/// (send nil? #Includes.all ...)} ...)`
pub fn scope_change(l: &Language, node: &Node) -> bool {
    plain_block_call(node).is_some_and(|c| {
        let n = method_name(&c);
        (rspec_recv(&c) && (l.shared_group(n) || l.example_group(n))) || (c.receiver().is_none() && l.include(n))
    })
}

/// ExampleGroup#find_all_in_scope: nodes below `group` (not itself) matching `pred`, not looking
/// inside matches, scope changes or examples.
pub fn find_all_in_scope<'pr>(
    l: &Language,
    group: &Node<'pr>,
    pred: fn(&Language, &Node) -> bool,
) -> Vec<Node<'pr>> {
    struct V<'l, 'pr> {
        l: &'l Language,
        pred: fn(&Language, &Node) -> bool,
        out: Vec<Node<'pr>>,
        root: (usize, usize),
    }
    impl<'pr> Visit<'pr> for V<'_, 'pr> {
        fn visit_call_node(&mut self, node: &CallNode<'pr>) {
            let n = node.as_node();
            let loc = n.location();
            if (loc.start_offset(), loc.end_offset()) != self.root {
                if (self.pred)(self.l, &n) {
                    self.out.push(n);
                    return;
                }
                if scope_change(self.l, &n) || example(self.l, &n) {
                    return;
                }
            }
            ruby_prism::visit_call_node(self, node);
        }
    }
    let loc = group.location();
    let mut v = V { l, pred, out: Vec::new(), root: (loc.start_offset(), loc.end_offset()) };
    v.visit(group);
    v.out
}

/// The statements of a parser body: a `begin`'s children, else the body itself (nothing if empty).
pub fn body_children<'pr>(body: Option<Node<'pr>>) -> Vec<Node<'pr>> {
    match body {
        None => Vec::new(),
        Some(b) => match b.as_statements_node() {
            Some(s) => s.body().iter().collect(),
            None => vec![b],
        },
    }
}

/// Whether a block body is parser's `begin` (two or more statements).
pub fn is_begin_body(body: &Option<Node>) -> bool {
    body.as_ref().and_then(|b| b.as_statements_node()).is_some_and(|s| s.body().iter().count() > 1)
}

/// Parser's statement list the node at `ancestors` sits in: (siblings, index) when its parent is a
/// `begin` (2+ statements in a body, or a multi-statement program).
pub fn begin_siblings<'pr>(ancestors: &[Node<'pr>], node: &Node) -> Option<(Vec<Node<'pr>>, usize)> {
    let parent = ancestors.last()?.as_statements_node()?;
    let items: Vec<Node> = parent.body().iter().collect();
    if items.len() < 2 {
        return None;
    }
    let start = node.location().start_offset();
    let i = items.iter().position(|n| n.location().start_offset() == start)?;
    Some((items, i))
}

/// `node.loc.end` of a parser block (`}` / `end`) or send (`)`), if any.
pub fn loc_end<'pr>(node: &Node<'pr>) -> Option<ruby_prism::Location<'pr>> {
    let call = node.as_call_node()?;
    match block_of(&call) {
        Some(b) => Some(b.closing_loc()),
        None => call.closing_loc(),
    }
}

/// FinalEndLocation#final_end_location: `loc.end`, or the heredoc terminator below it (the lowest
/// line wins, `loc.end` on a tie), as a byte range. None where RuboCop raises (no `loc.end`).
pub fn final_end_location(node: &Node) -> Option<(usize, usize)> {
    let end = loc_end(node)?;
    let mut best = (end.start_offset(), end.end_offset());
    let mut best_start = best.0;
    for (s, e) in heredoc_ends(node) {
        if s > best_start {
            // Ranges on later lines start later; a heredoc terminator never shares `end`'s line.
            best = (s, e);
            best_start = s;
        }
    }
    Some(best)
}

/// `loc.heredoc_end` ranges of the heredocs in `node` (terminator line, indentation included,
/// newline excluded), in visit order.
pub fn heredoc_ends(node: &Node) -> Vec<(usize, usize)> {
    struct V(Vec<(usize, usize)>);
    impl V {
        fn closing(&mut self, opening: Option<ruby_prism::Location>, closing: Option<ruby_prism::Location>) {
            if let (Some(o), Some(c)) = (opening, closing)
                && o.as_slice().starts_with(b"<<")
            {
                let s = c.as_slice();
                let trim = if s.ends_with(b"\r\n") {
                    2
                } else {
                    usize::from(s.ends_with(b"\n"))
                };
                self.0.push((c.start_offset(), c.end_offset() - trim));
            }
        }
    }
    impl<'pr> Visit<'pr> for V {
        fn visit_string_node(&mut self, n: &ruby_prism::StringNode<'pr>) {
            self.closing(n.opening_loc(), n.closing_loc());
        }
        fn visit_interpolated_string_node(&mut self, n: &ruby_prism::InterpolatedStringNode<'pr>) {
            self.closing(n.opening_loc(), n.closing_loc());
            ruby_prism::visit_interpolated_string_node(self, n);
        }
        fn visit_x_string_node(&mut self, n: &ruby_prism::XStringNode<'pr>) {
            self.closing(Some(n.opening_loc()), Some(n.closing_loc()));
        }
        fn visit_interpolated_x_string_node(&mut self, n: &ruby_prism::InterpolatedXStringNode<'pr>) {
            self.closing(Some(n.opening_loc()), Some(n.closing_loc()));
            ruby_prism::visit_interpolated_x_string_node(self, n);
        }
    }
    let mut v = V(Vec::new());
    v.visit(node);
    v.0
}

/// String#blank? (RuboCop's core extension): empty or only whitespace / NUL.
pub fn blank(s: &str) -> bool {
    s.bytes().all(|b| b" \t\n\x0b\x0c\r\0".contains(&b))
}

/// EmptyLineSeparation#missing_separating_line: the offending range (the content of the line with
/// the final end, or of the last `rubocop:enable` directive line after it) when the line after
/// `node` and its trailing comment lines is not blank. None also where RuboCop raises.
pub fn missing_separating_line(src: &crate::source::Source, node: &Node) -> Option<(usize, usize)> {
    let final_end_line = src.line_of(final_end_location(node)?.0);
    let mut line = final_end_line;
    let mut enable_line = None;
    while let Some(c) = src.comment_on_line(line + 1) {
        line += 1;
        if crate::directives::parse(src.slice(c.start, c.end)).is_some_and(|d| d.enabled()) {
            enable_line = Some(line);
        }
    }
    // `processed_source[line].blank?` (nil past the last line is blank too)
    if src.lines.get(line).is_none_or(|t| blank(t)) {
        return None;
    }
    Some(offending_loc(src, enable_line.unwrap_or(final_end_line)))
}

/// EmptyLineSeparation#offending_loc: the line's content after its indentation.
fn offending_loc(src: &crate::source::Source, line: usize) -> (usize, usize) {
    let start = src.line_start(line);
    let text = src.lines[line - 1];
    let content = text.trim_start_matches(|c: char| c.is_ascii_whitespace() || c == '\0');
    (start + (text.len() - content.len()), start + text.len())
}

/// EmptyLineSeparation#last_child?: no parser `begin` parent, or the last statement in it.
pub fn last_child(ancestors: &[Node], node: &Node) -> bool {
    match begin_siblings(ancestors, node) {
        None => true,
        Some((items, i)) => i + 1 == items.len(),
    }
}

/// EmptyLineSeparation#missing_separating_line_offense for `node` (the visited node, whose Prism
/// ancestors are `cx.ancestors`).
pub fn separating_line_offense(cx: &mut Cx, node: &Node, msg: impl FnOnce() -> String) {
    if last_child(cx.ancestors, node) {
        return;
    }
    if let Some((s, e)) = missing_separating_line(cx.src, node) {
        cx.add_fix(s, e, msg(), |c| c.insert_after(e, e, "\n"));
    }
}

/// `processed_source.ast_with_comments[node].first`: start of the first comment parser's
/// Comment::Associator gives the node spanning `start..end` (a statement, block or send).
pub fn first_comment(src: &crate::source::Source, program: &Node, start: usize, end: usize) -> Option<usize> {
    let mut a =
        helpers::comment_association::Associator::new(src, helpers::comment_association::skipped_directives(src));
    a.visit(program);
    a.comments_of(start, end).next().map(|c| c.start)
}

/// `buffer.line_range(line)`: the line without its newline (1-based).
pub fn line_range(src: &crate::source::Source, line: usize) -> (usize, usize) {
    let s = src.line_start(line);
    (s, s + src.lines.get(line - 1).map_or(0, |l| l.len()))
}

/// CommentsHelp#source_range_with_comment: from the line of the node's first associated comment
/// (else the node's own first line) to the end of its final end line. None where RuboCop raises.
pub fn range_with_comment(src: &crate::source::Source, program: &Node, node: &Node) -> Option<(usize, usize)> {
    let loc = node.location();
    let first = first_comment(src, program, loc.start_offset(), loc.end_offset()).unwrap_or(loc.start_offset());
    let (end, _) = final_end_location(node)?;
    Some((line_range(src, src.line_of(first)).0, line_range(src, src.line_of(end)).1))
}

/// RuboCop::RSpec::Corrector::MoveNode: the edits that move `node` before or after `other`.
pub struct MoveNode {
    /// (range, text, insert after) of the insertion, then the removed range.
    insert: (usize, usize, String, bool),
    remove: (usize, usize),
}

impl MoveNode {
    /// `move_before(other)` (`after` false) or `move_after(other)`.
    pub fn new(src: &crate::source::Source, program: &Node, node: &Node, other: &Node, after: bool) -> Option<Self> {
        let (s, e) = range_with_comment(src, program, node)?;
        let text = src.slice(s, e);
        let remove = helpers::whole_lines::range_by_whole_lines(src.text, s, e, true);
        let insert = if after {
            let (oe, _) = final_end_location(other)?;
            let (ls, le) = line_range(src, src.line_of(oe));
            (ls, le, format!("\n{text}"), true)
        } else {
            let (ls, le) = line_range(src, src.line_of(other.location().start_offset()));
            (ls, le, format!("{text}\n"), false)
        };
        Some(MoveNode { insert, remove })
    }

    pub fn apply(&self, c: &mut Corrector) {
        let (s, e, ref text, after) = self.insert;
        if after {
            c.insert_after(s, e, text);
        } else {
            c.insert_before(s, e, text);
        }
        c.remove(self.remove.0, self.remove.1);
    }
}

/// InsideExampleGroup#inside_example_group?: the top-level statement holding the node (or the
/// node itself at the top) is a spec group.
pub fn inside_example_group(l: &Language, ancestors: &[Node], node: &Node) -> bool {
    // Prism: [ProgramNode, StatementsNode, <top-level statement>, ...]
    let top = ancestors.get(2).unwrap_or(node);
    spec_group(l, top)
}

/// TopLevelGroup#top_level_nodes of the program: its statements, or the body of a lone
/// class/module (recursively).
pub fn top_level_nodes<'pr>(program: &Node<'pr>) -> Vec<Node<'pr>> {
    fn of<'pr>(node: Option<Node<'pr>>) -> Vec<Node<'pr>> {
        let Some(node) = node else { return Vec::new() };
        if let Some(s) = node.as_statements_node() {
            let items: Vec<Node> = s.body().iter().collect();
            return if items.len() == 1 { of(items.into_iter().next()) } else { items };
        }
        if let Some(m) = node.as_module_node() {
            return of(m.body());
        }
        if let Some(c) = node.as_class_node() {
            return of(c.body());
        }
        vec![node]
    }
    of(program.as_program_node().map(|p| p.statements().as_node()))
}

/// TopLevelGroup#top_level_groups: the top-level nodes that are spec groups.
pub fn top_level_groups<'pr>(l: &Language, program: &Node<'pr>) -> Vec<Node<'pr>> {
    top_level_nodes(program).into_iter().filter(|n| spec_group(l, n)).collect()
}

/// Metadata#on_block for a visited node: the metadata argument lists of `RSpec.configure` hooks
/// (`config.before(:each, ...)`) and of example/group/hook blocks, in RuboCop's order.
pub fn metadata_lists<'pr>(l: &Language, node: &Node<'pr>) -> Vec<(CallNode<'pr>, Vec<Node<'pr>>)> {
    let mut out = Vec::new();
    let Some(call) = plain_block_call(node) else { return out };
    // rspec_configure: (block (send #rspec? :configure) (args (arg $_)) ...)
    if rspec_recv(&call) && method_name(&call) == "configure" && call.arguments().is_none() {
        let block = block_of(&call).unwrap();
        let params = block.parameters().and_then(|p| p.as_block_parameters_node());
        let var = params
            .filter(|p| p.locals().iter().count() == 0)
            .and_then(|p| p.parameters())
            .filter(|p| {
                p.requireds().iter().count() == 1
                    && p.optionals().iter().count() == 0
                    && p.rest().is_none()
                    && p.posts().iter().count() == 0
                    && p.keywords().iter().count() == 0
                    && p.keyword_rest().is_none()
                    && p.block().is_none()
            })
            .and_then(|p| p.requireds().iter().next())
            .and_then(|r| r.as_required_parameter_node())
            .map(|r| r.name().as_slice().to_vec());
        if let Some(var) = var {
            struct V<'a, 'pr> {
                l: &'a Language,
                var: &'a [u8],
                out: &'a mut Vec<(CallNode<'pr>, Vec<Node<'pr>>)>,
            }
            impl<'pr> Visit<'pr> for V<'_, 'pr> {
                fn visit_call_node(&mut self, n: &CallNode<'pr>) {
                    // (send (lvar %) #Hooks.all _ $...)
                    if !is_csend(n)
                        && n.receiver()
                            .and_then(|r| r.as_local_variable_read_node())
                            .is_some_and(|r| r.name().as_slice() == self.var)
                        && self.l.hook(method_name(n))
                    {
                        let args = send_args(n);
                        if !args.is_empty() {
                            self.out.push((n.as_node().as_call_node().unwrap(), args.into_iter().skip(1).collect()));
                        }
                    }
                    ruby_prism::visit_call_node(self, n);
                }
            }
            V { l, var: &var, out: &mut out }.visit(node);
        }
    }
    // rspec_metadata: (block (send #rspec? {#Examples.all #ExampleGroups.all #SharedGroups.all
    // #Hooks.all} _ $...) ...)
    let n = method_name(&call);
    if rspec_recv(&call) && (l.example(n) || l.example_group(n) || l.shared_group(n) || l.hook(n)) {
        let args = send_args(&call);
        if !args.is_empty() {
            out.push((call, args.into_iter().skip(1).collect()));
        }
    }
    out
}

/// Metadata#on_metadata_arguments: (leading arguments, trailing hash).
pub fn split_metadata<'a, 'pr>(args: &'a [Node<'pr>]) -> (&'a [Node<'pr>], Option<&'a Node<'pr>>) {
    match args.last() {
        Some(h) if h.as_hash_node().is_some() || h.as_keyword_hash_node().is_some() => {
            (&args[..args.len() - 1], Some(h))
        }
        _ => (args, None),
    }
}

