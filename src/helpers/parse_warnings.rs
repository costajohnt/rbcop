//! Parser warnings (`processed_source.diagnostics`), which the runner does not keep: the cops that
//! need them re-parse the source. Prism's warning types map to parser reasons as in
//! Prism::Translation::Parser#warning_diagnostic.

use crate::source::Source;

pub struct Warning {
    /// Prism warning type, e.g. `ambiguous_first_argument_plus`.
    pub kind: &'static str,
    pub start: usize,
    pub end: usize,
}

/// The file's parse warnings, in Prism's order.
/// ponytail: one extra parse per cop and file; keep the runner's ParseResult if more cops need this.
pub fn warnings(src: &Source) -> Vec<Warning> {
    let result = ruby_prism::parse_with_version(src.text.as_bytes(), src.version, !src.binary);
    result
        .warnings()
        .map(|w| {
            let loc = w.location();
            Warning { kind: w.type_name(), start: loc.start_offset(), end: loc.end_offset() }
        })
        .collect()
}

/// `diagnostic.reason == :ambiguous_prefix`: the prefix operator, if so.
pub fn ambiguous_prefix(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "ambiguous_first_argument_plus" => "+",
        "ambiguous_first_argument_minus" => "-",
        "ambiguous_prefix_ampersand" => "&",
        "ambiguous_prefix_star" => "*",
        "ambiguous_prefix_star_star" => "**",
        _ => return None,
    })
}
