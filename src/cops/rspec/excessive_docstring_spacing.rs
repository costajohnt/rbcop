use crate::prelude::*;

pub const NAME: &str = "RSpec/ExcessiveDocstringSpacing";
pub static COP: ExcessiveDocstringSpacing = ExcessiveDocstringSpacing;
pub struct ExcessiveDocstringSpacing;

// Ruby's [[:blank:]] is Unicode (Zs and tab), [[:space:]] the White_Space property.
static EXCESSIVE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\A[\t\p{Zs}]|[\t\p{Zs}]\z|[^\s][\t\p{Zs}]{2,}[^\t\p{Zs}]").unwrap()
});
static RUNS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[\t\p{Zs}]{2,}").unwrap());
static ENDS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A[\t\p{Zs}]|[\t\p{Zs}]\z").unwrap());

impl Cop for ExcessiveDocstringSpacing {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send #rspec? {#Examples.all #ExampleGroups.all} ${$str $(dstr ({str dstr `sym} ...) ...)} ...)
        let call = node.as_call_node().unwrap();
        if !helpers::rspec::rspec_recv(&call) {
            return;
        }
        let l = helpers::rspec::lang(cx);
        let n = method_name(&call);
        if !(l.example(n) || l.example_group(n)) {
            return;
        }
        let Some(desc) = send_args(&call).into_iter().next() else { return };
        let ok = helpers::rspec::is_str(&desc)
            || (helpers::rspec::is_dstr(&desc) && helpers::rspec::dstr_first_is_str(&desc));
        if !ok || is_heredoc(&desc) {
            return;
        }
        let text = helpers::rspec::dstr_value(&desc);
        if !EXCESSIVE.is_match(&text) {
            return;
        }
        let corrected = RUNS.replace_all(&text, " ");
        let corrected = ENDS.replace_all(&corrected, "").into_owned();
        let (ns, ne) = helpers::rspec::node_range(&desc);
        let (ds, de) = (ns + 1, ne - 1);
        if !helpers::rspec::docstring_inert(&desc) {
            cx.add_withheld_fix(ds, de, "Excessive whitespace.");
            return;
        }
        cx.add_fix(ds, de, "Excessive whitespace.", |c| c.replace(ds, de, &corrected));
    }
}
