//! RuboCop's MethodComplexity mixin with Metrics::Utils::IteratingBlock and RepeatedCsendDiscount,
//! and the CyclomaticComplexity / PerceivedComplexity scores (Metrics/AbcSize uses
//! `helpers::abc_size_calculator`).

use crate::cop::Cx;
use crate::helpers::allowed::{allowed_method, matches_allowed_pattern};
use crate::node_kind as kind;
use ruby_prism::{CallNode, Node, Visit};
use std::collections::HashMap;

/// IteratingBlock::KNOWN_ITERATING_METHODS
const KNOWN_ITERATING_METHODS: &[&str] = &[
    "all?", "any?", "chain", "chunk", "chunk_while", "collect", "collect_concat", "count", "cycle", "detect", "drop",
    "drop_while", "each", "each_cons", "each_entry", "each_slice", "each_with_index", "each_with_object", "entries",
    "filter", "filter_map", "find", "find_all", "find_index", "flat_map", "grep", "grep_v", "group_by", "inject",
    "lazy", "map", "max", "max_by", "min", "min_by", "minmax", "minmax_by", "none?", "one?", "partition", "reduce",
    "reject", "reverse_each", "select", "slice_after", "slice_before", "slice_when", "sort", "sort_by", "sum", "take",
    "take_while", "tally", "to_h", "uniq", "zip", "with_index", "with_object", "bsearch", "bsearch_index", "collect!",
    "combination", "d_permutation", "delete_if", "each_index", "keep_if", "map!", "permutation", "product", "reject!",
    "repeat", "repeated_combination", "select!", "sort!", "each_key", "each_pair", "each_value", "fetch",
    "fetch_values", "has_key?", "merge", "merge!", "transform_keys", "transform_keys!", "transform_values",
    "transform_values!",
];

pub fn iterating_method(name: &[u8]) -> bool {
    KNOWN_ITERATING_METHODS.iter().any(|m| m.as_bytes() == name)
}

/// A parser `block` (not numblock / itblock) attached to a call.
pub fn plain_block<'pr>(call: &CallNode<'pr>) -> Option<ruby_prism::BlockNode<'pr>> {
    let b = call.block()?.as_block_node()?;
    let numbered = b.parameters().is_some_and(|p| {
        matches!(kind::of(&p), kind::NUMBERED_PARAMETERS_NODE | kind::IT_PARAMETERS_NODE)
    });
    (!numbered).then_some(b)
}

/// `iterating_block?(node) == false` is false for blocks and block passes of known iterating
/// methods: the call's block (`block` or `block_pass`) counts.
pub fn iterating_block(call: &CallNode) -> bool {
    iterating_method(call.name().as_slice())
        && (plain_block(call).is_some() || call.block().is_some_and(|b| b.as_block_argument_node().is_some()))
}

/// RepeatedCsendDiscount: `&.` on the same local variable counts once until it is reassigned.
#[derive(Default)]
pub struct CsendDiscount {
    seen: HashMap<Vec<u8>, usize>,
}

impl CsendDiscount {
    /// `discount_for_repeated_csend?` for the csend at offset `at` with this receiver.
    pub fn discount_receiver(&mut self, receiver: Option<&Node>, at: usize) -> bool {
        let Some(name) = receiver.and_then(lvar_name) else { return false };
        match self.seen.get(&name) {
            None => {
                self.seen.insert(name, at);
                false
            }
            Some(&first) => first != at,
        }
    }

    /// `reset_on_lvasgn`
    pub fn reset(&mut self, name: &[u8]) {
        self.seen.remove(name);
    }
}

/// Name of a parser `lvar` node.
pub fn lvar_name(node: &Node) -> Option<Vec<u8>> {
    if let Some(l) = node.as_local_variable_read_node() {
        return Some(l.name().as_slice().to_vec());
    }
    node.as_it_local_variable_read_node().map(|_| b"it".to_vec())
}

