use crate::prelude::*;
use ruby_prism::{CallNode, Visit};

pub const NAME: &str = "RSpec/DescribedClass";
pub static COP: DescribedClass = DescribedClass;
pub struct DescribedClass;

/// `described_constant`: `(block (send _ :describe $(const ...) ...) (args) $_)`: the described
/// constant of a plain, parameterless `describe` block.
fn described_constant<'pr>(node: &Node<'pr>) -> Option<Node<'pr>> {
    let call = helpers::rspec::plain_block_call(node)?;
    if is_csend(&call) || method_name(&call) != "describe" {
        return None;
    }
    let block = helpers::rspec::block_of(&call)?;
    if block.parameters().is_some_and(|p| p.as_block_parameters_node().is_none_or(|b| b.parameters().is_some() || b.locals().iter().next().is_some())) {
        return None;
    }
    let first = send_args(&call).into_iter().next()?;
    matches!(kind::of(&first), kind::CONSTANT_READ_NODE | kind::CONSTANT_PATH_NODE).then_some(first)
}

/// `const_name(node)` of DescribedClass: the constant's segments, `nil` for an `lvar`, `::` or send
/// namespace; None where RuboCop's method returns nil (any other namespace).
fn const_parts(node: &Node) -> Option<Vec<Option<String>>> {
    if let Some(c) = node.as_constant_read_node() {
        return Some(vec![Some(String::from_utf8_lossy(c.name().as_slice()).into_owned())]);
    }
    let p = node.as_constant_path_node()?;
    let name = Some(String::from_utf8_lossy(p.name()?.as_slice()).into_owned());
    match p.parent() {
        None => Some(vec![None, name]),
        Some(ns) if matches!(kind::of(&ns), kind::CONSTANT_READ_NODE | kind::CONSTANT_PATH_NODE) => {
            let mut v = const_parts(&ns)?;
            v.push(name);
            Some(v)
        }
        Some(ns) if ns.as_local_variable_read_node().is_some() || (ns.as_call_node().is_some() && helpers::rspec::block_of(&ns.as_call_node().unwrap()).is_none()) => {
            Some(vec![None, name])
        }
        Some(_) => None,
    }
}

/// `collapse_namespace(namespace, const)`
fn collapse(namespace: &[String], konst: Option<Vec<Option<String>>>) -> Option<Vec<Option<String>>> {
    let konst = konst?;
    if namespace.is_empty() || konst.first().is_some_and(|f| f.is_none()) {
        return Some(konst);
    }
    let ns: Vec<Option<String>> = namespace.iter().cloned().map(Some).collect();
    let max = ns.len();
    let start = max.saturating_sub(konst.len());
    let shift = (start..=max)
        .find(|&shift| {
            let n = max - shift;
            ns[shift..max] == konst[..n.min(konst.len())]
        })
        .unwrap_or(max);
    let mut out = ns[..shift].to_vec();
    out.extend(konst);
    Some(out)
}

struct V<'a, 'pr> {
    l: &'a helpers::rspec::Language,
    namespace: Vec<String>,
    explicit: bool,
    only_static: bool,
    skip_blocks: bool,
    /// Plain blocks around the current node, innermost last (parser `each_ancestor(:block)`).
    blocks: Vec<Node<'pr>>,
    found: Vec<(usize, usize, Option<String>)>,
}

impl<'pr> V<'_, 'pr> {
    fn full_name(&self, node: &Node) -> Option<Vec<Option<String>>> {
        collapse(&self.namespace, const_parts(node))
    }

    fn check_const(&mut self, node: &Node<'pr>) {
        if self.explicit {
            return;
        }
        // contains_described_class?: (send nil? :described_class) inside the constant
        struct C(bool);
        impl<'pr> Visit<'pr> for C {
            fn visit_call_node(&mut self, n: &CallNode<'pr>) {
                if n.receiver().is_none() && method_name(n) == "described_class" {
                    self.0 = true;
                }
                ruby_prism::visit_call_node(self, n);
            }
        }
        let mut c = C(false);
        c.visit(node);
        if c.0 {
            return;
        }
        let Some(nearest) = self.blocks.iter().rev().find_map(described_constant) else { return };
        if nearest.location().start_offset() == node.location().start_offset()
            && nearest.location().end_offset() == node.location().end_offset()
        {
            return;
        }
        if self.full_name(&nearest) == self.full_name(node) {
            let loc = node.location();
            self.found.push((loc.start_offset(), loc.end_offset(), helpers::const_name::const_name(node)));
        }
    }

    /// scope_change? of a call: common_instance_exec_closure? or skippable_block?
    fn call_scope_change(&self, call: &CallNode) -> bool {
        let Some(block) = helpers::rspec::block_of(call) else { return false };
        let plain = helpers::rspec::is_plain(&block);
        if plain && !is_csend(call) {
            let name = method_name(call);
            let bare_const = |names: &[&str]| {
                call.receiver().and_then(|r| r.as_constant_read_node()).is_some_and(|c| names.iter().any(|n| c.name().as_slice() == n.as_bytes()))
            };
            if (name == "new" && bare_const(&["Class", "Module", "Struct"]))
                || (name == "define" && bare_const(&["Data"]))
                || matches!(name, "class_eval" | "module_eval" | "instance_eval" | "class_exec" | "module_exec" | "instance_exec")
            {
                return true;
            }
        }
        // skippable_block?: any block that is not `(any_block (send #rspec? #ALL.all ...) ...)`
        self.skip_blocks && !(helpers::rspec::rspec_recv(call) && self.l.any(method_name(call)))
    }
}

