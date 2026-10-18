use crate::prelude::*;

pub const NAME: &str = "RSpec/SharedExamples";
pub static COP: SharedExamples = SharedExamples;
pub struct SharedExamples;

static SYMBOL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A:[\p{L}_][\p{L}\p{N}_]*[?!=]?\z").unwrap());

impl Cop for SharedExamples {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // {(send #rspec? #SharedGroups.all $_ ...) (send nil? #Includes.all $_ ...)}
        let call = node.as_call_node().unwrap();
        if is_csend(&call) {
            return;
        }
        // NodePattern checks the arity (`$_`) before it calls the predicates.
        let Some(arg) = send_args(&call).into_iter().next() else { return };
        let l = helpers::rspec::lang(cx);
        let name = method_name(&call);
        let ok = (helpers::rspec::rspec_recv(&call) && l.shared_group(name)) || (call.receiver().is_none() && l.include(name));
        if !ok {
            return;
        }
        let (s, e) = helpers::rspec::node_range(&arg);
        if cx.cfg.str("EnforcedStyle") == Some("symbol") {
            if !helpers::rspec::is_str(&arg) {
                return;
            }
            let value = String::from_utf8_lossy(arg.as_string_node().unwrap().unescaped()).into_owned();
            let prefer = format!(":{}", value.to_lowercase().replace(' ', "_"));
            let current = helpers::ruby_string::inspect(value.as_bytes());
            let msg = format!("Prefer {prefer} over `{current}` to symbolize shared examples.");
            // The text becomes a symbol literal as is: only a plain one is written.
            if !SYMBOL.is_match(&prefer) {
                cx.add_withheld_fix(s, e, msg);
                return;
            }
            cx.add_fix(s, e, msg, |c| c.replace(s, e, &prefer));
        } else {
            if !helpers::rspec::is_sym(cx, &arg) {
                return;
            }
            let sym = arg.as_symbol_node().unwrap();
            let value = String::from_utf8_lossy(sym.unescaped()).into_owned();
            let prefer = format!("'{}'", value.replace('_', " "));
            let current = helpers::ruby_string::symbol_inspect(sym.unescaped());
            let msg = format!("Prefer {prefer} over `{current}` to titleize shared examples.");
            // The text goes into a new '...' literal unescaped.
            if value.contains(['\'', '\\']) {
                cx.add_withheld_fix(s, e, msg);
                return;
            }
            cx.add_fix(s, e, msg, |c| c.replace(s, e, &prefer));
        }
    }
}
