use crate::prelude::*;
use helpers::select_by::plain_send;
use crate::source::Source;
use ruby_prism::CallNode;
use std::collections::{HashMap, HashSet};

pub const NAME: &str = "Layout/ClassStructure";
pub static COP: ClassStructure = ClassStructure;
pub struct ClassStructure;

const VISIBILITY_SCOPES: [&str; 3] = ["private", "protected", "public"];

struct Conf {
    order: Order,
    categories: Vec<(String, Vec<String>)>,
}

/// `cop_config['ExpectedOrder']`, read bare: on a String, `index`/`include?` find substrings
/// (character offsets) and `expected_order[i]` is one character.
enum Order {
    List(Vec<String>),
    Str(String),
}

impl Conf {
    fn index(&self, c: &str) -> Option<usize> {
        match &self.order {
            Order::List(order) => order.iter().position(|o| o == c),
            Order::Str(s) => s.find(c).map(|b| s[..b].chars().count()),
        }
    }

    /// `expected_order[i]`
    fn at(&self, i: usize) -> String {
        match &self.order {
            Order::List(order) => order[i].clone(),
            Order::Str(s) => s.chars().nth(i).map(String::from).unwrap_or_default(),
        }
    }

    fn find_category(&self, name: &str) -> Option<&str> {
        self.categories.iter().find(|(_, names)| names.iter().any(|n| n == name)).map(|(c, _)| c.as_str())
    }
}

/// The scope name of a receiver-less `private`/`protected`/`public` send.
fn visibility_scope(call: &CallNode) -> Option<&'static str> {
    let name = method_name(call);
    VISIBILITY_SCOPES.into_iter().find(|v| *v == name).filter(|_| call.receiver().is_none())
}

/// `(send nil? VISIBILITY_SCOPES)`.
fn visibility_block(node: &Node) -> Option<&'static str> {
    plain_send(node).filter(|c| send_args(c).is_empty()).and_then(|c| visibility_scope(&c))
}

/// `(send nil? VISIBILITY_SCOPES (sym _))`: the scope and the symbol.
fn visibility_inline_on_method_name(node: &Node) -> Option<(&'static str, Vec<u8>)> {
    let call = plain_send(node)?;
    let args = send_args(&call);
    let sym = args.first().filter(|_| args.len() == 1)?.as_symbol_node()?;
    Some((visibility_scope(&call)?, sym.unescaped().to_vec()))
}

/// MethodDispatchNode#def_modifier?
fn def_modifier(call: &CallNode) -> bool {
    if call.receiver().is_some() {
        return false;
    }
    match send_args(call).into_iter().next() {
        Some(Node::DefNode { .. }) => true,
        Some(arg) => plain_send(&arg).is_some_and(|c| def_modifier(&c)),
        None => false,
    }
}

fn is_kwbegin(node: &Node) -> bool {
    node.as_begin_node().is_some_and(|b| b.begin_keyword_loc().is_some())
}

fn is_casgn(node: &Node) -> bool {
    matches!(node, Node::ConstantWriteNode { .. } | Node::ConstantPathWriteNode { .. })
}

/// A namespace-less `casgn` whose value is a send other than `<literal>.freeze`.
fn dynamic_constant(node: &Node) -> bool {
    let Some(casgn) = node.as_constant_write_node() else { return false };
    plain_send(&casgn.value()).is_some_and(|c| {
        !(method_name(&c) == "freeze"
            && c.receiver().is_some_and(|r| helpers::literal_node::recursive_basic_literal(&r)))
    })
}

/// One parser sibling list (a class body `begin`, a `kwbegin`, or a lone body statement) with the
/// per-element facts the cop needs, computed once so the work stays linear in the body size.
struct Body<'pr> {
    sibs: Vec<Node<'pr>>,
    class: Vec<String>,
    /// `expected_order` index, None when `ignore?`.
    rank: Vec<Option<usize>>,
    dynamic: Vec<bool>,
    dependent: Vec<bool>,
    /// Index of the last dynamic constant / bare visibility send before each element.
    last_dynamic: Vec<Option<usize>>,
    last_vblock: Vec<Option<usize>>,
    anchors: HashMap<(usize, usize), Option<usize>>,
}

