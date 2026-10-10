use crate::prelude::*;
use ruby_prism::{CallNode, Visit};

pub const NAME: &str = "Lint/IneffectiveAccessModifier";
pub static COP: IneffectiveAccessModifier = IneffectiveAccessModifier;
pub struct IneffectiveAccessModifier;

/// `private_class_methods(node).to_a.flatten.select(&:basic_literal?).map(&:value)`, keeping the
/// symbols (only those can equal a method name).
#[derive(Default)]
struct PrivateClassMethods(Vec<Vec<u8>>);

impl<'pr> Visit<'pr> for PrivateClassMethods {
    fn visit_call_node(&mut self, node: &CallNode<'pr>) {
        if node.receiver().is_none() && method_name(node) == "private_class_method" {
            for a in call_args(node) {
                if let Some(s) = a.as_symbol_node() {
                    self.0.push(s.unescaped().to_vec());
                }
            }
        }
        ruby_prism::visit_call_node(self, node);
    }
}

/// `bare_access_modifier? && !method?(:module_function)` for a direct child of a class body.
fn access_modifier<'pr>(n: &Node<'pr>) -> Option<CallNode<'pr>> {
    n.as_call_node().filter(|c| {
        c.receiver().is_none()
            && c.arguments().is_none()
            && c.block().is_none()
            && matches!(method_name(c), "public" | "protected" | "private")
    })
}

struct Hit {
    keyword: (usize, usize),
    modifier: &'static str,
    line: usize,
}

fn ineffective<'pr>(
    stmts: &[Node<'pr>],
    ignored: &mut Option<Vec<Vec<u8>>>,
    outer: &Node<'pr>,
    mut modifier: Option<(&'static str, usize)>,
    cx: &Cx,
    out: &mut Vec<Hit>,
) {
    let compute = |ignored: &mut Option<Vec<Vec<u8>>>| {
        if ignored.is_none() {
            let mut v = PrivateClassMethods::default();
            v.visit(outer);
            *ignored = Some(v.0);
        }
    };
    for child in stmts {
        if let Some(m) = access_modifier(child) {
            let name = match method_name(&m) {
                "public" => "public",
                "protected" => "protected",
                _ => "private",
            };
            modifier = Some((name, cx.src.line_of(child.location().start_offset())));
        } else if let Some(d) = child.as_def_node().filter(|d| d.receiver().is_some()) {
            compute(ignored);
            let Some((name, line)) = modifier.filter(|(m, _)| *m != "public") else { continue };
            if ignored.as_ref().unwrap().iter().any(|i| i.as_slice() == d.name().as_slice()) {
                continue;
            }
            let k = d.def_keyword_loc();
            out.push(Hit { keyword: (k.start_offset(), k.end_offset()), modifier: name, line });
        } else if let Some(b) = child.as_begin_node().filter(|b| {
            b.begin_keyword_loc().is_some() && b.rescue_clause().is_none() && b.ensure_clause().is_none() && b.else_clause().is_none()
        }) {
            compute(ignored);
            let inner: Vec<Node> = b.statements().map(|s| s.body().iter().collect()).unwrap_or_default();
            ineffective(&inner, ignored, outer, modifier, cx, out);
        }
    }
}

impl Cop for IneffectiveAccessModifier {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CLASS_NODE, kind::MODULE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let body = match node.as_class_node() {
            Some(c) => c.body(),
            None => node.as_module_node().unwrap().body(),
        };
        // `node.body.begin_type?`: two or more statements.
        let Some(stmts) = body.and_then(|b| b.as_statements_node()) else { return };
        let list: Vec<Node> = stmts.body().iter().collect();
        if list.len() < 2 {
            return;
        }
        let mut hits = Vec::new();
        ineffective(&list, &mut None, &stmts.as_node(), None, cx, &mut hits);
        for h in hits {
            let alternative = if h.modifier == "private" {
                "`private_class_method` or `private` inside a `class << self` block"
            } else {
                "`protected` inside a `class << self` block"
            };
            let msg = format!(
                "`{m}` (on line {l}) does not make singleton methods {m}. Use {alternative} instead.",
                m = h.modifier,
                l = h.line
            );
            cx.add(h.keyword.0, h.keyword.1, msg);
        }
    }
}
