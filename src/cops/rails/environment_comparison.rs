use crate::prelude::*;

pub const NAME: &str = "Rails/EnvironmentComparison";
pub static COP: EnvironmentComparison = EnvironmentComparison;
pub struct EnvironmentComparison;

static IDENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A[A-Za-z_][A-Za-z0-9_]*\z").unwrap());

/// (send (const {nil? cbase} :Rails) :env)
fn rails_env(n: &Node) -> bool {
    helpers::rails::send_node(n).is_some_and(|c| {
        method_name(&c) == "env" && send_args(&c).is_empty() && c.receiver().is_some_and(|r| helpers::rails::is_const(&r, "Rails"))
    })
}

/// (send <Rails.env> :to_sym)
fn rails_env_to_sym(n: &Node) -> bool {
    helpers::rails::send_node(n)
        .is_some_and(|c| method_name(&c) == "to_sym" && send_args(&c).is_empty() && c.receiver().is_some_and(|r| rails_env(&r)))
}

impl Cop for EnvironmentComparison {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = helpers::rails::send(node) else { return };
        let op = method_name(&call);
        if !matches!(op, "==" | "!=") {
            return;
        }
        let args = send_args(&call);
        let [arg] = args.as_slice() else { return };
        let Some(recv) = call.receiver() else { return };
        let str_ = |n: &Node| helpers::rails::str_value(n);
        let sym = |n: &Node| helpers::rails::sym_value(n);
        // (env side, value, symbol comparison with a plain Rails.env)
        let (env, value, sym_msg) = if rails_env(&recv) && str_(arg).is_some() {
            (&recv, str_(arg).unwrap(), false)
        } else if rails_env_to_sym(&recv) && sym(arg).is_some() {
            (&recv, sym(arg).unwrap(), false)
        } else if str_(&recv).is_some() && rails_env(arg) {
            (arg, str_(&recv).unwrap(), false)
        } else if sym(&recv).is_some() && rails_env_to_sym(arg) {
            (arg, sym(&recv).unwrap(), false)
        } else if rails_env(&recv) && sym(arg).is_some() {
            (&recv, sym(arg).unwrap(), true)
        } else if sym(&recv).is_some() && rails_env(arg) {
            (arg, sym(&recv).unwrap(), true)
        } else {
            return;
        };
        // `receiver = receiver.receiver if receiver.method?(:to_sym)`
        let env_src = match helpers::rails::send_node(env).filter(|c| method_name(c) == "to_sym") {
            Some(c) => node_text(&c.receiver().unwrap()).to_string(),
            None => node_text(env).to_string(),
        };
        let bang = if op == "!=" { "!" } else { "" };
        let replacement = format!("{bang}{env_src}.{value}?");
        let (s, e) = send_range(&call);
        let msg = if sym_msg {
            "Do not compare `Rails.env` with a symbol, it will always evaluate to `false`.".to_string()
        } else {
            format!("Favor `{replacement}` over `{}`.", cx.src.slice(s, e))
        };
        // Deliberate difference (docs/known-gaps.md): the value becomes a method name only when it is one.
        if !IDENT.is_match(&value) {
            return cx.add_withheld_fix(s, e, msg);
        }
        cx.add_fix(s, e, msg, |c| c.replace(s, e, &replacement));
    }
}