impl<'pr> Body<'pr> {
    fn new(conf: &Conf, sibs: Vec<Node<'pr>>) -> Self {
        let n = sibs.len();
        // node_visibility: an inline `private :name` among the right siblings (the last one wins),
        // else the last bare visibility send among the left siblings, else public.
        let mut vis = vec!["public"; n];
        let mut last_dynamic = vec![None; n];
        let mut last_vblock = vec![None; n];
        let (mut d, mut v, mut scope) = (None, None, "public");
        for i in 0..n {
            last_dynamic[i] = d;
            last_vblock[i] = v;
            vis[i] = scope;
            if dynamic_constant(&sibs[i]) {
                d = Some(i);
            }
            if let Some(s) = visibility_block(&sibs[i]) {
                v = Some(i);
                scope = s;
            }
        }
        let mut inline: HashMap<Vec<u8>, &'static str> = HashMap::new();
        for i in (0..n).rev() {
            if let Some(def) = sibs[i].as_def_node()
                && let Some(s) = inline.get(def.name().as_slice())
            {
                vis[i] = s;
            }
            if let Some((s, name)) = visibility_inline_on_method_name(&sibs[i]) {
                inline.entry(name).or_insert(s);
            }
        }
        // private_constant?: `parent.each_child_node(:send)` with a matching symbol argument.
        let private: HashSet<Vec<u8>> = sibs
            .iter()
            .filter_map(plain_send)
            .filter(|c| method_name(c) == "private_constant")
            .flat_map(|c| send_args(&c).into_iter().filter_map(|a| a.as_symbol_node().map(|s| s.unescaped().to_vec())))
            .collect();
        let class: Vec<String> = (0..n).map(|i| classify(conf, &sibs[i], vis[i])).collect();
        let rank = (0..n)
            .map(|i| {
                let c = &class[i];
                let private_const =
                    sibs[i].as_constant_write_node().is_some_and(|w| private.contains(w.name().as_slice()));
                if c.ends_with('=') || private_const { None } else { conf.index(c) }
            })
            .collect();
        let dynamic = sibs.iter().map(dynamic_constant).collect();
        let dependent = sibs.iter().map(|s| visibility_dependent(conf, s)).collect();
        Body { sibs, class, rank, dynamic, dependent, last_dynamic, last_vblock, anchors: HashMap::new() }
    }

    /// insertion_anchor: the first left sibling after the last barrier whose category is expected
    /// after the node's. Memoised per (span start, rank): a span start is shared by every element
    /// up to the next barrier, so the scans stay linear per rank.
    fn insertion_anchor(&mut self, i: usize) -> Option<usize> {
        let index = self.rank[i]?;
        let barrier = if self.dependent[i] { self.last_dynamic[i].max(self.last_vblock[i]) } else { self.last_dynamic[i] };
        let from = barrier.map_or(0, |b| b + 1);
        let rank = &self.rank;
        let first = *self
            .anchors
            .entry((from, index))
            .or_insert_with(|| (from..rank.len()).find(|&j| rank[j].is_some_and(|x| x > index)));
        first.filter(|&j| j < i)
    }

    /// movable_group: the node and the contiguous same-category movable right siblings.
    fn movable_group(&self, i: usize) -> Vec<usize> {
        let mut group = vec![i];
        for j in i + 1..self.sibs.len() {
            if self.class[j] != self.class[i] || self.rank[j].is_none() || self.dynamic[j] {
                break;
            }
            group.push(j);
        }
        group
    }
}

