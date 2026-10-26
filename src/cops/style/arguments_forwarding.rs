use crate::prelude::*;
use helpers::add_parentheses as parens;
use ruby_prism::Visit;
use std::collections::HashSet;

pub const NAME: &str = "Style/ArgumentsForwarding";
pub static COP: ArgumentsForwarding = ArgumentsForwarding;
pub struct ArgumentsForwarding;

const FORWARDING_MSG: &str = "Use shorthand syntax `...` for arguments forwarding.";
const ARGS_MSG: &str = "Use anonymous positional arguments forwarding (`*`).";
const KWARGS_MSG: &str = "Use anonymous keyword arguments forwarding (`**`).";
const BLOCK_MSG: &str = "Use anonymous block arguments forwarding (`&`).";

type Range = (usize, usize);

fn range(n: &Node) -> Range {
    let l = n.location();
    (l.start_offset(), l.end_offset())
}

/// A parser `any_block` at or around the node (a call owning a `{}`/`do` block is inside it).
fn block_like(node: &Node) -> bool {
    let owns = |b: Option<Node>| b.is_some_and(|b| b.as_block_node().is_some());
    match node {
        Node::BlockNode { .. } | Node::LambdaNode { .. } => true,
        Node::CallNode { .. } => owns(node.as_call_node().unwrap().block()),
        Node::SuperNode { .. } => owns(node.as_super_node().unwrap().block()),
        Node::ForwardingSuperNode { .. } => {
            owns(node.as_forwarding_super_node().unwrap().block().map(|b| b.as_node()))
        }
        _ => false,
    }
}

/// A parser argument of a send / super / yield.
enum Arg<'pr> {
    /// `(splat (lvar name))`, or `forwarded_restarg` with no name
    Splat(Option<&'pr [u8]>),
    /// `hash`: element count, `(kwsplat (lvar name))` elements, and `(hash (forwarded_kwrestarg))`
    Hash { count: usize, kwsplats: Vec<(&'pr [u8], Range)>, anon_only: bool, anon: bool },
    /// `(block_pass (lvar name))` or `(block_pass nil)`
    BlockPass(Option<&'pr [u8]>),
    Other,
}

struct Send<'pr> {
    node: Node<'pr>,
    in_block: bool,
    args: Vec<(Arg<'pr>, Range)>,
    paren: Paren,
    plain_send: bool,
    /// the send owns a literal `{}`/`do` block (`block_literal?`)
    block_literal: bool,
}

/// What add_parens_if_missing does for a send.
enum Paren {
    /// parenthesized, or an operator (no dot) or setter call: nothing
    Skip,
    /// Util#add_parentheses on the node
    Plan,
}

fn lvar_name<'pr>(n: Option<Node<'pr>>) -> Option<Option<&'pr [u8]>> {
    match n {
        None => Some(None),
        Some(e) => e.as_local_variable_read_node().map(|l| Some(l.name().as_slice())),
    }
}

fn classify_arg<'pr>(n: Node<'pr>) -> Arg<'pr> {
    match &n {
        Node::SplatNode { .. } => match lvar_name(n.as_splat_node().unwrap().expression()) {
            Some(name) => Arg::Splat(name),
            None => Arg::Other,
        },
        Node::BlockArgumentNode { .. } => match lvar_name(n.as_block_argument_node().unwrap().expression()) {
            Some(name) => Arg::BlockPass(name),
            None => Arg::Other,
        },
        Node::HashNode { .. } | Node::KeywordHashNode { .. } => {
            let elems: Vec<Node> = match n.as_hash_node() {
                Some(h) => h.elements().iter().collect(),
                None => n.as_keyword_hash_node().unwrap().elements().iter().collect(),
            };
            let mut kwsplats = Vec::new();
            let mut anon = 0;
            for e in &elems {
                if let Some(s) = e.as_assoc_splat_node() {
                    match s.value() {
                        None => anon += 1,
                        Some(v) => {
                            if let Some(l) = v.as_local_variable_read_node() {
                                kwsplats.push((l.name().as_slice(), range(e)));
                            }
                        }
                    }
                }
            }
            Arg::Hash { count: elems.len(), kwsplats, anon_only: elems.len() == 1 && anon == 1, anon: anon > 0 }
        }
        _ => Arg::Other,
    }
}

struct Sends<'pr> {
    outer_block: bool,
    depth: usize,
    found: Vec<Send<'pr>>,
}

