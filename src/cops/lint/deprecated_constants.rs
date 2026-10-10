use crate::prelude::*;

pub const NAME: &str = "Lint/DeprecatedConstants";
pub static COP: DeprecatedConstants = DeprecatedConstants;
pub struct DeprecatedConstants;

/// A config value as a String (`to_s` of strings and numbers).
fn as_str(v: Option<&Value>) -> Option<String> {
    v.and_then(Value::as_string)
}

impl Cop for DeprecatedConstants {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CONSTANT_READ_NODE, kind::CONSTANT_PATH_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // The target of `Foo::Bar = 1` (and op-assign forms) is part of parser's `casgn`.
        let loc = node.location();
        let span = (loc.start_offset(), loc.end_offset());
        let is_casgn_target = cx.ancestors.last().is_some_and(|p| {
            let target = match kind::of(p) {
                kind::CONSTANT_PATH_WRITE_NODE => Some(p.as_constant_path_write_node().unwrap().target()),
                kind::CONSTANT_PATH_OPERATOR_WRITE_NODE => Some(p.as_constant_path_operator_write_node().unwrap().target()),
                kind::CONSTANT_PATH_OR_WRITE_NODE => Some(p.as_constant_path_or_write_node().unwrap().target()),
                kind::CONSTANT_PATH_AND_WRITE_NODE => Some(p.as_constant_path_and_write_node().unwrap().target()),
                _ => None,
            };
            target.is_some_and(|t| (t.location().start_offset(), t.location().end_offset()) == span)
        });
        if is_casgn_target {
            return;
        }
        let source = node_text(node);
        let constant = source.strip_prefix("::").unwrap_or(source);
        let Some(Value::Map(entries)) = cx.cfg.get("DeprecatedConstants") else { return };
        let Some(Value::Map(deprecated)) = entries.get(constant) else { return };
        let alternative = as_str(deprecated.get("Alternative"));
        let version = as_str(deprecated.get("DeprecatedVersion"));
        let version_f = version.as_deref().map_or(0.0, ruby_to_f);
        if cx.config.target_ruby < version_f {
            return;
        }
        let deprecated_message = version.map(|v| format!(", deprecated since Ruby {v}")).unwrap_or_default();
        let msg = match &alternative {
            Some(good) => format!("Use `{good}` instead of `{source}`{deprecated_message}."),
            None => format!("Do not use `{source}`{deprecated_message}."),
        };
        let (start, end) = span;
        // corrector.replace(node, nil) makes no edit: not [Correctable] without an Alternative.
        cx.add_fix(start, end, msg, |c| {
            if let Some(alternative) = &alternative {
                c.replace(start, end, alternative);
            }
        });
    }
}

/// Ruby `String#to_f`: the leading float prefix, 0.0 when there is none.
fn ruby_to_f(s: &str) -> f64 {
    let s = s.trim_start();
    let end = s
        .char_indices()
        .take_while(|&(i, c)| c.is_ascii_digit() || c == '.' || (i == 0 && (c == '-' || c == '+')))
        .count();
    s[..end].trim_end_matches('.').parse().unwrap_or(0.0)
}