/// classify (`vis` is the element's node_visibility).
fn classify(conf: &Conf, node: &Node, vis: &str) -> String {
    if let Some(call) = node.as_call_node() {
        if let Some(Node::BlockNode { .. }) = call.block() {
            let block = call.block().unwrap().as_block_node().unwrap();
            return match block.parameters() {
                Some(Node::NumberedParametersNode { .. }) => "numblock".into(),
                Some(Node::ItParametersNode { .. }) => "itblock".into(),
                _ if is_csend(&call) => "csend".into(),
                // classify(node.send_node): the send has no siblings inside the block.
                _ => send_category(conf, &call, "public"),
            };
        }
        if !is_csend(&call) {
            return send_category(conf, &call, vis);
        }
    }
    let name: String = match node {
        Node::DefNode { .. } => {
            let def = node.as_def_node().unwrap();
            if def.receiver().is_some() {
                "public_class_methods".into()
            } else if def.name().as_slice() == b"initialize" {
                "initializer".into()
            } else {
                format!("{vis}_methods")
            }
        }
        _ if is_casgn(node) => "constants".into(),
        Node::SingletonClassNode { .. } => "class_singleton".into(),
        Node::CallNode { .. } => "csend".into(),
        _ if is_kwbegin(node) => "kwbegin".into(),
        // ponytail: other node types only matter if a category is named after them.
        _ => helpers::literal_node::parser_type(node).into(),
    };
    conf.find_category(&name).map_or(name, str::to_string)
}

/// find_send_node_category.
fn send_category(conf: &Conf, call: &CallNode, vis: &str) -> String {
    let name = method_name(call);
    let key = conf.find_category(name).unwrap_or(name);
    let visibility_key = if def_modifier(call) {
        if name.ends_with("_class_method") { format!("{name}s") } else { format!("{name}_methods") }
    } else {
        format!("{vis}_{key}")
    };
    if conf.index(&visibility_key).is_some() { visibility_key } else { key.to_string() }
}

/// visibility_dependent?
fn visibility_dependent(conf: &Conf, node: &Node) -> bool {
    if node.as_def_node().is_some_and(|d| d.receiver().is_none()) {
        return true;
    }
    let Some(call) = plain_send(node) else { return false };
    if def_modifier(&call) {
        return false;
    }
    let name = method_name(&call);
    let key = conf.find_category(name).unwrap_or(name);
    VISIBILITY_SCOPES.iter().any(|v| conf.index(&format!("{v}_{key}")).is_some())
}

/// class_elements + flatten_class_elements: sibling lists and the elements `(body, index)` in order.
fn flatten<'pr>(conf: &Conf, list: Vec<Node<'pr>>, bodies: &mut Vec<Body<'pr>>, elems: &mut Vec<(usize, usize)>) {
    let mut nested: Vec<Option<Vec<Node<'pr>>>> = list
        .iter()
        .map(|node| {
            let b = node.as_begin_node().filter(|b| b.begin_keyword_loc().is_some())?;
            // A kwbegin with rescue/ensure has one child of an ignored type.
            if b.rescue_clause().is_some() || b.ensure_clause().is_some() {
                return Some(Vec::new());
            }
            Some(b.statements().map(|s| s.body().iter().collect()).unwrap_or_default())
        })
        .collect();
    let bi = bodies.len();
    bodies.push(Body::new(conf, list));
    for (i, kw) in nested.iter_mut().enumerate() {
        match kw.take() {
            Some(children) => flatten(conf, children, bodies, elems),
            None => elems.push((bi, i)),
        }
    }
}

/// `processed_source.comment_at_line(line)` exists.
fn comment_starts_on(src: &Source, line: usize) -> bool {
    let start = src.line_start(line);
    let i = src.comments.partition_point(|c| c.start < start);
    src.comments.get(i).is_some_and(|c| src.line_of(c.start) == line)
}

