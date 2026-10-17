use crate::prelude::*;
use ruby_prism::{CallNode, Visit};

pub const NAME: &str = "FactoryBot/CreateList";
pub static COP: CreateList = CreateList;
pub struct CreateList;

static NAME_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A[\p{L}_][\p{L}\p{N}_]*[?!]?\z").unwrap());

/// factory_call: `(send #factory_call? :create sym ...)`
fn factory_call<'pr>(cx: &Cx, node: &Node<'pr>) -> Option<CallNode<'pr>> {
    let c = helpers::rspec::send_call(node).filter(|c| !is_csend(c) && method_name(c) == "create")?;
    if !helpers::factory_bot::factory_call(cx, c.receiver().as_ref()) {
        return None;
    }
    let first = send_args(&c).into_iter().next()?;
    helpers::rspec::is_sym(cx, &first).then_some(c)
}

/// arguments_include_method_call?: `(send #factory_call? :create sym ... `(send ...))`: the last
/// argument (after the factory name) has a send in it.
fn arguments_include_method_call(cx: &Cx, node: &Node) -> bool {
    let Some(c) = factory_call(cx, node) else { return false };
    let args = send_args(&c);
    if args.len() < 2 {
        return false;
    }
    struct V(bool);
    impl<'pr> Visit<'pr> for V {
        fn visit_call_node(&mut self, n: &CallNode<'pr>) {
            self.0 |= !is_csend(n);
            ruby_prism::visit_call_node(self, n);
        }
    }
    let mut v = V(false);
    v.visit(args.last().unwrap());
    v.0
}

/// repeat_count: `Array.new(n) {}`, `n.times {}` or `n.times.map {}`, and the count node.
fn repeat_count<'pr>(call: &CallNode<'pr>) -> Option<(i128, Node<'pr>)> {
    if is_csend(call) {
        return None;
    }
    let int = |n: &Node<'pr>| n.as_integer_node().is_some().then(|| int_literal_value(n)).flatten();
    match method_name(call) {
        "new" => {
            let r = call.receiver()?;
            let arr = r.as_constant_read_node().is_some_and(|c| c.name().as_slice() == b"Array")
                || r.as_constant_path_node().is_some_and(|p| p.parent().is_none() && p.name().is_some_and(|n| n.as_slice() == b"Array"));
            let args = send_args(call);
            if args.len() != 1 {
                return None;
            }
            let a = args.into_iter().next()?;
            arr.then(|| int(&a).map(|v| (v, a))).flatten()
        }
        "times" if send_args(call).is_empty() => {
            let r = call.receiver()?;
            int(&r).map(|v| (v, r))
        }
        "map" if send_args(call).is_empty() => {
            let t = call.receiver().and_then(|r| helpers::rspec::send_call(&r))?;
            if is_csend(&t) || method_name(&t) != "times" || !send_args(&t).is_empty() {
                return None;
            }
            let r = t.receiver()?;
            int(&r).map(|v| (v, r))
        }
        _ => None,
    }
}