impl<'pr> Sends<'pr> {
    fn record(&mut self, node: Node<'pr>, args: Option<ruby_prism::ArgumentsNode<'pr>>, block: Option<Node<'pr>>) {
        let in_block = self.outer_block || self.depth > 0 || block_like(&node);
        let mut list: Vec<Node<'pr>> = args.map(|a| a.arguments().iter().collect()).unwrap_or_default();
        list.extend(block.filter(|b| b.as_block_argument_node().is_some()));
        let args = list
            .into_iter()
            .map(|a| {
                let r = range(&a);
                (classify_arg(a), r)
            })
            .collect();
        let skip = |b: bool| if b { Paren::Skip } else { Paren::Plan };
        let (paren, plain_send) = match &node {
            Node::CallNode { .. } => {
                let c = node.as_call_node().unwrap();
                // parentheses?: `loc.end` is `)`; operator_or_setter_call?
                let p = c.closing_loc().is_some_and(|l| l.as_slice() == b")")
                    || c.is_attribute_write()
                    || (helpers::node_types::is_operator_method(method_name(&c)) && c.call_operator_loc().is_none());
                (skip(p), !is_csend(&c))
            }
            Node::SuperNode { .. } => (skip(node.as_super_node().unwrap().rparen_loc().is_some()), false),
            Node::YieldNode { .. } => (skip(node.as_yield_node().unwrap().rparen_loc().is_some()), false),
            // a `[]=` index target (an operator call) or the `[]` send inside op_asgn / or_asgn / and_asgn
            _ => (Paren::Skip, true),
        };
        let block_literal = matches!(node, Node::CallNode { .. } | Node::SuperNode { .. }) && block_like(&node);
        self.found.push(Send { node, in_block, args, paren, plain_send, block_literal });
    }

    fn nest(&mut self, node: &Node, f: impl FnOnce(&mut Self)) {
        let b = block_like(node);
        self.depth += b as usize;
        f(self);
        self.depth -= b as usize;
    }
}

impl<'pr> Visit<'pr> for Sends<'pr> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'pr>) {
        let n = node.as_node();
        self.record(node.as_node(), node.arguments(), node.block());
        self.nest(&n, |s| ruby_prism::visit_call_node(s, node));
    }
    fn visit_super_node(&mut self, node: &ruby_prism::SuperNode<'pr>) {
        let n = node.as_node();
        self.record(node.as_node(), node.arguments(), node.block());
        self.nest(&n, |s| ruby_prism::visit_super_node(s, node));
    }
    fn visit_yield_node(&mut self, node: &ruby_prism::YieldNode<'pr>) {
        self.record(node.as_node(), node.arguments(), None);
        ruby_prism::visit_yield_node(self, node);
    }
    fn visit_index_operator_write_node(&mut self, node: &ruby_prism::IndexOperatorWriteNode<'pr>) {
        self.record(node.as_node(), node.arguments(), node.block().map(|b| b.as_node()));
        ruby_prism::visit_index_operator_write_node(self, node);
    }
    fn visit_index_or_write_node(&mut self, node: &ruby_prism::IndexOrWriteNode<'pr>) {
        self.record(node.as_node(), node.arguments(), node.block().map(|b| b.as_node()));
        ruby_prism::visit_index_or_write_node(self, node);
    }
    fn visit_index_and_write_node(&mut self, node: &ruby_prism::IndexAndWriteNode<'pr>) {
        self.record(node.as_node(), node.arguments(), node.block().map(|b| b.as_node()));
        ruby_prism::visit_index_and_write_node(self, node);
    }
    fn visit_index_target_node(&mut self, node: &ruby_prism::IndexTargetNode<'pr>) {
        self.record(node.as_node(), node.arguments(), node.block().map(|b| b.as_node()));
        ruby_prism::visit_index_target_node(self, node);
    }
    fn visit_forwarding_super_node(&mut self, node: &ruby_prism::ForwardingSuperNode<'pr>) {
        self.nest(&node.as_node(), |s| ruby_prism::visit_forwarding_super_node(s, node));
    }
    fn visit_block_node(&mut self, node: &ruby_prism::BlockNode<'pr>) {
        self.nest(&node.as_node(), |s| ruby_prism::visit_block_node(s, node));
    }
    fn visit_lambda_node(&mut self, node: &ruby_prism::LambdaNode<'pr>) {
        self.nest(&node.as_node(), |s| ruby_prism::visit_lambda_node(s, node));
    }
}