impl<'pr> Visit<'pr> for V<'_, 'pr> {
    fn visit_constant_read_node(&mut self, node: &ruby_prism::ConstantReadNode<'pr>) {
        self.check_const(&node.as_node());
    }

    fn visit_constant_path_node(&mut self, node: &ruby_prism::ConstantPathNode<'pr>) {
        self.check_const(&node.as_node());
        if !self.only_static {
            ruby_prism::visit_constant_path_node(self, node);
        }
    }

    fn visit_call_node(&mut self, node: &CallNode<'pr>) {
        // A scope-changing block hides its send too.
        if self.call_scope_change(node) {
            return;
        }
        if self.explicit && !is_csend(node) && method_name(node) == "described_class" {
            let (s, e) = send_range(node);
            self.found.push((s, e, None));
        }
        let pushed = helpers::rspec::plain_block_call(&node.as_node()).is_some();
        if pushed {
            self.blocks.push(node.as_node());
        }
        ruby_prism::visit_call_node(self, node);
        if pushed {
            self.blocks.pop();
        }
    }

    // `A::B = x` is parser's (casgn (const nil :A) :B x): the target is no `const` node, its
    // namespace is.
    fn visit_constant_path_write_node(&mut self, node: &ruby_prism::ConstantPathWriteNode<'pr>) {
        if let Some(ns) = node.target().parent() {
            self.visit(&ns);
        }
        self.visit(&node.value());
    }

    fn visit_constant_path_operator_write_node(&mut self, node: &ruby_prism::ConstantPathOperatorWriteNode<'pr>) {
        if let Some(ns) = node.target().parent() {
            self.visit(&ns);
        }
        self.visit(&node.value());
    }

    fn visit_constant_path_or_write_node(&mut self, node: &ruby_prism::ConstantPathOrWriteNode<'pr>) {
        if let Some(ns) = node.target().parent() {
            self.visit(&ns);
        }
        self.visit(&node.value());
    }

    fn visit_constant_path_and_write_node(&mut self, node: &ruby_prism::ConstantPathAndWriteNode<'pr>) {
        if let Some(ns) = node.target().parent() {
            self.visit(&ns);
        }
        self.visit(&node.value());
    }

    fn visit_constant_path_target_node(&mut self, node: &ruby_prism::ConstantPathTargetNode<'pr>) {
        if let Some(ns) = node.parent() {
            self.visit(&ns);
        }
    }

    fn visit_def_node(&mut self, node: &ruby_prism::DefNode<'pr>) {
        // `def` is a scope change, `defs` (def self.x) is not.
        if node.receiver().is_some() {
            ruby_prism::visit_def_node(self, node);
        }
    }

    fn visit_lambda_node(&mut self, node: &ruby_prism::LambdaNode<'pr>) {
        // `-> {}` is a parser block of `lambda`, never an RSpec one.
        if !self.skip_blocks {
            ruby_prism::visit_lambda_node(self, node);
        }
    }

    fn visit_class_node(&mut self, _: &ruby_prism::ClassNode<'pr>) {}

    fn visit_module_node(&mut self, _: &ruby_prism::ModuleNode<'pr>) {}
}

impl Cop for DescribedClass {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(described) = described_constant(node) else { return };
        let call = node.as_call_node().unwrap();
        let Some(body) = helpers::rspec::block_body(&call) else { return };
        let l = helpers::rspec::lang(cx);
        let explicit = cx.cfg.str("EnforcedStyle") == Some("explicit");
        // Namespace#namespace: the enclosing classes and modules (the same for every node of the body).
        let mut namespace = Vec::new();
        for a in cx.ancestors.iter() {
            if a.as_class_node().is_some() || a.as_module_node().is_some() {
                let Some(name) = helpers::rails::defined_module_name(a) else { return };
                namespace.extend(name.split("::").map(String::from));
            }
        }
        // parser ancestors of the body that are plain blocks, outermost first
        let blocks: Vec<Node> = cx
            .ancestors
            .iter()
            .filter_map(|a| helpers::rspec::plain_block_call(a).map(|c| c.as_node()))
            .chain(std::iter::once(call.as_node()))
            .collect();
        let mut v = V {
            l: &l,
            namespace,
            explicit,
            only_static: cx.cfg.get("OnlyStaticConstants").is_none_or(|v| !matches!(v, Value::Null | Value::Bool(false))),
            skip_blocks: cx.cfg.bool("SkipBlocks") == Some(true),
            blocks,
            found: Vec::new(),
        };
        // find_usage(body): the body node itself first
        v.visit(&body);
        let described_name = helpers::const_name::const_name(&described).unwrap_or_default();
        for (s, e, name) in v.found {
            let (msg, replacement) = if explicit {
                (format!("Use `{described_name}` instead of `described_class`."), described_name.clone())
            } else {
                (format!("Use `described_class` instead of `{}`.", name.unwrap_or_default()), "described_class".to_string())
            };
            cx.add_fix(s, e, msg, |c| c.replace(s, e, &replacement));
        }
    }
}