/// `csend` view of a call-like node: `a&.b`, `a&.b ||= c`, `a&.b, = c`.
fn csend_receiver_call<'pr>(node: &Node<'pr>) -> Option<bool> {
    let op = match kind::of(node) {
        kind::CALL_OPERATOR_WRITE_NODE => node.as_call_operator_write_node().unwrap().call_operator_loc(),
        kind::CALL_OR_WRITE_NODE => node.as_call_or_write_node().unwrap().call_operator_loc(),
        kind::CALL_AND_WRITE_NODE => node.as_call_and_write_node().unwrap().call_operator_loc(),
        kind::CALL_TARGET_NODE => Some(node.as_call_target_node().unwrap().call_operator_loc()),
        _ => return None,
    };
    Some(op.is_some_and(|l| l.as_slice() == b"&."))
}

/// Receiver of the call-like nodes in `csend_receiver_call`.
fn call_like_receiver<'pr>(node: &Node<'pr>) -> Option<Node<'pr>> {
    match kind::of(node) {
        kind::CALL_OPERATOR_WRITE_NODE => node.as_call_operator_write_node().unwrap().receiver(),
        kind::CALL_OR_WRITE_NODE => node.as_call_or_write_node().unwrap().receiver(),
        kind::CALL_AND_WRITE_NODE => node.as_call_and_write_node().unwrap().receiver(),
        kind::CALL_TARGET_NODE => Some(node.as_call_target_node().unwrap().receiver()),
        _ => None,
    }
}

/// A `def`, or a `define_method(:name) { }` block: (name, body, offense range).
pub fn method_node<'pr>(node: &Node<'pr>) -> Option<(String, Option<Node<'pr>>, (usize, usize))> {
    let loc = node.location();
    let range = (loc.start_offset(), loc.end_offset());
    if let Some(def) = node.as_def_node() {
        return Some((String::from_utf8_lossy(def.name().as_slice()).into_owned(), def.body(), range));
    }
    // (any_block (send nil? :define_method ({sym str} $_)) _ _)
    let call = node.as_call_node()?;
    let block = call.block()?.as_block_node()?;
    if call.receiver().is_some() || call.name().as_slice() != b"define_method" {
        return None;
    }
    let args = crate::ast::call_args(&call);
    if args.len() != 1 {
        return None;
    }
    let name = if let Some(s) = args[0].as_symbol_node() {
        s.unescaped().to_vec()
    } else {
        args[0].as_string_node()?.unescaped().to_vec()
    };
    Some((String::from_utf8_lossy(&name).into_owned(), block.body(), range))
}

/// `cop_config['Max']` (an Integer or a Float). Anything else (`Max: "10"`, `Max: ~`) makes
/// `complexity > max` raise: a cop error at the method.
pub fn max(cx: &Cx) -> f64 {
    match cx.cfg.get("Max") {
        Some(crate::config::value::Value::Int(i)) => *i as f64,
        Some(crate::config::value::Value::Float(f)) => *f,
        _ => panic!("comparison of Integer with String failed"),
    }
}

/// MethodComplexity#on_def / #on_block: the method's name, body and range, unless it is allowed.
pub fn target<'pr>(cx: &Cx, node: &Node<'pr>) -> Option<(String, Node<'pr>, (usize, usize))> {
    let (name, body, range) = method_node(node)?;
    if allowed_method(cx.cfg, &name) || matches_allowed_pattern(cx.cfg, &name) {
        return None;
    }
    Some((name, body?, crate::helpers::lsp::location(node, range)))
}

/// CyclomaticComplexity (`perceived: false`) or PerceivedComplexity score of a method body.
pub fn complexity(body: &Node, perceived: bool) -> i64 {
    let mut v = Counter { score: 1, perceived, csend: CsendDiscount::default(), pattern: 0, guard: None };
    v.visit(body);
    v.score
}

struct Counter {
    score: i64,
    perceived: bool,
    csend: CsendDiscount,
    /// Inside a pattern (`in`, `=>`, `in?`): local variable targets are `match_var`, not `lvasgn`.
    pattern: usize,
    /// Range of the `in pat if guard` IfNode/UnlessNode (parser `if_guard`).
    guard: Option<(usize, usize)>,
}

fn range(node: &Node) -> (usize, usize) {
    let l = node.location();
    (l.start_offset(), l.end_offset())
}