/// source_range_with_comment with ClassStructure's begin/end positions; None where RuboCop's range
/// would start before the buffer (an element on line 1), which raises in the corrector.
fn range_with_comment(src: &Source, node: &Node) -> Option<(usize, usize)> {
    let loc = node.location();
    let first_line = src.line_of(loc.start_offset());
    let mut start_line = first_line;
    for line in (1..first_line).rev() {
        if !comment_starts_on(src, line) {
            break;
        }
        if src.lines[line - 1].trim_start_matches([' ', '\t', '\n', '\x0b', '\x0c', '\r']).starts_with('#') {
            start_line = line;
        }
    }
    let begin = src.line_start(start_line).checked_sub(1)?;
    let end = match is_casgn(node).then(|| helpers::heredoc::first_heredoc_end(node)).flatten() {
        Some(e) => e + 1,
        None => {
            let line = src.line_of(loc.end_offset());
            if line < src.line_count() { src.line_start(line + 1) - 1 } else { src.text.len() }
        }
    };
    Some((begin, end))
}

impl Cop for ClassStructure {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CLASS_NODE, kind::SINGLETON_CLASS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let body = match node {
            Node::ClassNode { .. } => node.as_class_node().unwrap().body(),
            _ => node.as_singleton_class_node().unwrap().body(),
        };
        // A body with rescue/ensure is one (ignored) element.
        let Some(stmts) = body.as_ref().and_then(Node::as_statements_node) else { return };
        let conf = Conf {
            order: match cx.cfg.get("ExpectedOrder") {
                Some(Value::Str(s)) => Order::Str(s.clone()),
                _ => Order::List(cx.cfg.array("ExpectedOrder")),
            },
            // ponytail: category members as an Array (the documented form).
            categories: match cx.cfg.get("Categories") {
                Some(Value::Map(m)) => m
                    .0
                    .iter()
                    .map(|(k, v)| {
                        let names = match v {
                            Value::Seq(items) => items.iter().filter_map(Value::as_string).collect(),
                            _ => Vec::new(),
                        };
                        (k.clone(), names)
                    })
                    .collect(),
                _ => Vec::new(),
            },
        };
        let mut bodies = Vec::new();
        let mut elems = Vec::new();
        flatten(&conf, stmts.body().iter().collect(), &mut bodies, &mut elems);

        // out_of_order_elements
        let mut out = Vec::new();
        let mut max_index: Option<usize> = None;
        let mut previous: Option<&str> = None;
        for &(bi, i) in &elems {
            let b = &bodies[bi];
            let Some(index) = b.rank[i] else { continue };
            let category = b.class[i].as_str();
            if let Some(max) = max_index
                && index < max
                && previous != Some(category)
            {
                out.push((bi, i, format!("`{category}` is supposed to appear before `{}`.", conf.at(max))));
            }
            max_index = max_index.max(Some(index));
            previous = Some(category);
        }

        let src = cx.src;
        // Registered in reverse source order, like RuboCop.
        for (bi, i, msg) in out.into_iter().rev() {
            let body = &mut bodies[bi];
            let mut edits: Vec<((usize, usize), (usize, usize))> = Vec::new();
            if !body.dynamic[i]
                && let Some(anchor) = body.insertion_anchor(i)
            {
                // A range starting before the buffer raises in the corrector: RuboCop's on_class
                // stops there, losing this offense and the earlier ones.
                let Some(anchor_range) = range_with_comment(src, &body.sibs[anchor]) else { return };
                for j in body.movable_group(i).into_iter().rev() {
                    let Some(r) = range_with_comment(src, &body.sibs[j]) else { return };
                    edits.push((anchor_range, r));
                }
            }
            let loc = body.sibs[i].location();
            cx.add_fix(loc.start_offset(), loc.end_offset(), msg, |c| {
                for (anchor, r) in edits {
                    c.insert_before(anchor.0, anchor.1, &src.text[r.0..r.1]);
                    c.remove(r.0, r.1);
                }
            });
        }
    }
}
