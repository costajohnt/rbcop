use crate::helpers::format_string::{format_sequences, sequence_parts};
use crate::helpers::parser_parent::parser_parent;
use crate::prelude::*;

pub const NAME: &str = "Style/FormatStringToken";
pub static COP: FormatStringToken = FormatStringToken;
pub struct FormatStringToken;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Style {
    Annotated,
    Template,
    Unannotated,
}

fn message_text(style: Style) -> &'static str {
    match style {
        Style::Annotated => "annotated tokens (like `%<foo>s`)",
        Style::Template => "template tokens (like `%{foo}`)",
        Style::Unannotated => "unannotated tokens (like `%s`)",
    }
}

/// `format_string_in_typical_context?` for the node at `node` whose Prism ancestors are `ancestors`.
fn typical(node: &Node, ancestors: &[Node]) -> bool {
    let Some(call) = parser_parent(ancestors).and_then(|p| p.as_call_node()) else { return false };
    if is_csend(&call) {
        return false;
    }
    let start = node.location().start_offset();
    let args = send_args(&call);
    match method_name(&call) {
        "format" | "sprintf" | "printf" => args.first().is_some_and(|a| a.location().start_offset() == start),
        "%" => call.receiver().is_some_and(|r| r.location().start_offset() == start) && args.len() == 1,
        _ => false,
    }
}

impl Cop for FormatStringToken {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::STRING_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let string = node.as_string_node().unwrap();
        let ancestors = cx.ancestors;
        if ancestors.iter().any(|a| {
            matches!(
                kind::of(a),
                kind::X_STRING_NODE
                    | kind::INTERPOLATED_X_STRING_NODE
                    | kind::REGULAR_EXPRESSION_NODE
                    | kind::INTERPOLATED_REGULAR_EXPRESSION_NODE
                    | kind::MATCH_LAST_LINE_NODE
                    | kind::INTERPOLATED_MATCH_LAST_LINE_NODE
            )
        }) {
            return;
        }
        // use_allowed_method?: the innermost parser `send` ancestor.
        let send_parent = (0..ancestors.len()).rev().find_map(|i| {
            let c = ancestors[i].as_call_node()?;
            let in_block = ancestors.get(i + 1).is_some_and(|n| n.as_block_node().is_some());
            (!in_block && !is_csend(&c)).then_some(c)
        });
        if let Some(s) = send_parent {
            let name = method_name(&s);
            if helpers::allowed::allowed_method(cx.cfg, name) || helpers::allowed::matches_allowed_pattern(cx.cfg, name) {
                return;
            }
        }

        let style = match cx.cfg.str("EnforcedStyle") {
            Some("template") => Style::Template,
            Some("unannotated") => Style::Unannotated,
            _ => Style::Annotated,
        };
        let conservative = cx.cfg.str("Mode") == Some("conservative");
        let correctable_seq = |ty: Option<&str>| ty == Some("s") || style != Style::Template;

        // Parser makes a multi-line string a `dstr` of one `str` per line; a part of an
        // interpolated string is a `str` whose parent is the `dstr`.
        let split = is_parser_dstr(node);
        let node_typical = typical(node, ancestors);
        let self_typical = !split && node_typical;
        // A split string's pieces have the string itself as their `dstr` ancestor.
        let context = node_typical
            || (0..ancestors.len()).any(|i| {
                ancestors[i].as_interpolated_string_node().is_some() && typical(&ancestors[i], &ancestors[..i])
            });

        let content = string.content_loc();
        let base = content.start_offset();
        let text = std::str::from_utf8(content.as_slice()).unwrap_or("");
        let mut offset = 0;
        for piece in text.split_inclusive('\n') {
            let piece_start = base + offset;
            offset += piece.len();
            if !piece.contains('%') {
                continue;
            }
            let mut detections = Vec::new();
            for seq in format_sequences(piece) {
                if seq.percent {
                    continue;
                }
                let detected = if seq.name.is_some() && seq.source.contains('<') {
                    Style::Annotated
                } else if seq.name.is_some() && seq.source.contains('{') {
                    Style::Template
                } else {
                    Style::Unannotated
                };
                if (detected == Style::Unannotated || conservative) && !self_typical {
                    continue;
                }
                detections.push((seq, detected));
            }
            if detections.is_empty() {
                continue;
            }
            if detections.iter().all(|(_, d)| *d == Style::Unannotated)
                && (detections.len() as i64 <= cx.cfg.int("MaxUnannotatedPlaceholdersAllowed").unwrap_or(1)
                    || detections.iter().any(|(s, _)| !correctable_seq(sequence_parts(s.source).ty)))
            {
                continue;
            }
            for (seq, detected) in detections {
                let parts = sequence_parts(seq.source);
                if detected == style || !correctable_seq(parts.ty) {
                    continue;
                }
                let (start, end) = (piece_start + seq.begin, piece_start + seq.end);
                let msg = format!("Prefer {} over {}.", message_text(style), message_text(detected));
                if !context {
                    cx.add_with(start, end, msg, false);
                    continue;
                }
                let correction = parts.name.filter(|_| style != Style::Unannotated).map(|name| {
                    let (flags, width, precision) = (&parts.flags, parts.width.unwrap_or(""), parts.precision.unwrap_or(""));
                    let ty = if detected == Style::Template { "s" } else { parts.ty.unwrap_or("") };
                    if style == Style::Annotated {
                        format!("%<{name}>{flags}{width}{precision}{ty}")
                    } else {
                        format!("%{flags}{width}{precision}{{{name}}}")
                    }
                });
                cx.add_fix(start, end, &msg, |c| {
                    if let Some(correction) = &correction {
                        c.replace(start, end, correction);
                    }
                });
            }
        }
    }
}
