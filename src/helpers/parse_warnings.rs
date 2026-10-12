//! Parser warnings (`processed_source.diagnostics`), which the runner does not keep: the cops that
//! need them re-parse the source. Prism's warning types map to parser reasons as in
//! Prism::Translation::Parser#warning_diagnostic.

use crate::source::Source;

#[derive(Clone)]
pub struct Warning {
    /// Prism warning type, e.g. `ambiguous_first_argument_plus`.
    pub kind: &'static str,
    pub start: usize,
    pub end: usize,
}

/// The file's parse warnings, in Prism's order: the runner's (see `from_result`), or from one
/// extra parse shared by the cops that ask.
pub fn warnings(src: &Source) -> Vec<Warning> {
    src.parse_warnings.get_or_init(|| from_result(&parse(src))).clone()
}

/// The parse `warnings` needs: `src.text` as is, with the file's version and encoding.
fn parse<'a>(src: &Source<'a>) -> ruby_prism::ParseResult<'a> {
    ruby_prism::parse_with_version(src.text.as_bytes(), src.version, !src.binary)
}

pub fn from_result(result: &ruby_prism::ParseResult) -> Vec<Warning> {
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
