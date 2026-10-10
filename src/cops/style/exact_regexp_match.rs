use crate::helpers::regexp_parser::{self, ExprKind};
use crate::prelude::*;

pub const NAME: &str = "Style/ExactRegexpMatch";
pub static COP: ExactRegexpMatch = ExactRegexpMatch;
pub struct ExactRegexpMatch;

impl Cop for ExactRegexpMatch {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        if !matches!(name, "=~" | "===" | "!~" | "match" | "match?") {
            return;
        }
        let Some(receiver) = call.receiver() else { return };
        let args = send_args(&call);
        let [arg] = args.as_slice() else { return };
        // `(regexp (str $_) (regopt))`: one string part, no options.
        let Some(re) = arg.as_regular_expression_node() else { return };
        let content = text(&re.content_loc());
        if content.is_empty() || content.contains('\n') || re.closing_loc().as_slice().len() != 1 {
            return;
        }
        let Some(tree) = regexp_parser::tree(content, false) else { return };
        let token = |i: usize, s: &str| tree.get(i).is_some_and(|e| &content[e.start..e.end] == s);
        let Some(lit) = tree.get(1) else { return };
        if !token(0, "\\A") || lit.kind != ExprKind::Literal || !token(2, "\\z") || lit.quantifier.is_some() {
            return;
        }
        let mut string = String::new();
        for ch in content[lit.start..lit.end].chars() {
            if ch == '\'' || ch == '\\' {
                string.push('\\');
            }
            string.push(ch);
        }
        let op = if name == "!~" { "!=" } else { "==" };
        let prefer = format!("{} {op} '{string}'", node_text(&receiver));
        let (start, end) = send_range(&call);
        cx.add_fix(start, end, format!("Use `{prefer}`."), |c| c.replace(start, end, &prefer));
    }
}
