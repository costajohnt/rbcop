use crate::config::value::Value;
use crate::directives;
use crate::helpers::cop_registry::{is_department, is_known};
use crate::prelude::*;

pub const NAME: &str = "Lint/MissingCopEnableDirective";
pub static COP: MissingCopEnableDirective = MissingCopEnableDirective;
pub struct MissingCopEnableDirective;

/// Lines as Ruby sees them: `i64::MIN`/`i64::MAX` are the infinite range ends.
fn as_f64(line: i64) -> f64 {
    match line {
        i64::MIN => f64::NEG_INFINITY,
        i64::MAX => f64::INFINITY,
        l => l as f64,
    }
}

impl Cop for MissingCopEnableDirective {
    fn on_file(&self, cx: &mut Cx) {
        // processed_source.disabled_line_ranges analyzes the comment config.
        crate::runner::comment_config_read();
        let src = cx.src;
        let cc = &cx.facts.comment_config;
        let max_range = match cx.cfg.get("MaxRangeSize") {
            Some(Value::Int(i)) => *i as f64,
            Some(Value::Float(f)) => *f,
            _ => f64::INFINITY,
        };
        let mut found = Vec::new();
        for cop in &cc.order {
            let Some(ranges) = cc.ranges.get(cop) else { continue };
            for r in ranges {
                let directive = r.2.and_then(|i| directives::parse(src.slice(src.comments[i].start, src.comments[i].end)));
                // A `disable-next` or `next` scope closes itself with its statement.
                if directive.as_ref().is_some_and(|d| matches!(d.mode.as_str(), "disable-next" | "todo-next" | "next")) {
                    continue;
                }
                // acceptable_range?
                let (min, max) = (as_f64(r.0), as_f64(r.1));
                if max - min < max_range + 2.0 || !is_known(cop) {
                    continue;
                }
                // expected_open_range?
                if r.0 == i64::MIN || (!cx.config.cop_enabled(cop) && r.1 == i64::MAX) {
                    continue;
                }
                // opening_comment: the range's directive (a disable suspended by `enable-next` or
                // `next +Cop` reopens below the statement, on a code line), else
                // processed_source.comment_at_line(line_range.begin); RuboCop raises on nil.
                let opening = r.2.map(|i| &src.comments[i]);
                let Some(c) = opening.or_else(|| src.comments.iter().rev().find(|c| src.line_of(c.start) as i64 == r.0)) else {
                    crate::runner::cop_error(NAME, src, 0);
                    return emit(cx, found);
                };
                let text = src.slice(c.start, c.end);
                let d = directives::parse(text);
                let raw: Vec<&str> = d.as_ref().map(|d| d.raw_names()).unwrap_or_default();
                let department = raw.iter().any(|n| is_department(n) && cop.starts_with(n));
                let (name, kind) =
                    if department { (cop.split('/').next().unwrap_or(""), "department") } else { (cop.as_str(), "cop") };
                let msg = if max_range == f64::INFINITY {
                    let directive =
                        if d.as_ref().is_some_and(|d| d.mode == "push") { "# rubocop:pop" } else { "# rubocop:enable" };
                    format!("Re-enable {name} {kind} with `{directive}` after disabling it.")
                } else {
                    let max = match cx.cfg.get("MaxRangeSize") {
                        Some(Value::Int(i)) => i.to_string(),
                        _ => helpers::ruby_string::float_to_s(max_range),
                    };
                    format!("Re-enable {name} {kind} within {max} lines after disabling it.")
                };
                found.push((c.start, c.end, msg));
            }
        }
        emit(cx, found);
    }
}

fn emit(cx: &mut Cx, found: Vec<(usize, usize, String)>) {
    for (s, e, msg) in found {
        cx.add(s, e, msg);
    }
}