impl Cop for CreateList {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE, kind::ARRAY_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let style = cx.cfg.str("EnforcedStyle").unwrap_or("create_list").to_string();
        let src = cx.src;
        if let Some(array) = node.as_array_node() {
            // on_array: identical factory calls in the array
            let elements: Vec<Node> = array.elements().iter().collect();
            if elements.len() < 2 {
                return;
            }
            let first = node_text(&elements[0]);
            if !elements.iter().all(|e| node_text(e) == first) {
                return;
            }
            if !factory_calls_in_array(cx, node) {
                return;
            }
            let msg = if style == "create_list" && !arguments_include_method_call(cx, &elements[0]) {
                "Prefer create_list.".to_string()
            } else {
                format!("Prefer {}.times.map.", elements.len())
            };
            let replacement = format!("{}.times.map {{ {first} }}", elements.len());
            let (s, e) = helpers::rspec::node_range(node);
            cx.add_fix(s, e, msg, |c| c.replace(s, e, &replacement));
            return;
        }
        let call = node.as_call_node().unwrap();
        if style == "n_times" {
            // factory_list_call: (send #factory_call? :create_list (sym _) (int $_) ...)
            if is_csend(&call) || method_name(&call) != "create_list" || !helpers::factory_bot::factory_call(cx, call.receiver().as_ref()) {
                return;
            }
            let args = send_args(&call);
            let [factory, count, options @ ..] = &args[..] else { return };
            if !helpers::rspec::is_sym(cx, factory) || count.as_integer_node().is_none() {
                return;
            }
            if int_literal_value(count).is_none_or(|v| v < 2) {
                return;
            }
            let mut arguments = node_text(factory).to_string();
            if !options.is_empty() {
                arguments.push_str(", ");
                arguments.push_str(&options.iter().map(|o| node_text(o)).collect::<Vec<_>>().join(", "));
            }
            let block = helpers::rspec::block_of(&call);
            let receiver = call.receiver().map_or(String::new(), |r| format!("{}.", node_text(&r)));
            let parenthesized = call.opening_loc().is_some_and(|o| o.as_slice() == b"(");
            let mut replacement = receiver;
            replacement.push_str(&if parenthesized { format!("create({arguments})") } else { format!("create {arguments}") });
            if let Some(b) = &block {
                replacement.push(' ');
                replacement.push_str(src.slice(b.opening_loc().start_offset(), b.closing_loc().end_offset()));
            }
            let replacement = format!("{}.times.map {{ {replacement} }}", node_text(count));
            let msg = format!("Prefer {}.times.map.", int_literal_value(count).unwrap());
            let sel = call.message_loc().unwrap();
            let (rs, re) = helpers::rspec::node_range(node);
            cx.add_fix(sel.start_offset(), sel.end_offset(), msg, |c| c.replace(rs, re, &replacement));
            return;
        }
        if style != "create_list" {
            return;
        }
        // on_block
        let Some(block) = helpers::rspec::block_of(&call).filter(helpers::rspec::is_plain) else { return };
        let Some((count, count_node)) = repeat_count(&call) else { return };
        if count <= 1 {
            return;
        }
        let Some(body) = helpers::rspec::parser_body(block.body()) else { return };
        // block_with_arg_and_used?: one `|x|` argument that the body mentions
        if let Some(p) = block.parameters().and_then(|p| p.as_block_parameters_node()).and_then(|p| p.parameters())
            && p.requireds().iter().count() == 1
            && p.optionals().iter().next().is_none()
            && p.rest().is_none()
            && p.posts().iter().next().is_none()
            && p.keywords().iter().next().is_none()
            && p.keyword_rest().is_none()
            && p.block().is_none()
            && let Some(r) = p.requireds().iter().next().and_then(|r| r.as_required_parameter_node())
            && mentions(&body, r.name().as_slice(), src.target_ruby)
        {
            return;
        }
        if arguments_include_method_call(cx, &body) {
            return;
        }
        // contains_only_factory?
        let inner = helpers::rspec::any_block_call(&body);
        let factory = match &inner {
            Some(b) => factory_call_ignoring_block(cx, b),
            None => factory_call(cx, &body),
        };
        let Some(factory) = factory else { return };
        let (ss, se) = send_range(&call);
        let (ns, ne) = helpers::rspec::node_range(node);
        let count_src = node_text(&count_node).to_string();
        let args = send_args(&factory);
        let receiver = factory.receiver().map_or(String::new(), |r| format!("{}.", node_text(&r)));
        let options: Vec<String> = args[1..].iter().map(|o| node_text(o).to_string()).collect();
        let replacement = match &inner {
            Some(b) => {
                // call_with_block_replacement
                let name = helpers::rails::sym_value(&args[0]).unwrap_or_default();
                let mut arguments = format!(":{name}, {count_src}");
                if !options.is_empty() {
                    arguments.push_str(", ");
                    arguments.push_str(&options.join(", "));
                }
                let inner_block = helpers::rspec::block_of(b).unwrap();
                let params = inner_block.parameters().map_or("", |p| node_text(&p));
                let Some(inner_body) = helpers::rspec::parser_body(inner_block.body()) else { return };
                let block_src = if inner_body.as_statements_node().is_some() {
                    let (bs, be) = body_range(&inner_body);
                    let indent = " ".repeat(src.column(bs));
                    let indent_end = " ".repeat(src.column(ns));
                    format!(" do {params}\n{indent}{}\n{indent_end}end", src.slice(bs, be))
                } else {
                    format!(" {{ {params} {} }}", node_text(&inner_body))
                };
                // The factory name becomes a new `:name` literal: only a plain one is written.
                if !NAME_RE.is_match(&name) {
                    cx.add_withheld_fix(ss, se, "Prefer create_list.");
                    return;
                }
                format!("{receiver}create_list({arguments}){block_src}")
            }
            None => {
                let mut arguments = format!("{}, {count_src}", node_text(&args[0]));
                if !options.is_empty() {
                    arguments.push_str(", ");
                    arguments.push_str(&options.join(", "));
                }
                let parenthesized = factory.opening_loc().is_some_and(|o| o.as_slice() == b"(");
                if parenthesized { format!("{receiver}create_list({arguments})") } else { format!("{receiver}create_list {arguments}") }
            }
        };
        cx.add_fix(ss, se, "Prefer create_list.", |c| c.replace(ns, ne, &replacement));
    }
}