/// rubocop-ast `literal?` (or `const_type?`) for a pattern: PerceivedComplexity's simple `in`.
fn simple_pattern(node: &Node) -> bool {
    matches!(
        kind::of(node),
        kind::STRING_NODE
            | kind::INTERPOLATED_STRING_NODE
            | kind::X_STRING_NODE
            | kind::INTERPOLATED_X_STRING_NODE
            | kind::INTEGER_NODE
            | kind::FLOAT_NODE
            | kind::SYMBOL_NODE
            | kind::INTERPOLATED_SYMBOL_NODE
            | kind::ARRAY_NODE
            | kind::HASH_NODE
            | kind::REGULAR_EXPRESSION_NODE
            | kind::INTERPOLATED_REGULAR_EXPRESSION_NODE
            | kind::TRUE_NODE
            | kind::FALSE_NODE
            | kind::NIL_NODE
            | kind::RANGE_NODE
            | kind::IMAGINARY_NODE
            | kind::RATIONAL_NODE
            | kind::SOURCE_FILE_NODE
            | kind::CONSTANT_READ_NODE
            | kind::CONSTANT_PATH_NODE
    )
}

impl Counter {
    fn csend_score(&mut self, recv: Option<Node>, at: usize) {
        if !self.csend.discount_receiver(recv.as_ref(), at) {
            self.score += 1;
        }
    }

    fn call_like(&mut self, node: &Node) {
        if csend_receiver_call(node) == Some(true) {
            self.csend_score(call_like_receiver(node), node.location().start_offset());
        }
    }
}

