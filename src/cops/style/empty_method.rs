use crate::helpers::alignment::cop_enabled;
use crate::helpers::comments_help::contains_comments;
use crate::prelude::*;

pub const NAME: &str = "Style/EmptyMethod";
pub static COP: EmptyMethod = EmptyMethod;
pub struct EmptyMethod;

impl Cop for EmptyMethod {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let def = node.as_def_node().unwrap();
        let (s, e) = (node.location().start_offset(), node.location().end_offset());
        let (first, last) = (cx.src.line_of(s), cx.src.line_of(e));
        if def.body().is_some() || contains_comments(cx, first, last + 1) {
            return;
        }
        let compact = cx.cfg.str("EnforcedStyle") != Some("expanded");
        if (compact && first == last) || (!compact && first != last) {
            return;
        }
        if compact
            && cop_enabled(cx, "Style/SingleLineMethods")
            && cx.config.for_cop("Style/SingleLineMethods").bool("AllowIfMethodIsEmpty") == Some(false)
        {
            return;
        }
        // parser drops the parens of `def (expr).foo`
        let scope = def
            .receiver()
            .map(|r| {
                let text = node_text(&r);
                format!("{}.", def.receiver().and_then(strip_parens).map_or(text, |n| node_text(&n)))
            })
            .unwrap_or_default();
        let args: Vec<&str> = def.parameters().map(|p| parameter_nodes(&p)).unwrap_or_default().iter().map(node_text).collect();
        let arguments = if args.is_empty() {
            String::new()
        } else if def.lparen_loc().is_some() {
            format!("({})", args.join(", "))
        } else {
            format!(" {}", args.join(", "))
        };
        let name = std::str::from_utf8(def.name().as_slice()).unwrap_or("");
        let joint = if compact { "; ".to_string() } else { format!("\n{}", " ".repeat(cx.src.column(s))) };
        let correction = format!("def {scope}{name}{arguments}{joint}end");
        let max = cop_enabled(cx, "Layout/LineLength")
            .then(|| cx.config.for_cop("Layout/LineLength").int("Max").unwrap_or(120));
        let exceeds = compact && max.is_some_and(|m| correction.chars().count() as i64 > m);
        let msg = if compact {
            "Put empty method definitions on a single line."
        } else {
            "Put the `end` of empty method definitions on the next line."
        };
        cx.add_fix(s, e, msg, |c| {
            if !exceeds {
                c.replace(s, e, &correction);
            }
        });
    }
}