/// factory_call on the send of a block call.
fn factory_call_ignoring_block<'pr>(cx: &Cx, b: &CallNode<'pr>) -> Option<CallNode<'pr>> {
    if is_csend(b) || method_name(b) != "create" || !helpers::factory_bot::factory_call(cx, b.receiver().as_ref()) {
        return None;
    }
    let first = send_args(b).into_iter().next()?;
    helpers::rspec::is_sym(cx, &first).then(|| b.as_node().as_call_node().unwrap())
}

/// `` `_value ``: the body has a node (or symbol) named like the block argument.
fn mentions(body: &Node, name: &[u8], target: f64) -> bool {
    struct V<'a>(&'a [u8], bool, f64);
    impl<'pr> Visit<'pr> for V<'_> {
        fn visit_local_variable_read_node(&mut self, n: &ruby_prism::LocalVariableReadNode<'pr>) {
            self.1 |= n.name().as_slice() == self.0;
        }
        fn visit_local_variable_write_node(&mut self, n: &ruby_prism::LocalVariableWriteNode<'pr>) {
            self.1 |= n.name().as_slice() == self.0;
            ruby_prism::visit_local_variable_write_node(self, n);
        }
        fn visit_call_node(&mut self, n: &CallNode<'pr>) {
            self.1 |= n.name().as_slice() == self.0;
            ruby_prism::visit_call_node(self, n);
        }
        fn visit_symbol_node(&mut self, n: &ruby_prism::SymbolNode<'pr>) {
            self.1 |= !is_parser_dsym(&n.as_node(), self.2) && n.unescaped() == self.0;
        }
    }
    let mut v = V(name, false, target);
    v.visit(body);
    v.1
}

/// factory_calls_in_array?: an array (this one or one inside it) of factory calls only.
fn factory_calls_in_array(cx: &Cx, node: &Node) -> bool {
    struct A<'pr>(Vec<ruby_prism::ArrayNode<'pr>>);
    impl<'pr> Visit<'pr> for A<'pr> {
        fn visit_array_node(&mut self, n: &ruby_prism::ArrayNode<'pr>) {
            self.0.push(n.as_node().as_array_node().unwrap());
            ruby_prism::visit_array_node(self, n);
        }
    }
    let mut a = A(Vec::new());
    a.visit(node);
    a.0.iter().any(|arr| {
        let els: Vec<Node> = arr.elements().iter().collect();
        !els.is_empty() && els.iter().all(|e| factory_call(cx, e).is_some())
    })
}
