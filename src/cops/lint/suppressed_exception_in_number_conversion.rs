use crate::prelude::*;
use helpers::parser_parent::is_top_const;
use ruby_prism::CallNode;

pub const NAME: &str = "Lint/SuppressedExceptionInNumberConversion";
pub static COP: SuppressedExceptionInNumberConversion = SuppressedExceptionInNumberConversion;
pub struct SuppressedExceptionInNumberConversion;

const EXPECTED: &[&str] = &["ArgumentError", "TypeError", "::ArgumentError", "::TypeError"];

/// `numeric_method?`: `Integer(x)`, `Kernel.Float(x)`, ... (receiver nil or Kernel).
fn numeric_method<'pr>(node: &Node<'pr>) -> Option<CallNode<'pr>> {
    let c = node.as_call_node()?;
    if c.block().is_some_and(|b| b.as_block_node().is_some()) || !c.receiver().is_none_or(|r| is_top_const(&r, "Kernel")) {
        return None;
    }
    let n = send_args(&c).len();
    let ok = match method_name(&c) {
        "Integer" | "BigDecimal" | "Complex" | "Rational" => n == 1 || n == 2,
        "Float" => n == 1,
        _ => false,
    };
    ok.then_some(c)
}

/// The only statement of a statement list.
fn single<'pr>(s: Option<ruby_prism::StatementsNode<'pr>>) -> Option<Node<'pr>> {
    let mut it = s?.body().iter();
    let first = it.next()?;
    it.next().is_none().then_some(first)
}

fn is_nil_body(s: Option<ruby_prism::StatementsNode>) -> bool {
    single(s).is_some_and(|n| n.as_nil_node().is_some())
}

impl Cop for SuppressedExceptionInNumberConversion {
    fn correctable(&self) -> bool {
        true
    }

    fn ruby_range(&self) -> (f64, f64) {
        (2.6, f64::MAX)
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::RESCUE_MODIFIER_NODE, kind::BEGIN_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let (method, range) = if let Some(m) = node.as_rescue_modifier_node() {
            let Some(method) = numeric_method(&m.expression()) else { return };
            if m.rescue_expression().as_nil_node().is_none() {
                return;
            }
            // `begin x rescue nil end`: the kwbegin's only child.
            let kwbegin = cx.ancestors.len() >= 2
                && cx.ancestors[cx.ancestors.len() - 2].as_begin_node().is_some_and(|b| {
                    b.begin_keyword_loc().is_some()
                        && b.rescue_clause().is_none()
                        && b.else_clause().is_none()
                        && b.ensure_clause().is_none()
                        && single(b.statements()).is_some_and(|s| s.location().start_offset() == node.location().start_offset())
                });
            let l = if kwbegin { cx.ancestors[cx.ancestors.len() - 2].location() } else { node.location() };
            (method, (l.start_offset(), l.end_offset()))
        } else {
            let b = node.as_begin_node().unwrap();
            let Some(r) = b.rescue_clause() else { return };
            if r.subsequent().is_some() || r.reference().is_some() || b.else_clause().is_some() {
                return;
            }
            let Some(body) = single(b.statements()) else { return };
            let Some(method) = numeric_method(&body) else { return };
            let rescue_body_nil = is_nil_body(r.statements());
            if b.begin_keyword_loc().is_some()
                && b.ensure_clause().is_none()
                && (rescue_body_nil || r.statements().is_none())
            {
                if r.exceptions().iter().any(|e| !EXPECTED.contains(&node_text(&e))) {
                    return;
                }
                let l = node.location();
                (method, (l.start_offset(), l.end_offset()))
            } else {
                if !r.exceptions().is_empty() || !rescue_body_nil {
                    return;
                }
                let (_, end) = helpers::resbody::range(cx.src, &r);
                (method, (body.location().start_offset(), end))
            }
        };
        let args = send_args(&method);
        let exception_kw = args.iter().any(|a| {
            let pairs: Vec<Node> = match (a.as_hash_node(), a.as_keyword_hash_node()) {
                (Some(h), _) => h.elements().iter().collect(),
                (_, Some(h)) => h.elements().iter().collect(),
                _ => return false,
            };
            pairs.iter().any(|p| {
                p.as_assoc_node().and_then(|p| p.key().as_symbol_node()).is_some_and(|s| s.unescaped() == b"exception")
            })
        });
        if exception_kw {
            return;
        }
        let mut parts: Vec<&str> = args.iter().map(|a| node_text(a)).collect();
        parts.push("exception: false");
        let mut prefer = format!("{}({})", method_name(&method), parts.join(", "));
        if let Some(r) = method.receiver() {
            let dot = method.call_operator_loc().map_or("", |d| text(&d));
            prefer = format!("{}{dot}{prefer}", node_text(&r));
        }
        let (s, e) = range;
        let msg = format!("Use `{prefer}` instead.");
        cx.add_fix(s, e, msg, |c| c.replace(s, e, &prefer));
    }
}
