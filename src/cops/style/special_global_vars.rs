use crate::prelude::*;
use helpers::whole_lines::range_by_whole_lines;

pub const NAME: &str = "Style/SpecialGlobalVars";
pub static COP: SpecialGlobalVars = SpecialGlobalVars;
pub struct SpecialGlobalVars;

/// ENGLISH_VARS before its merge: Perl name to English names.
const VARS: &[(&str, &[&str])] = &[
    ("$:", &["$LOAD_PATH"]),
    ("$\"", &["$LOADED_FEATURES"]),
    ("$0", &["$PROGRAM_NAME"]),
    ("$!", &["$ERROR_INFO"]),
    ("$@", &["$ERROR_POSITION"]),
    ("$;", &["$FIELD_SEPARATOR", "$FS"]),
    ("$,", &["$OUTPUT_FIELD_SEPARATOR", "$OFS"]),
    ("$/", &["$INPUT_RECORD_SEPARATOR", "$RS"]),
    ("$\\", &["$OUTPUT_RECORD_SEPARATOR", "$ORS"]),
    ("$.", &["$INPUT_LINE_NUMBER", "$NR"]),
    ("$_", &["$LAST_READ_LINE"]),
    ("$>", &["$DEFAULT_OUTPUT"]),
    ("$<", &["$DEFAULT_INPUT"]),
    ("$$", &["$PROCESS_ID", "$PID"]),
    ("$?", &["$CHILD_STATUS"]),
    ("$~", &["$LAST_MATCH_INFO"]),
    ("$=", &["$IGNORECASE"]),
    ("$*", &["$ARGV", "ARGV"]),
];

const NON_ENGLISH: &[&str] = &["$LOAD_PATH", "$LOADED_FEATURES", "$PROGRAM_NAME", "ARGV"];

#[derive(Clone, Copy, PartialEq)]
enum Style {
    English,
    Perl,
    Builtin,
}

/// `STYLE_VARS_MAP[style][global]`.
fn preferred(style: Style, g: &str) -> Option<Vec<&'static str>> {
    for &(perl, english) in VARS {
        if g == perl {
            return Some(match style {
                Style::English => english.to_vec(),
                Style::Builtin if NON_ENGLISH.contains(&english[0]) => vec![english[0]],
                _ => vec![perl],
            });
        }
        if let Some(&e) = english.iter().find(|&&e| e == g) {
            return Some(match style {
                Style::English => vec![e],
                Style::Builtin if NON_ENGLISH.contains(&e) => vec![e],
                _ => vec![perl],
            });
        }
    }
    None
}

fn message(style: Style, g: &str, preferred: &[&str]) -> String {
    if style != Style::English {
        return format!("Prefer `{}` over `{g}`.", preferred[0]);
    }
    let (regular, english): (Vec<&str>, Vec<&str>) = preferred.iter().partition(|v| NON_ENGLISH.contains(v));
    let (english, regular) = (english.join("` or `"), regular.join("` or `"));
    if regular.is_empty() {
        format!("Prefer `{english}` from the stdlib 'English' module (don't forget to require it) over `{g}`.")
    } else if english.is_empty() {
        format!("Prefer `{regular}` over `{g}`.")
    } else {
        format!(
            "Prefer `{english}` from the stdlib 'English' module (don't forget to require it) or `{regular}` over `{g}`."
        )
    }
}

/// `(send {(const {nil? cbase} :Kernel) nil?} :require (str "English"))`
fn requires_english(node: &Node) -> bool {
    let Some(call) = node.as_call_node() else { return false };
    if method_name(&call) != "require" || call.block().is_some() {
        return false;
    }
    if call.receiver().is_some_and(|r| !helpers::parser_parent::is_top_const(&r, "Kernel")) {
        return false;
    }
    let args = call_args(&call);
    args.len() == 1
        && !is_parser_dstr(&args[0])
        && args[0].as_string_node().is_some_and(|s| s.unescaped() == b"English")
}

#[derive(Default)]
struct State {
    required_english: bool,
}