/// A forwardable def argument: range and name (None when anonymous).
#[derive(Clone, Copy)]
struct Fwd<'pr> {
    range: Range,
    name: Option<&'pr [u8]>,
}

#[derive(Clone, Copy, PartialEq)]
enum Class {
    AllAnonymous,
    All,
    RestOrKwrest,
}

struct Classified {
    send: usize,
    class: Class,
    rest: Option<Range>,
    kwrest: Option<Range>,
    block: Option<Range>,
}

struct DefInfo<'pr> {
    rest: Option<Fwd<'pr>>,
    kwrest: Option<Fwd<'pr>>,
    block: Option<Fwd<'pr>>,
    all_anonymous: bool,
    count: usize,
    optarg: bool,
    kwargs: bool,
}

/// SendNodeClassifier
fn classify(d: &DefInfo, s: &Send, refs: &HashSet<&[u8]>, ruby: f64, allow_only_rest: bool) -> Option<Classified> {
    let referenced = |f: Option<Fwd>| f.and_then(|f| f.name).is_some_and(|n| refs.contains(n));
    let (rest_name, kw_name, block_name) =
        (d.rest.and_then(|f| f.name), d.kwrest.and_then(|f| f.name), d.block.and_then(|f| f.name));
    let rest_idx = if referenced(d.rest) {
        None
    } else {
        s.args.iter().position(|(a, _)| matches!(a, Arg::Splat(Some(n)) if Some(*n) == rest_name))
    };
    let rest = rest_idx.map(|i| s.args[i].1);
    let kwrest_hash = if referenced(d.kwrest) {
        None
    } else {
        s.args.iter().find_map(|(a, _)| match a {
            Arg::Hash { count, kwsplats, .. } => {
                kwsplats.iter().find(|(n, _)| Some(*n) == kw_name).map(|&(_, r)| (r, *count))
            }
            _ => None,
        })
    };
    let kwrest = kwrest_hash.map(|(r, _)| r);
    let block = if referenced(d.block) {
        None
    } else {
        s.args.iter().find_map(|(a, r)| match a {
            Arg::BlockPass(None) => Some(*r),
            Arg::BlockPass(Some(n)) if Some(*n) == block_name => Some(*r),
            _ => None,
        })
    };
    if rest.is_none() && kwrest.is_none() && block.is_none() {
        return None;
    }
    let send_all_anonymous = s.plain_send && {
        let n = s.args.len();
        n >= 3
            && matches!(s.args[n - 3].0, Arg::Splat(None))
            && matches!(s.args[n - 2].0, Arg::Hash { anon_only: true, .. })
            && matches!(s.args[n - 1].0, Arg::BlockPass(None))
    };
    let can_forward_all = || {
        if referenced(d.rest) || referenced(d.kwrest) || referenced(d.block) {
            return false;
        }
        if ruby <= 3.0 && d.optarg {
            return false;
        }
        if ruby >= 3.2 && !(rest.is_some() && kwrest.is_some()) {
            return false;
        }
        let offensive_block =
            !s.block_literal && if d.block.is_some() { block.is_some() } else { !allow_only_rest };
        if !offensive_block {
            return false;
        }
        if d.kwargs || kwrest_hash.is_some_and(|(_, count)| count != 1) {
            return false;
        }
        let forwardable_count = [d.rest.is_some(), d.kwrest.is_some(), d.block.is_some()].iter().filter(|&&b| b).count();
        let missing = (rest_name.is_some() && rest.is_none()) || (kw_name.is_some() && kwrest.is_none());
        let no_additional = !missing && d.count == forwardable_count && s.args.len() == forwardable_count;
        let no_post_splat = match rest_idx {
            None => true,
            Some(i) => matches!(s.args.get(i + 1), None | Some((Arg::Hash { .. } | Arg::BlockPass(_), _))),
        };
        no_additional || (ruby >= 3.0 && no_post_splat)
    };
    let class = if !s.in_block && d.all_anonymous && send_all_anonymous {
        Class::AllAnonymous
    } else if can_forward_all() {
        Class::All
    } else {
        Class::RestOrKwrest
    };
    Some(Classified { send: 0, class, rest, kwrest, block })
}

