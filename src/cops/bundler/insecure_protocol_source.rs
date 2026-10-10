use crate::prelude::*;

pub const NAME: &str = "Bundler/InsecureProtocolSource";
pub static COP: InsecureProtocolSource = InsecureProtocolSource;
pub struct InsecureProtocolSource;

impl Cop for InsecureProtocolSource {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if call.receiver().is_some() || method_name(&call) != "source" {
            return;
        }
        let args = send_args(&call);
        let [arg] = args.as_slice() else { return };
        let msg = if let Some(sym) = arg.as_symbol_node() {
            let name = String::from_utf8_lossy(sym.unescaped());
            if !matches!(name.as_ref(), "gemcutter" | "rubygems" | "rubyforge") {
                return;
            }
            format!(
                "The source `:{name}` is deprecated because HTTP requests are insecure. Please change your source to 'https://rubygems.org' if possible, or 'http://rubygems.org' if not."
            )
        } else if arg.as_string_node().is_some_and(|s| s.unescaped() == b"http://rubygems.org") {
            if cx.cfg.bool("AllowHttpProtocol") != Some(false) {
                return;
            }
            "Use `https://rubygems.org` instead of `http://rubygems.org`.".to_string()
        } else {
            return;
        };
        let (s, e) = (arg.location().start_offset(), arg.location().end_offset());
        cx.add_fix(s, e, msg, |c| c.replace(s, e, "'https://rubygems.org'"));
    }
}