impl Cop for SpecialGlobalVars {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::GLOBAL_VARIABLE_READ_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let g = node_text(node);
        let style = match cx.cfg.str("EnforcedStyle") {
            Some("use_perl_names") => Style::Perl,
            Some("use_builtin_english_names") => Style::Builtin,
            _ => Style::English,
        };
        let Some(pref) = preferred(style, g) else { return };
        if pref.contains(&g) {
            return cx.correct_style_detected();
        }
        if crate::autogen::on() {
            // matching_styles: the styles that prefer this name for some variable.
            let names: Vec<&str> = VARS.iter().flat_map(|(perl, english)| std::iter::once(*perl).chain(english.iter().copied())).collect();
            let matching: Vec<&str> = [
                (Style::English, "use_english_names"),
                (Style::Perl, "use_perl_names"),
                (Style::Builtin, "use_builtin_english_names"),
            ]
            .into_iter()
            .filter(|(s, _)| names.iter().any(|n| preferred(*s, n).is_some_and(|p| p.contains(&g))))
            .map(|(_, name)| name)
            .collect();
            cx.style_detected(&matching);
        }
        let msg = message(style, g, &pref);
        let loc = node.location();
        let (start, end) = (loc.start_offset(), loc.end_offset());

        // `node = node.parent while node.parent&.begin_type? && node.parent.children.one?`, then
        // the parser type of the parent.
        let anc = cx.ancestors;
        let (mut rs, mut re) = (start, end);
        let mut climbed = false;
        let mut parent_kind = None;
        for a in anc.iter().rev() {
            let k = kind::of(a);
            let one = |s: Option<ruby_prism::StatementsNode>| s.is_some_and(|s| s.body().iter().count() == 1);
            match k {
                kind::EMBEDDED_VARIABLE_NODE => {}
                kind::STATEMENTS_NODE if one(a.as_statements_node()) => {}
                kind::PARENTHESES_NODE if one(a.as_parentheses_node().unwrap().body().and_then(|b| b.as_statements_node())) => {
                    (rs, re) = (a.location().start_offset(), a.location().end_offset());
                    climbed = true;
                }
                kind::EMBEDDED_STATEMENTS_NODE if one(a.as_embedded_statements_node().unwrap().statements()) => {
                    (rs, re) = (a.location().start_offset(), a.location().end_offset());
                    climbed = true;
                }
                _ => {
                    parent_kind = Some(k);
                    break;
                }
            }
        }
        let interpolated = matches!(
            parent_kind,
            Some(kind::INTERPOLATED_STRING_NODE | kind::INTERPOLATED_X_STRING_NODE | kind::INTERPOLATED_REGULAR_EXPRESSION_NODE)
        );
        let replacement = if !interpolated {
            pref[0].to_string()
        } else if style == Style::English {
            if climbed { format!("#{{{}}}", pref[0]) } else { format!("{{{}}}", pref[0]) }
        } else {
            format!("#{}", pref[0])
        };

        // RequireLibrary#ensure_required.
        let require_english = style == Style::English
            && cx.cfg.bool("RequireEnglish") == Some(true)
            && !cx.state::<State>().required_english
            && !NON_ENGLISH.contains(&pref[0]);
        let mut insert: Option<(usize, usize)> = None;
        let mut removals = vec![];
        if require_english {
            let stmts: Vec<Node> = anc
                .first()
                .and_then(|p| p.as_program_node())
                .map(|p| p.statements().body().iter().collect())
                .unwrap_or_default();
            let range = |n: &Node| (n.location().start_offset(), n.location().end_offset());
            let top = stmts.iter().position(|s| range(s).0 <= start && end <= range(s).1);
            let mut skip = false;
            if stmts.len() > 1
                && let Some(top) = top
            {
                skip = stmts[..top].iter().any(requires_english);
                if !skip {
                    for s in &stmts[top + 1..] {
                        if requires_english(s) {
                            let (a, b) = range(s);
                            removals.push(range_by_whole_lines(cx.src.text, a, b, true));
                        }
                    }
                }
            }
            if !skip {
                insert = Some(stmts.first().map(range).unwrap_or((start, end)));
            }
        }
        let mut required = false;
        cx.add_fix(start, end, msg, |c| {
            if require_english {
                for &(a, b) in &removals {
                    c.remove(a, b);
                }
                if let Some((a, b)) = insert {
                    c.insert_before(a, b, "require 'English'\n");
                }
                required = true;
            }
            c.replace(rs, re, &replacement);
        });
        if required {
            cx.state::<State>().required_english = true;
        }
    }
}