impl<'pr> Visit<'pr> for Counter {
    fn visit_if_node(&mut self, node: &ruby_prism::IfNode<'pr>) {
        if self.guard != Some(range(&node.as_node())) {
            let elsif = node.if_keyword_loc().is_some_and(|l| l.as_slice() == b"elsif");
            self.score += if self.perceived && node.subsequent().is_some() && !elsif && node.if_keyword_loc().is_some() { 2 } else { 1 };
        }
        ruby_prism::visit_if_node(self, node);
    }
    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode<'pr>) {
        if self.guard != Some(range(&node.as_node())) {
            self.score += if self.perceived && node.else_clause().is_some() { 2 } else { 1 };
        }
        ruby_prism::visit_unless_node(self, node);
    }
    fn visit_while_node(&mut self, node: &ruby_prism::WhileNode<'pr>) {
        if !node.is_begin_modifier() {
            self.score += 1;
        }
        ruby_prism::visit_while_node(self, node);
    }
    fn visit_until_node(&mut self, node: &ruby_prism::UntilNode<'pr>) {
        if !node.is_begin_modifier() {
            self.score += 1;
        }
        ruby_prism::visit_until_node(self, node);
    }
    fn visit_for_node(&mut self, node: &ruby_prism::ForNode<'pr>) {
        self.score += 1;
        ruby_prism::visit_for_node(self, node);
    }
    fn visit_case_node(&mut self, node: &ruby_prism::CaseNode<'pr>) {
        if self.perceived {
            let nb = node.conditions().iter().count() as f64 + node.else_clause().and_then(|e| e.statements()).map_or(0.0, |_| 1.0);
            self.score += if node.predicate().is_none() { nb as i64 } else { (nb * 0.2 + 0.8).round() as i64 };
        }
        ruby_prism::visit_case_node(self, node);
    }
    fn visit_case_match_node(&mut self, node: &ruby_prism::CaseMatchNode<'pr>) {
        if self.perceived {
            let mut score = 0.0;
            for c in node.conditions().iter() {
                let simple = c.as_in_node().is_some_and(|i| simple_pattern(&i.pattern()));
                score += if simple { 0.2 } else { 1.0 };
            }
            if node.else_clause().and_then(|e| e.statements()).is_some() {
                score += 0.2;
            }
            self.score += f64::round(score) as i64;
        }
        ruby_prism::visit_case_match_node(self, node);
    }
    fn visit_when_node(&mut self, node: &ruby_prism::WhenNode<'pr>) {
        if !self.perceived {
            self.score += 1;
        }
        ruby_prism::visit_when_node(self, node);
    }
    fn visit_in_node(&mut self, node: &ruby_prism::InNode<'pr>) {
        if !self.perceived {
            self.score += 1;
        }
        let pattern = node.pattern();
        let saved = self.guard;
        if matches!(kind::of(&pattern), kind::IF_NODE | kind::UNLESS_NODE) {
            self.guard = Some(range(&pattern));
        }
        self.pattern += 1;
        self.visit(&pattern);
        self.pattern -= 1;
        self.guard = saved;
        if let Some(s) = node.statements() {
            self.visit_statements_node(&s);
        }
    }
    fn visit_match_predicate_node(&mut self, node: &ruby_prism::MatchPredicateNode<'pr>) {
        self.visit(&node.value());
        self.pattern += 1;
        self.visit(&node.pattern());
        self.pattern -= 1;
    }
    fn visit_match_required_node(&mut self, node: &ruby_prism::MatchRequiredNode<'pr>) {
        self.visit(&node.value());
        self.pattern += 1;
        self.visit(&node.pattern());
        self.pattern -= 1;
    }
    fn visit_match_write_node(&mut self, node: &ruby_prism::MatchWriteNode<'pr>) {
        // match_with_lvasgn: no lvasgn children.
        self.visit_call_node(&node.call());
    }
    fn visit_begin_node(&mut self, node: &ruby_prism::BeginNode<'pr>) {
        if node.rescue_clause().is_some() {
            self.score += 1;
        }
        ruby_prism::visit_begin_node(self, node);
    }
    fn visit_rescue_modifier_node(&mut self, node: &ruby_prism::RescueModifierNode<'pr>) {
        self.score += 1;
        ruby_prism::visit_rescue_modifier_node(self, node);
    }
    fn visit_and_node(&mut self, node: &ruby_prism::AndNode<'pr>) {
        self.score += 1;
        ruby_prism::visit_and_node(self, node);
    }
    fn visit_or_node(&mut self, node: &ruby_prism::OrNode<'pr>) {
        self.score += 1;
        ruby_prism::visit_or_node(self, node);
    }
    fn visit_call_node(&mut self, node: &CallNode<'pr>) {
        if iterating_block(node) {
            self.score += 1;
        }
        if crate::ast::is_csend(node) {
            self.csend_score(node.receiver(), node.location().start_offset());
        }
        ruby_prism::visit_call_node(self, node);
    }
    fn visit_call_operator_write_node(&mut self, node: &ruby_prism::CallOperatorWriteNode<'pr>) {
        self.call_like(&node.as_node());
        ruby_prism::visit_call_operator_write_node(self, node);
    }
    fn visit_call_or_write_node(&mut self, node: &ruby_prism::CallOrWriteNode<'pr>) {
        self.score += 1;
        self.call_like(&node.as_node());
        ruby_prism::visit_call_or_write_node(self, node);
    }
    fn visit_call_and_write_node(&mut self, node: &ruby_prism::CallAndWriteNode<'pr>) {
        self.score += 1;
        self.call_like(&node.as_node());
        ruby_prism::visit_call_and_write_node(self, node);
    }
    fn visit_call_target_node(&mut self, node: &ruby_prism::CallTargetNode<'pr>) {
        self.call_like(&node.as_node());
        ruby_prism::visit_call_target_node(self, node);
    }
    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode<'pr>) {
        self.csend.reset(node.name().as_slice());
        ruby_prism::visit_local_variable_write_node(self, node);
    }
    fn visit_local_variable_target_node(&mut self, node: &ruby_prism::LocalVariableTargetNode<'pr>) {
        if self.pattern == 0 {
            self.csend.reset(node.name().as_slice());
        }
    }
    fn visit_local_variable_or_write_node(&mut self, node: &ruby_prism::LocalVariableOrWriteNode<'pr>) {
        self.score += 1;
        self.csend.reset(node.name().as_slice());
        ruby_prism::visit_local_variable_or_write_node(self, node);
    }
    fn visit_local_variable_and_write_node(&mut self, node: &ruby_prism::LocalVariableAndWriteNode<'pr>) {
        self.score += 1;
        self.csend.reset(node.name().as_slice());
        ruby_prism::visit_local_variable_and_write_node(self, node);
    }
    fn visit_local_variable_operator_write_node(&mut self, node: &ruby_prism::LocalVariableOperatorWriteNode<'pr>) {
        self.csend.reset(node.name().as_slice());
        ruby_prism::visit_local_variable_operator_write_node(self, node);
    }
    fn visit_instance_variable_or_write_node(&mut self, node: &ruby_prism::InstanceVariableOrWriteNode<'pr>) {
        self.score += 1;
        ruby_prism::visit_instance_variable_or_write_node(self, node);
    }
    fn visit_instance_variable_and_write_node(&mut self, node: &ruby_prism::InstanceVariableAndWriteNode<'pr>) {
        self.score += 1;
        ruby_prism::visit_instance_variable_and_write_node(self, node);
    }
    fn visit_class_variable_or_write_node(&mut self, node: &ruby_prism::ClassVariableOrWriteNode<'pr>) {
        self.score += 1;
        ruby_prism::visit_class_variable_or_write_node(self, node);
    }
    fn visit_class_variable_and_write_node(&mut self, node: &ruby_prism::ClassVariableAndWriteNode<'pr>) {
        self.score += 1;
        ruby_prism::visit_class_variable_and_write_node(self, node);
    }
    fn visit_global_variable_or_write_node(&mut self, node: &ruby_prism::GlobalVariableOrWriteNode<'pr>) {
        self.score += 1;
        ruby_prism::visit_global_variable_or_write_node(self, node);
    }
    fn visit_global_variable_and_write_node(&mut self, node: &ruby_prism::GlobalVariableAndWriteNode<'pr>) {
        self.score += 1;
        ruby_prism::visit_global_variable_and_write_node(self, node);
    }
    fn visit_constant_or_write_node(&mut self, node: &ruby_prism::ConstantOrWriteNode<'pr>) {
        self.score += 1;
        ruby_prism::visit_constant_or_write_node(self, node);
    }
    fn visit_constant_and_write_node(&mut self, node: &ruby_prism::ConstantAndWriteNode<'pr>) {
        self.score += 1;
        ruby_prism::visit_constant_and_write_node(self, node);
    }
    fn visit_constant_path_or_write_node(&mut self, node: &ruby_prism::ConstantPathOrWriteNode<'pr>) {
        self.score += 1;
        ruby_prism::visit_constant_path_or_write_node(self, node);
    }
    fn visit_constant_path_and_write_node(&mut self, node: &ruby_prism::ConstantPathAndWriteNode<'pr>) {
        self.score += 1;
        ruby_prism::visit_constant_path_and_write_node(self, node);
    }
    fn visit_index_or_write_node(&mut self, node: &ruby_prism::IndexOrWriteNode<'pr>) {
        self.score += 1;
        if node.call_operator_loc().is_some_and(|l| l.as_slice() == b"&.") {
            self.csend_score(node.receiver(), node.location().start_offset());
        }
        ruby_prism::visit_index_or_write_node(self, node);
    }
    fn visit_index_and_write_node(&mut self, node: &ruby_prism::IndexAndWriteNode<'pr>) {
        self.score += 1;
        if node.call_operator_loc().is_some_and(|l| l.as_slice() == b"&.") {
            self.csend_score(node.receiver(), node.location().start_offset());
        }
        ruby_prism::visit_index_and_write_node(self, node);
    }
}

