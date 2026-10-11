use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Lint/UselessRescue";
pub static COP: UselessRescue = UselessRescue;
pub struct UselessRescue;

const MSG: &str = "Useless `rescue` detected.";

/// Sources of the local variable reads under a node (`each_descendant(:lvar).map(&:source)`).
#[derive(Default)]
struct Lvars(Vec<String>);

impl<'pr> Visit<'pr> for Lvars {
    fn visit_local_variable_read_node(&mut self, node: &ruby_prism::LocalVariableReadNode<'pr>) {
        // parser's lvar source is the bare name (Prism's `foo(name:)` read spans the colon).
        self.0.push(String::from_utf8_lossy(node.name().as_slice()).into_owned());
    }
}

/// `only_reraising?` for a resbody with the given body and `=> var` source.
fn only_reraising(body: Option<Node>, var: Option<&str>) -> bool {
    let Some(body) = body else { return false };
    let body = match body.as_statements_node() {
        Some(s) => {
            let stmts: Vec<_> = s.body().iter().collect();
            if stmts.len() != 1 {
                return false;
            }
            stmts.into_iter().next().unwrap()
        }
        None => body,
    };
    let Some(call) = body.as_call_node() else { return false };
    // A call with a literal block is a parser `block`, not a `send`.
    if call.block().is_some_and(|b| b.as_block_node().is_some()) || call.receiver().is_some() || method_name(&call) != "raise" {
        return false;
    }
    let args = send_args(&call);
    match args.as_slice() {
        [] => true,
        [arg] => {
            let src = node_text(arg);
            var == Some(src) || src == "$!" || src == "$ERROR_INFO"
        }
        _ => false,
    }
}

impl Cop for UselessRescue {
    fn kinds(&self) -> &'static [usize] {
        &[kind::BEGIN_NODE, kind::RESCUE_MODIFIER_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if let Some(m) = node.as_rescue_modifier_node() {
            if only_reraising(Some(m.rescue_expression()), None) {
                let (s, e) = helpers::resbody::modifier_range(&m);
                cx.add(s, e, MSG);
            }
            return;
        }
        let Some(mut resbody) = node.as_begin_node().unwrap().rescue_clause() else { return };
        while let Some(next) = resbody.subsequent() {
            resbody = next;
        }
        let var = resbody.reference().map(|r| node_text(&r));
        if let Some(var) = var {
            // `each_ancestor(:ensure).first`: the nearest begin with an ensure clause.
            let ensure = std::iter::once(node)
                .chain(cx.ancestors.iter().rev())
                .find_map(|a| a.as_begin_node().and_then(|b| b.ensure_clause()));
            if let Some(body) = ensure.and_then(|e| e.statements()) {
                let mut lvars = Lvars::default();
                lvars.visit_statements_node(&body);
                if lvars.0.iter().any(|l| l == var) {
                    return;
                }
            }
        }
        if only_reraising(resbody.statements().map(|s| s.as_node()), var) {
            let (s, e) = helpers::resbody::range(cx.src, &resbody);
            cx.add(s, e, MSG);
        }
    }
}
