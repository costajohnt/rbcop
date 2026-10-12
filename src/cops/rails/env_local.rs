use crate::prelude::*;

pub const NAME: &str = "Rails/EnvLocal";
pub static COP: EnvLocal = EnvLocal;
pub struct EnvLocal;

/// (send (send (const {cbase nil?} :Rails) :env) ${:development? :test?}): the method.
fn env_local(n: &Node) -> Option<String> {
    let c = helpers::rails::send_node(n)?;
    let m = method_name(&c);
    if !matches!(m, "development?" | "test?") || !send_args(&c).is_empty() {
        return None;
    }
    let env = helpers::rails::send_node(&c.receiver()?)?;
    (method_name(&env) == "env" && send_args(&env).is_empty() && env.receiver().is_some_and(|r| helpers::rails::is_const(&r, "Rails")))
        .then(|| m.to_string())
}

/// (send #rails_env_local? :!)
fn not_env_local(n: &Node) -> Option<String> {
    let c = helpers::rails::send_node(n)?;
    if method_name(&c) != "!" || !send_args(&c).is_empty() {
        return None;
    }
    env_local(&c.receiver()?)
}

fn sides<'pr>(n: &Node<'pr>) -> Option<(Node<'pr>, Node<'pr>)> {
    if let Some(o) = n.as_or_node() {
        return Some((o.left(), o.right()));
    }
    n.as_and_node().map(|a| (a.left(), a.right()))
}

impl Cop for EnvLocal {
    fn correctable(&self) -> bool {
        true
    }

    fn min_rails(&self) -> f64 {
        7.1
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::OR_NODE, kind::AND_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let or = node.as_or_node().is_some();
        let (lhs, rhs) = sides(node).unwrap();
        let check: fn(&Node) -> Option<String> = if or { env_local } else { not_env_local };
        let Some(r) = check(&rhs) else { return };
        let (l_node, l) = if let Some(l) = check(&lhs) {
            (lhs, l)
        } else {
            // lhs.or_type? (on_or) / lhs.operator_keyword? (on_and) with a matching rhs.
            let inner = if or { lhs.as_or_node().map(|o| o.right()) } else { sides(&lhs).map(|(_, r)| r) };
            let Some(inner) = inner else { return };
            let Some(l) = check(&inner) else { return };
            (inner, l)
        };
        if l == r {
            return;
        }
        let s = l_node.location().start_offset();
        let e = rhs.location().end_offset();
        let (msg, replacement) =
            if or { ("Use `Rails.env.local?` instead.", "Rails.env.local?") } else { ("Use `!Rails.env.local?` instead.", "!Rails.env.local?") };
        cx.add_fix(s, e, msg, |c| c.replace(s, e, replacement));
    }
}