/// MethodComplexity#check_complexity for CyclomaticComplexity / PerceivedComplexity.
pub fn check_score(cx: &mut Cx, node: &Node, perceived: bool, label: &str) {
    let Some((name, body, (start, end))) = target(cx, node) else { return };
    let max = max(cx);
    let score = complexity(&body, perceived);
    if (score as f64) <= max {
        return;
    }
    cx.add(start, end, format!("{label} complexity for `{name}` is too high. [{score}/{}]", max as i64));
}

/// Ruby `format('%.4g', x)` for a positive x: a port of the digit generation of Ruby's dtoa
/// (mode 2, 4 digits). Its floating-point "quick" path decides near-halfway cases half-even and
/// then keeps trailing zeros (`format('%.4g', 260.05)` is `260.0`, `format('%.4g', 173.35)` is
/// `173.4`), so this is not plain correctly rounded `%g`.
pub fn format_g4(x: f64) -> String {
    if x <= 0.0 || !x.is_finite() {
        return format!("{x}");
    }
    let (digits, k) = dtoa4(x);
    let expt = k + 1;
    let ds: String = digits.iter().map(|d| (d + b'0') as char).collect();
    if expt <= -4 || expt > 4 {
        let mant = if ds.len() > 1 { format!("{}.{}", &ds[..1], &ds[1..]) } else { ds };
        let e = expt - 1;
        return format!("{mant}e{}{:02}", if e < 0 { '-' } else { '+' }, e.abs());
    }
    if expt <= 0 {
        return format!("0.{}{ds}", "0".repeat((-expt) as usize));
    }
    let int_len = expt as usize;
    if ds.len() <= int_len {
        format!("{ds}{}", "0".repeat(int_len - ds.len()))
    } else {
        format!("{}.{}", &ds[..int_len], &ds[int_len..])
    }
}