/// redundant_named_arg: the def argument when its source is the bare keyword or a configured name.
fn redundant_named_arg<'pr>(cx: &Cx, n: Option<&Node<'pr>>, key: &str, keyword: &str) -> Option<Fwd<'pr>> {
    let n = n?;
    let src = node_text(n);
    // `cop_config.fetch(key, []).map`: a String raises (a cop error).
    let names = cx.cfg.array(key);
    if src != keyword && !names.iter().any(|s| format!("{keyword}{s}") == src) {
        return None;
    }
    let name = match n {
        Node::RestParameterNode { .. } => n.as_rest_parameter_node().unwrap().name(),
        Node::KeywordRestParameterNode { .. } => n.as_keyword_rest_parameter_node().unwrap().name(),
        _ => n.as_block_parameter_node().unwrap().name(),
    };
    Some(Fwd { range: range(n), name: name.map(|c| c.as_slice()) })
}

impl Cop for ArgumentsForwarding {
    fn correctable(&self) -> bool {
        true
    }

    fn ruby_range(&self) -> (f64, f64) {
        (2.7, f64::MAX)
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let def = node.as_def_node().unwrap();
        let Some(body) = def.body() else { return };
        let params = def.parameters();
        let args: Vec<Node> = params.as_ref().map(parameter_nodes).unwrap_or_default();

        let fwd = |n, key, keyword| redundant_named_arg(cx, n, key, keyword);
        let restarg = args.iter().find(|a| a.as_rest_parameter_node().is_some());
        let kwrestarg = args.iter().find(|a| a.as_keyword_rest_parameter_node().is_some());
        let blockarg = args.iter().find(|a| a.as_block_parameter_node().is_some());
        let all_anonymous = def.receiver().is_none() && args.len() >= 3 && {
            let n = args.len();
            args[n - 3].as_rest_parameter_node().is_some_and(|r| r.name().is_none())
                && args[n - 2].as_keyword_rest_parameter_node().is_some_and(|r| r.name().is_none())
                && args[n - 1].as_block_parameter_node().is_some_and(|r| r.name().is_none())
        };
        let d = DefInfo {
            rest: fwd(restarg, "RedundantRestArgumentNames", "*"),
            kwrest: fwd(kwrestarg, "RedundantKeywordRestArgumentNames", "**"),
            block: fwd(blockarg, "RedundantBlockArgumentNames", "&"),
            all_anonymous,
            count: args.len(),
            optarg: args.iter().any(|a| a.as_optional_parameter_node().is_some()),
            kwargs: args.iter().any(|a| {
                a.as_required_keyword_parameter_node().is_some() || a.as_optional_keyword_parameter_node().is_some()
            }),
        };

        let mut v = Sends { outer_block: cx.ancestors.iter().any(block_like), depth: 0, found: Vec::new() };
        // each_descendant: the def itself is not a send.
        if let Some(r) = def.receiver() {
            v.visit(&r);
        }
        if let Some(p) = &params {
            v.visit_parameters_node(p);
        }
        v.visit(&body);
        let sends = v.found;

        let refs = helpers::local_references::lvar_lvasgn_names(&body, true);
        let ruby = cx.config.target_ruby;
        let allow_only_rest = cx.cfg.get("AllowOnlyRestArgument").is_none() || cx.cfg.params.truthy("AllowOnlyRestArgument");
        let classes: Vec<Classified> = sends
            .iter()
            .enumerate()
            .filter_map(|(i, s)| {
                classify(&d, s, &refs, ruby, allow_only_rest).map(|mut c| {
                    c.send = i;
                    c
                })
            })
            .collect();
        if classes.is_empty() {
            return;
        }

        let text = cx.src.text;
        // A paren plan with the node it parenthesizes (add_parens_if_missing adds them once per node).
        type Plan = Option<Option<(Range, parens::ParenFix)>>;
        let params_paren = |cx: &Cx| -> Plan {
            // add_parens_if_missing(node.arguments): None = RuboCop would raise.
            if def.lparen_loc().is_some() {
                return Some(None);
            }
            params.as_ref().map(|p| parens::plan(&p.as_node(), cx.src.text).map(|f| (range(&p.as_node()), f)))
        };
        let send_paren = |s: &Send| -> Plan {
            match s.paren {
                Paren::Skip => Some(None),
                Paren::Plan => parens::plan(&s.node, text).map(|f| Some((range(&s.node), f))),
            }
        };
        let explicit_block_name =
            helpers::alignment::enabled_cop_str(cx, "Naming/BlockForwarding", "EnforcedStyle").as_deref() == Some("explicit");
        let allow_in_block = |arg: Option<Range>, s: &Send| arg.is_some() && (ruby >= 3.4 || !s.in_block);

        // register_forward_*_offense: (range, message, paren plan if added, replacement)
        let report = |cx: &mut Cx, r: Range, msg: &str, paren: Plan, with: &str| {
            let Some(paren) = paren else { return false };
            let paren = paren.filter(|(key, _)| cx.state::<HashSet<Range>>().insert(*key)).map(|(_, f)| f);
            let (start, end) = r;
            cx.add_fix(start, end, msg, |c| {
                if let Some(p) = &paren {
                    parens::apply(c, p);
                }
                c.replace(start, end, with);
            });
            true
        };
        // false where RuboCop raises inside the corrector, which ends on_def
        let block_offense = |cx: &mut Cx, add_parens: bool, paren: Plan, arg: Option<Range>| {
            let Some(r) = arg else { return true };
            if ruby <= 3.0 || &text[r.0..r.1] == "&" || explicit_block_name {
                return true;
            }
            report(cx, r, BLOCK_MSG, if add_parens { paren } else { Some(None) }, "&")
        };

        // anonymous_forwarding_elsewhere?: an unclassified send passing `*` or `**` on.
        let elsewhere = sends.iter().enumerate().any(|(i, s)| {
            classes.iter().all(|c| c.send != i)
                && s.args.iter().any(|(a, _)| matches!(a, Arg::Splat(None) | Arg::Hash { anon: true, .. }))
        });
        if classes.iter().all(|c| c.class != Class::RestOrKwrest) && !elsewhere {
            // add_forward_all_offenses
            for c in &classes {
                let s = &sends[c.send];
                if c.rest.is_none() && c.kwrest.is_none() && c.class != Class::AllAnonymous {
                    if allow_in_block(c.block, s) && block_offense(cx, true, params_paren(cx), d.block.map(|f| f.range)) {
                        block_offense(cx, true, send_paren(s), c.block);
                    }
                    return;
                }
                let first = c.rest.or(c.kwrest).or_else(|| {
                    s.args.iter().rev().find(|(a, _)| matches!(a, Arg::Splat(None))).map(|&(_, r)| r)
                });
                // `nil.source_range` raises: the rest of on_def is lost.
                let Some(first) = first else { return };
                let last = s.args.last().unwrap().1;
                if !report(cx, (first.0.min(last.1), first.0.max(last.1)), FORWARDING_MSG, send_paren(s), "...") {
                    return;
                }
            }
            let Some(first) = d.rest.or(d.kwrest).map(|f| f.range) else { return };
            let last = range(args.last().unwrap());
            report(cx, (first.0.min(last.1), first.0.max(last.1)), FORWARDING_MSG, params_paren(cx), "...");
            return;
        }
        if ruby < 3.2 {
            return;
        }
        // add_post_ruby_32_offenses
        if !cx.cfg.params.truthy("UseAnonymousForwarding") {
            return;
        }
        if ruby < 3.4 && classes.iter().any(|c| sends[c.send].in_block) {
            return;
        }
        for c in &classes {
            let s = &sends[c.send];
            let add_parens = c.rest.is_none();
            let maybe = |p: Plan| if add_parens { p } else { Some(None) };
            let ok = (!allow_in_block(c.rest, s)
                || (d.rest.is_none_or(|f| report(cx, f.range, ARGS_MSG, params_paren(cx), "*"))
                    && report(cx, c.rest.unwrap(), ARGS_MSG, send_paren(s), "*")))
                && (!allow_in_block(c.kwrest, s)
                    || (d.kwrest.is_none_or(|f| report(cx, f.range, KWARGS_MSG, maybe(params_paren(cx)), "**"))
                        && report(cx, c.kwrest.unwrap(), KWARGS_MSG, maybe(send_paren(s)), "**")))
                && (!allow_in_block(c.block, s)
                    || (block_offense(cx, add_parens, params_paren(cx), d.block.map(|f| f.range))
                        && block_offense(cx, add_parens, send_paren(s), c.block)));
            if !ok {
                return;
            }
        }
    }
}