fn strip_zeros(mut v: Vec<u8>) -> Vec<u8> {
    while v.len() > 1 && v.last() == Some(&0) {
        v.pop();
    }
    v
}

/// dtoa's `bump_up`: add one to the last digit, carrying (k grows when all digits were 9).
fn bump_up(mut v: Vec<u8>, k: &mut i32) -> Vec<u8> {
    while let Some(&9) = v.last() {
        v.pop();
    }
    match v.last_mut() {
        Some(d) => *d += 1,
        None => {
            v.push(1);
            *k += 1;
        }
    }
    v
}

/// Up to 4 significant digits of x and the decimal exponent of the first one.
fn dtoa4(x: f64) -> (Vec<u8>, i32) {
    const ILIM: usize = 4;
    let exact = format!("{x:.40e}");
    let (mant, exp) = exact.split_once('e').unwrap();
    let mut k: i32 = exp.parse().unwrap();
    let exact_digits: Vec<u8> = mant.bytes().filter(u8::is_ascii_digit).map(|b| b - b'0').collect();
    // Quick path in floating point.
    let mut d = match k {
        k if k > 0 => x / 10f64.powi(k),
        k if k < 0 => x * 10f64.powi(-k),
        _ => x,
    };
    let eps = (2.0 * d + 7.0) * f64::EPSILON * 10f64.powi(ILIM as i32 - 1);
    let mut digits = Vec::new();
    let mut ilim = ILIM;
    let mut i = 1;
    loop {
        let l = d.trunc();
        d -= l;
        if d == 0.0 {
            ilim = i;
        }
        digits.push(l as u8);
        if i == ilim {
            if d > 0.5 + eps {
                return (bump_up(digits, &mut k), k);
            } else if d < 0.5 - eps {
                return (strip_zeros(digits), k);
            }
            if digits[i - 1] & 1 == 1 {
                return (bump_up(digits, &mut k), k);
            }
            break;
        }
        i += 1;
        d *= 10.0;
    }
    // Exact path after a near-halfway quick path (`half`): an even last digit is never bumped,
    // and the trailing zeros stay when the rest is above one half.
    let digits: Vec<u8> = exact_digits.iter().take(ILIM).copied().collect();
    let rest = &exact_digits[ILIM.min(exact_digits.len())..];
    let above = rest.first().is_some_and(|&r| r > 5 || (r == 5 && rest[1..].iter().any(|&r| r != 0)));
    if above {
        return (digits, k);
    }
    (strip_zeros(digits), k)
}

#[cfg(test)]
mod tests {
    use super::format_g4;

    #[test]
    fn g4() {
        assert_eq!(format_g4(17.0), "17");
        assert_eq!(format_g4(17.03), "17.03");
        assert_eq!(format_g4(123.456), "123.5");
        assert_eq!(format_g4(12345.6), "1.235e+04");
        assert_eq!(format_g4(0.5), "0.5");
        assert_eq!(format_g4(173.35), "173.4");
        assert_eq!(format_g4(260.05), "260.0");
        assert_eq!(format_g4(111.05), "111");
        assert_eq!(format_g4(17.045), "17.04");
        assert_eq!(format_g4(2.0015), "2.002");
        assert_eq!(format_g4(99.995), "100");
        assert_eq!(format_g4(0.00012345), "0.0001234");
        assert_eq!(format_g4(104.55), "104.6");
        assert_eq!(format_g4(9999.5), "1e+04");
        assert_eq!(format_g4(100.0), "100");
    }
}
