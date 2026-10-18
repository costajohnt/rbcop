//! Run the enabled cops over one file: parse once, walk once, dispatch nodes by kind.

use crate::autocorrect::{self, Mode, corrector::Corrector};
use crate::config::{CopConfig, Resolved, target_ruby};
use crate::cop::{Cop, Cx, Facts, Fixes, Heredoc, Offense, Severity};
use crate::directives;
use crate::helpers::comment_config::{StatementBounds, code_lines};
use crate::node_kind;
use crate::source::{Comment, Source};
use ruby_prism::{
    ArgumentsNode, BlockArgumentNode, BlockNode, BlockParameterNode, CallNode, ConstantPathNode, ElseNode, EnsureNode,
    InterpolatedStringNode, LocalVariableTargetNode, Node, ParametersNode, RegularExpressionNode, RescueNode,
    SplatNode, StatementsNode, StringNode, Visit, XStringNode,
};
use std::path::Path;
use std::sync::Arc;

/// What happened to one file.
pub struct FileReport {
    pub offenses: Vec<Offense>,
    /// Source text (after CRLF normalisation), kept for formatters that print code.
    pub text: String,
    /// Warnings printed when the file is reported (`no department given` for directive names).
    pub warnings: Vec<String>,
    /// The error that ends the run at this file, before it is reported (see `poison_raise`).
    pub abort: Option<String>,
}

impl FileReport {
    /// The source an offense's offsets refer to (autocorrect keeps each iteration's source).
    pub fn text_of<'a>(&'a self, o: &'a Offense) -> &'a str {
        o.source.as_deref().unwrap_or(&self.text)
    }
}

struct Active<'c> {
    name: &'static str,
    cop: &'static dyn Cop,
    cfg: &'c CopConfig,
}

/// Inspect one file. `selected` filters cop names (from --only / --except).
pub fn inspect(
    path: &Path,
    display: &str,
    text: String,
    cfg: &Resolved,
    selected: &dyn Fn(&str) -> bool,
) -> FileReport {
    // parser's Source::Buffer normalises CRLF before any cop sees the source; the raw text stays
    // available (ProcessedSource#raw_source) for cops such as Layout/EndOfLine.
    let (text, raw) = if text.contains("\r\n") { (text.replace("\r\n", "\n"), Some(text)) } else { (text, None) };
    let shared: Arc<str> = Arc::from(text.as_str());
    directives::take_warnings();
    take_abort();
    let (offenses, _, _) = analyze(path, display, &shared, raw.as_deref(), cfg, selected, None, Redundant::Own);
    FileReport { offenses, text, warnings: directives::take_warnings(), abort: take_abort() }
}

/// Inspect a file's bytes as read. A source that must be UTF-8 (`source::must_be_utf8`) but is not
/// gets only RuboCop's Lint/Syntax `Invalid byte sequence in utf-8.` (a US-ASCII one with a byte
/// above 0x7f its transcoding error, `source::encoding_error`); other non-UTF-8 sources are
/// inspected lossily.
pub fn inspect_bytes(
    path: &Path,
    display: &str,
    bytes: Vec<u8>,
    cfg: &Resolved,
    selected: &dyn Fn(&str) -> bool,
) -> FileReport {
    // Team#roundup_relevant_cops drops an excluded Lint/Syntax too: a project config cannot
    // exclude it, but a plugin's default.yml can (rubocop-erb's `**/*.erb`).
    let selected = &|cop: &str| selected(cop) && (cop != "Lint/Syntax" || cfg.relevant(cfg.cop(cop), path));
    let error = crate::source::encoding_error(&bytes);
    match String::from_utf8(bytes) {
        Ok(text) => match error {
            Some(msg) => encoding_error_report(text, msg, selected),
            None => inspect(path, display, text, cfg, selected),
        },
        Err(e) => {
            let text = String::from_utf8_lossy(e.as_bytes()).into_owned();
            if let Some(msg) = error {
                return encoding_error_report(text, msg, selected);
            }
            if !crate::source::must_be_utf8(&text) {
                return inspect(path, display, text, cfg, selected);
            }
            encoding_error_report(text, "Invalid byte sequence in utf-8.".into(), selected)
        }
    }
}

/// Lint/Syntax#add_offense_from_error: a global offense, beautified message, no version note.
fn encoding_error_report(text: String, message: String, selected: &dyn Fn(&str) -> bool) -> FileReport {
    // Lint/Syntax is not mobilized under -x, --only-guide-cops or `StyleGuideCopsOnly`.
    if !selected("Lint/Syntax") {
        return FileReport { offenses: Vec::new(), text, warnings: Vec::new(), abort: None };
    }
    let offense = Offense {
        start: 0,
        end: 0,
        cop: "Lint/Syntax",
        message,
        severity: Severity::Fatal,
        correctable: false,
        global: true,
        corrected: false,
        source: None,
    };
    FileReport { offenses: vec![offense], text, warnings: Vec::new(), abort: None }
}

/// How Ruby words a failed `File.read`/`File.open` of `path` (RuboCop raises its own error for a
/// missing file).
pub fn io_error(e: &std::io::Error, path: &Path) -> String {
    if e.kind() == std::io::ErrorKind::NotFound {
        return format!("Error: No such file or directory: {}", path.display());
    }
    let msg = e.to_string();
    let msg = msg.split(" (os error").next().unwrap_or(&msg);
    format!("{msg} @ rb_sysopen - {}", path.display())
}

/// One autocorrect iteration (see `crate::autocorrect::correct`): the offenses and, unless nothing
/// was corrected, the collated corrector. Like Team#investigate_with_corrector, offenses of cops
/// that do not autocorrect are only reported when no correction was made. Also returns the
/// (cop, first line, last line) of the offenses disabled by comments, which RuboCop keeps for
/// Lint/RedundantCopDisableDirective.
pub fn analyze_for_correction(
    path: &Path,
    display: &str,
    text: &Arc<str>,
    raw_text: Option<&str>,
    cfg: &Resolved,
    selected: &dyn Fn(&str) -> bool,
    mode: Mode,
) -> (Vec<Offense>, Option<Corrector>, Vec<Span>) {
    analyze(path, display, text, raw_text, cfg, selected, Some(mode), Redundant::Skip)
}

/// Runner#add_redundant_disables after an autocorrect loop: Lint/RedundantCopDisableDirective
/// alone, judging the directives of `text` against `checked` (every offense the loop found).
pub fn redundant_disables(
    path: &Path,
    display: &str,
    text: &Arc<str>,
    cfg: &Resolved,
    selected: &dyn Fn(&str) -> bool,
    mode: Mode,
    checked: &[Span],
) -> (Vec<Offense>, Option<Corrector>) {
    let (offenses, corrector, _) =
        analyze(path, display, text, None, cfg, selected, Some(mode), Redundant::Only(checked));
    (offenses, corrector)
}

/// An offense as Lint/RedundantCopDisableDirective sees it: cop, first line, last line.
pub type Span = (&'static str, usize, usize);

/// When Lint/RedundantCopDisableDirective runs (it needs every other cop's offenses).
#[derive(Clone, Copy)]
enum Redundant<'a> {
    /// After this file's cops (plain inspection).
    Own,
    /// Not in this pass (autocorrect iterations).
    Skip,
    /// Only it, against offenses found earlier.
    Only(&'a [Span]),
}

const REDUNDANT_DISABLE: &str = "Lint/RedundantCopDisableDirective";

/// Bytes a rewritten copy changed: (offset, new byte).
pub type ByteEdits = Vec<(usize, u8)>;

/// The parse of `text` that cops see and syntax checks judge, with the lexer's tokens (below 3.3),
/// the offsets of the comment lines the parse added (to drop from the file's comments), and the
/// bytes the parsed copy changed besides those (`Source::lex_tokens` lexes the same copy).
/// `analyze` and the reparse checks (`reparsed_equivalence`) share it; `buf` holds a rewritten copy.
pub fn parse_for_target<'t>(
    text: &'t str,
    buf: &'t mut Vec<u8>,
    version: &'static str,
    target_ruby: f64,
) -> (ruby_prism::ParseResult<'t>, Vec<ruby_prism::Token>, Vec<usize>, ByteEdits) {
    let utf8 = !crate::source::binary_encoding(text);
    // Below 3.3 RuboCop parses with the parser gem, which accepts some things Prism rejects.
    if target_ruby >= 3.3 {
        return (ruby_prism::parse_with_version(text.as_bytes(), version, utf8), Vec::new(), Vec::new(), Vec::new());
    }
    let (mut result, mut tokens) = ruby_prism::parse_with_version_and_tokens(text.as_bytes(), version, utf8);
    let mut errors = result.errors().next().is_some();
    // Rewrites of a copy for Prism, keeping every offset (and valid UTF-8); the original bytes are
    // written back once parsed, so node text is the user's text (#169).
    let mut copy: Option<Vec<u8>> = None;
    let mut edits: Vec<(usize, u8)> = Vec::new();
    let mut stand_ins: Vec<(char, Option<char>)> = Vec::new();
    let mut present: Option<std::collections::HashSet<char>> = None;
    // Heredoc terminators the copy has Prism find past their indentation: (found, line start).
    let mut heredoc_ends: Vec<(usize, usize)> = Vec::new();
    // Each rewrite is found in the tokens of the copy so far, which one rewrite can change for
    // another (a heredoc a NUL closes ends before an indented `__END__` or a constant, #306): parse
    // the copy again until no rewrite is left.
    // ponytail: bounded at 8 rounds; each round removes what it rewrites, so real files need 1-2.
    for _ in 0..8 {
        let cur = copy.as_deref().map_or(text, |c| std::str::from_utf8(c).unwrap_or(text));
        let mut round: Vec<(usize, u8)> = Vec::new();
        // The parser gem's lexer starts a constant only at ASCII `A`-`Z` (`Ж = 1` is a local): such
        // a CONSTANT's first character becomes, for Prism, a lowercase stand-in the file does not use
        // (#219, #259); each capital keeps its own (or none, when every candidate is in the file).
        for t in tokens.iter().filter(|t| t.kind == "CONSTANT") {
            let Some(c) = crate::legacy_syntax::non_ascii_constant_start(&cur[t.start..]) else { continue };
            let l = match stand_ins.iter().find(|&&(k, _)| k == c) {
                Some(&(_, l)) => l,
                None => {
                    let present = present.get_or_insert_with(|| text.chars().collect());
                    let l = crate::legacy_syntax::stand_in(c, present, &stand_ins);
                    stand_ins.push((c, l));
                    l
                }
            };
            let Some(l) = l else { continue };
            let mut enc = [0u8; 4];
            round.extend((t.start..).zip(l.encode_utf8(&mut enc).bytes()));
        }
        // An indented `__END__` ends the input for the parser gem (#228), also before a NUL, ^D or ^Z
        // (#505): the copy has it start its line (`__END__\n` + the indentation, same length, cut at
        // the end of the file).
        if cur.contains("__END__")
            && let Some((line, indent)) = crate::legacy_syntax::indented_data_marker(cur, &tokens)
        {
            round.extend((line..cur.len()).zip(b"__END__\n".iter().copied().chain(std::iter::repeat_n(b' ', indent))));
        }
        // Embedded documents the parser gem's lexer starts or ends where Prism does not (after a
        // BOM, #512; indented; `=beginx`, `=endx`).
        if cur.contains("=begin") {
            round.extend(crate::legacy_syntax::embdoc_fixups(cur, text, &tokens));
        }
        // A column-0 `__END__` the parser gem reads as an identifier (after `=`, `(`, `,`, `.`, a
        // `\` continuation, ...) is an identifier the file does not use, so Prism lexes on (#337).
        if cur.contains("__END__")
            && let Some(at) = crate::legacy_syntax::identifier_data_marker(cur, &tokens)
            && let Some(name) = crate::legacy_syntax::end_stand_in(text)
        {
            round.extend((at..).zip(name.bytes()));
        }
        // `def self&.foo` is a singleton method for the parser gem: the copy has `def self .foo` (#492).
        if errors {
            round.extend(crate::legacy_syntax::defs_and_dots(cur, &tokens, target_ruby).into_iter().map(|o| (o, b' ')));
        }
        // A NUL, ^D or ^Z ends a heredoc line for the parser gem's lexer: those that make a
        // terminator line become newlines (#247, #543), also where a later terminator closes the
        // heredoc for Prism (#544).
        if cur.contains(['\0', '\x04', '\x1a']) {
            let breaks = crate::legacy_syntax::heredoc_nul_breaks(cur, &tokens, !errors);
            round.extend(breaks.into_iter().map(|o| (o, b'\n')));
        }
        // The parser gem ends a plain heredoc at its delimiter after spaces and tabs (a `<<''` at a
        // blank line), Prism only at the delimiter alone (#318, #355): see plain_heredoc_terminator.
        // A heredoc after another on its line starts where the parser gem ends that one: Prism's
        // tokens do not show it when Prism's body runs on (#357), so each body is scanned once.
        let mut chain: Option<(usize, usize)> = None;
        for (i, t) in tokens.iter().enumerate().filter(|(_, t)| t.kind == "HEREDOC_START") {
            let line_end = cur[t.end..].find('\n').map_or(cur.len(), |n| t.end + n);
            let body = chain.filter(|c| c.0 == line_end).map(|c| c.1);
            chain = None;
            if let Some(h) = crate::legacy_syntax::plain_heredoc_terminator(cur, &tokens, i, body) {
                chain = Some((line_end, h.after));
                round.extend(h.edits);
                heredoc_ends.extend(h.moved);
            }
        }
        round.retain(|&(o, b)| cur.as_bytes()[o] != b);
        if round.is_empty() {
            break;
        }
        let bytes = copy.get_or_insert_with(|| text.as_bytes().to_vec());
        for &(o, b) in &round {
            bytes[o] = b;
        }
        edits.extend(round);
        let (r, t) = ruby_prism::parse_with_version_and_tokens(bytes, version, utf8);
        (errors, tokens) = (r.errors().next().is_some(), t);
    }
    let mut restore: Vec<(usize, u8)> = edits.iter().map(|&(o, _)| (o, text.as_bytes()[o])).collect();
    let mut fake = Vec::new();
    // 2.7 to 3.2: the parser gem continues a call over whitespace-only lines before a leading
    // `.`/`&.`, where Prism ends the expression. Parse with those lines made comment lines instead.
    if target_ruby >= 2.7 && errors {
        let dots: Vec<usize> =
            tokens.iter().filter(|t| matches!(t.kind, "DOT" | "AMPERSAND_DOT")).map(|t| t.start).collect();
        let current = copy.as_deref().map_or(text, |c| std::str::from_utf8(c).unwrap_or(text));
        if let Some((rewritten, added)) = crate::legacy_syntax::continue_over_blank_lines(current, &dots) {
            restore.extend(added.iter().map(|&f| (f, text.as_bytes()[f])));
            copy = Some(rewritten);
            fake = added;
        }
    }
    if let Some(c) = copy {
        *buf = c;
        (result, tokens) = ruby_prism::parse_with_version_and_tokens_restoring(buf, version, utf8, &restore);
        if !heredoc_ends.is_empty() {
            heredoc_ends.sort_unstable();
            result.move_heredoc_ends(&heredoc_ends);
            for t in tokens.iter_mut().filter(|t| matches!(t.kind, "HEREDOC_END" | "STRING_CONTENT")) {
                for o in [&mut t.start, &mut t.end] {
                    if let Ok(i) = heredoc_ends.binary_search_by_key(&*o, |e| e.0) {
                        *o = heredoc_ends[i].1;
                    }
                }
            }
        }
    }
    // A delimiter Prism lexes byte-wise (`%<U+00A0>q(`) splits a character between two tokens:
    // move the split to the character's end so every token slices the text (#250).
    let ceil = |mut o: usize| {
        while !text.is_char_boundary(o) {
            o += 1;
        }
        o
    };
    for t in &mut tokens {
        (t.start, t.end) = (ceil(t.start), ceil(t.end));
    }
    // Numbered parameters are 2.7+: below that `foo { _1 }` is a block without arguments.
    if target_ruby < 2.7 {
        result.drop_numbered_parameters(None);
    }
    // Below 3.0 `_1 = 1` declares a local whose reads Prism gives as calls (#364), or (2.7+, in a
    // block) as numbered parameters. From 3.0 only a named capture or some pattern variables do
    // (#533), and Prism's reserved-name errors there go.
    if text.as_bytes().windows(2).any(|w| w[0] == b'_' && matches!(w[1], b'1'..=b'9')) {
        let accept = (target_ruby >= 3.0).then(|| crate::syntax::accepted_numbered_targets(&result.node()));
        if let Some(a) = accept.as_ref().filter(|a| !a.is_empty()) {
            result.drop_errors("parameter_numbered_reserved", a);
        }
        if accept.as_ref().is_none_or(|a| !a.is_empty()) {
            let (reads, blocks, numblocks) = crate::syntax::assigned_numbered_reads(&result.node(), accept);
            if !reads.is_empty() {
                result.variable_calls_as_locals(&reads);
            }
            if target_ruby >= 2.7 && !blocks.is_empty() {
                result.drop_numbered_parameters(Some(&blocks));
                result.add_numbered_parameters(&numblocks);
            }
        }
    }
    (result, tokens, fake, edits)
}

/// rubocop-rails MigrationFileSkippable (prepended to every cop's add_offense once the gem loads):
/// a file whose name holds a 14-digit timestamp at or before `AllCops: MigratedSchemaVersion` gets
/// no offenses (and so no corrections) from any cop.
fn already_migrated_file(path: &Path, cfg: &Resolved) -> bool {
    use crate::config::value::Value;
    let version = match cfg.all_cops().get("MigratedSchemaVersion") {
        None | Some(Value::Null) => return false,
        Some(Value::Str(s)) => s.clone(),
        Some(Value::Int(i)) => i.to_string(),
        Some(v) => v.as_string().unwrap_or_default(),
    };
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let bytes = name.as_bytes();
    let stamp = (0..bytes.len().saturating_sub(13)).find(|&i| bytes[i..i + 14].iter().all(u8::is_ascii_digit));
    // `schema_version <= migrated_schema_version.to_s`: a String comparison.
    stamp.is_some_and(|i| name[i..i + 14] <= *version)
}

#[allow(clippy::too_many_arguments)]
fn analyze(
    path: &Path,
    display: &str,
    shared: &Arc<str>,
    raw_text: Option<&str>,
    cfg: &Resolved,
    selected: &dyn Fn(&str) -> bool,
    mode: Option<Mode>,
    redundant: Redundant,
) -> (Vec<Offense>, Option<Corrector>, Vec<Span>) {
    let rails = crate::helpers::cop_registry::plugin_loaded("rubocop-rails");
    if rails && already_migrated_file(path, cfg) {
        // MigrationFileSkippable skips only add_offense: the cops still run, so their errors and
        // the directive warnings stay, and nothing is offended or corrected (no mode).
        SKIP_OFFENSES.with(|s| s.set(true));
        investigate(path, display, shared, raw_text, cfg, selected, None, redundant, rails);
        SKIP_OFFENSES.with(|s| s.set(false));
        return (Vec::new(), None, Vec::new());
    }
    investigate(path, display, shared, raw_text, cfg, selected, mode, redundant, rails)
}

#[allow(clippy::too_many_arguments)]
fn investigate(
    path: &Path,
    display: &str,
    shared: &Arc<str>,
    raw_text: Option<&str>,
    cfg: &Resolved,
    selected: &dyn Fn(&str) -> bool,
    mode: Option<Mode>,
    redundant: Redundant,
    rails: bool,
) -> (Vec<Offense>, Option<Corrector>, Vec<Span>) {
    let text: &str = shared;
    let raw_text = raw_text.unwrap_or(text);
    let errors_before = file_errors_len();
    let warned_before = directives::warnings_len();
    COMMENTS_READ.with(|r| r.set(false));
    let version = target_ruby::prism_version(cfg.target_ruby);
    // Below 3.3 RuboCop parses with the parser gem, which accepts some things Prism rejects.
    let parser_gem = cfg.target_ruby < 3.3;
    crate::helpers::literal_node::set_fold_plus(parser_gem);
    let mut buf = Vec::new();
    let (result, tokens, fake, edits) = parse_for_target(text, &mut buf, version, cfg.target_ruby);
    let mut errors: Vec<(usize, usize, String)> = result
        .errors()
        .filter(|e| !tolerated(e, parser_gem, cfg.target_ruby, text))
        .map(|e| {
            let loc = e.location();
            crate::syntax::translate(e.type_name(), e.message(), loc.start_offset(), loc.end_offset(), text)
        })
        .collect();
    // ...rejects syntax newer than the target, and reports where its grammar fails rather than
    // Prism's messages (semantic errors keep their translated parser-gem messages).
    if parser_gem {
        // The parser-gem simulation below walks the tree recursively (`legacy_syntax::diagnostics`),
        // so the depth guard (see `program` below) comes first here: a long chain overflowed (#351).
        if crate::ast::deeper_than(&result.node(), MAX_AST_DEPTH) {
            file_error(format!("rbcop: {display}: nested more than {MAX_AST_DEPTH} levels deep, not inspected."));
            return (Vec::new(), None, Vec::new());
        }
        let kept = |e: &ruby_prism::Diagnostic| !tolerated(e, parser_gem, cfg.target_ruby, text);
        // A stray backslash is the parser gem's lexer error: reported, then skipped (no recovery).
        let bare_backslash = |e: &ruby_prism::Diagnostic| {
            let loc = e.location();
            e.type_name() == "unexpected_token_ignore" && text.get(loc.start_offset()..loc.end_offset()) == Some("\\")
        };
        // `@`, `@@` or `$` without a name: the lexer's fatal "unexpected `@'" ends the parse there.
        let fatal = |e: &ruby_prism::Diagnostic| {
            let s = e.location().start_offset();
            // Bytes: Prism can place an error inside a multibyte character (#250).
            let after = text.as_bytes()[s..].iter().find(|b| !matches!(b, b'@' | b'$'));
            match e.type_name().trim_end_matches("_3_3") {
                "incomplete_variable_instance" | "incomplete_variable_class" => {
                    !after.is_some_and(|c| c.is_ascii_digit())
                }
                "invalid_variable_global"
                | "instance_variable_bare"
                | "class_variable_bare"
                | "global_variable_bare" => true,
                _ => false,
            }
        };
        let fatal_at = result.errors().filter(|e| kept(e) && fatal(e)).map(|e| e.location().start_offset()).min();
        // A string left open at the end of the file: the lexer's fatal string_eof at its opener,
        // whatever the parser reported before.
        let open_literal = crate::legacy_syntax::unterminated_literal(result.errors().map(|e| e.type_name()), &tokens);
        let open_string = open_literal.map(|o| o.1);
        let semantic_kind = |e: &ruby_prism::Diagnostic| {
            let (k, l) = (e.type_name(), e.location());
            (crate::syntax::is_semantic(k)
                && k != "string_literal_eof"
                && crate::legacy_syntax::class_name_failure(k, l.start_offset(), l.end_offset(), &tokens).is_none())
                || bare_backslash(e)
        };
        let seed = result
            .errors()
            .filter(|e| kept(e) && !semantic_kind(e) && !fatal(e) && e.type_name() != "string_literal_eof")
            .map(|e| {
                crate::legacy_syntax::failure_offset(
                    e.type_name(),
                    e.location().start_offset(),
                    e.location().end_offset(),
                    &tokens,
                )
            })
            .min();
        // The parser gem reports semantic errors as it reduces, so only those before its first
        // grammar failure on Prism's syntax error.
        let cut = seed.unwrap_or(usize::MAX);
        let mut actions: Vec<usize> = Vec::new();
        // Circular argument references: the simulation reports them where the parser reads them.
        let mut circular: Vec<(usize, usize)> = Vec::new();
        // Numbered parameters (2.7+): the parser gem's per-identifier errors come from the tree
        // walk below, in place of Prism's; Prism's reserved-name errors stay where the walk has
        // nothing (parameters named `_1`).
        let numbered = if cfg.target_ruby >= 2.7 {
            crate::syntax::numbered_parameter_errors(&result.node(), cfg.target_ruby)
        } else {
            Vec::new()
        };
        let numbered_kind = |e: &ruby_prism::Diagnostic| {
            let k = e.type_name();
            k.starts_with("numbered_parameter_")
                || k == "expression_not_writable_numbered"
                || (k == "parameter_numbered_reserved" && numbered.iter().any(|n| n.0 == e.location().start_offset()))
        };
        let mut semantic: Vec<_> = result
            .errors()
            .filter(|e| {
                kept(e) && semantic_kind(e) && !fatal(e) && !numbered_kind(e) && e.location().start_offset() < cut
            })
            .filter_map(|e| {
                let (s, end) = crate::legacy_syntax::semantic_location(
                    e.type_name(),
                    e.location().start_offset(),
                    e.location().end_offset(),
                    &tokens,
                    text,
                );
                if bare_backslash(&e) {
                    return Some((s, end, "bare backslash only allowed before newline".to_string()));
                }
                if e.type_name() == "parameter_circular" {
                    circular.push((s, end));
                    return None;
                }
                if e.type_name() == "regexp_parse_error"
                    && let Some(t) = crate::legacy_syntax::invalid_regexp(e.message(), s, &tokens, text)
                {
                    actions.push(t.1);
                    return Some(t);
                }
                let kind = crate::syntax::parser_gem_kind(e.type_name());
                let t = crate::syntax::translate(kind, e.message(), s, end, text);
                actions.push(t.1);
                Some(t)
            })
            .collect();
        for e in numbered.into_iter().filter(|e| e.0 < cut) {
            actions.push(e.1);
            semantic.push(e);
        }
        // Onigmo compile errors of static regexps: semantic errors too (the builder's diagnostic).
        for e in crate::syntax::regexp_compile_errors(&result.node(), text).into_iter().filter(|e| e.0 < cut) {
            actions.push(e.1);
            semantic.push(e);
        }
        actions.sort_unstable();
        let diags = crate::legacy_syntax::diagnostics(
            &result.node(),
            &tokens,
            text,
            cfg.target_ruby,
            seed,
            &actions,
            &circular,
        );
        // Where racc gave up (an empty message): the lexer never gets further.
        let stopped = diags.iter().find(|d| d.2.is_empty()).map(|d| d.0);
        let lexed = |o: &usize| stopped.is_none_or(|s| *o <= s);
        let (fatal_at, open_string) = (fatal_at.filter(lexed), open_string.filter(lexed));
        // A tolerated `<<''` (ended by the end of the file) counts only on a last line without a
        // newline, where the parser gem never starts the heredoc.
        let heredoc_term: Vec<usize> = result
            .errors()
            .filter(|e| e.type_name() == "heredoc_term")
            .filter(|e| kept(e) || !text.as_bytes()[e.location().end_offset()..].contains(&b'\n'))
            .map(|e| e.location().start_offset())
            .collect();
        let heredoc = crate::legacy_syntax::unterminated_heredoc(&tokens, text, &heredoc_term).filter(|h| lexed(&h.0));
        errors = diags
            .into_iter()
            .filter(|d| !d.2.is_empty())
            .map(|(s, e, msg)| crate::syntax::translate("", &msg, s, e, text))
            .chain(semantic)
            .collect();
        // Whichever stops the parser gem's lexer first (an open string before an `@` on its offset).
        let first = [open_string.map(|s| (s, 0)), fatal_at.map(|f| (f, 1)), heredoc.as_ref().map(|h| (h.0, 2))]
            .into_iter()
            .flatten()
            .min();
        match first {
            Some((f, 1)) => {
                errors.retain(|e| e.0 < f);
                errors.push((f, f + 1, format!("unexpected `{}'", text.get(f..f + 1).unwrap_or(""))));
            }
            Some((s, 0)) => {
                let outer = open_literal.map_or(s, |o| o.0);
                errors.retain(|e| e.0 < outer);
                errors.push((s, s + 1, "unterminated string meets end of file".to_string()));
            }
            Some((s, _)) => {
                errors.retain(|e| e.0 < s);
                errors.extend(heredoc.into_iter().flat_map(|h| h.1));
            }
            None => {}
        }
    }
    // Pattern variable names the parser gem's builder rejects, only reached when Prism found no
    // error (a tree walk, so only on trees the depth check below accepts; patterns need `in`/`=>`).
    if errors.is_empty()
        && (text.contains("in") || text.contains("=>"))
        && !crate::ast::deeper_than(&result.node(), MAX_AST_DEPTH)
    {
        errors.extend(crate::syntax::pattern_variable_errors(&result.node()));
    }
    // Onigmo compile errors of static regexps (below 3.3 they are semantic errors above), reported
    // by the Prism translation only when the file otherwise parses.
    if !parser_gem
        && errors.is_empty()
        && text.contains(['/', '%'])
        && !crate::ast::deeper_than(&result.node(), MAX_AST_DEPTH)
    {
        errors.extend(crate::syntax::regexp_compile_errors(&result.node(), text));
    }
    let comments: Vec<Comment> = result
        .comments()
        .filter(|c| !fake.contains(&c.location().start_offset()))
        .map(|c| {
            let (start, mut end) = (c.location().start_offset(), c.location().end_offset());
            let mut embdoc = c.type_() == ruby_prism::CommentType::EmbDocComment;
            // A one-line embedded document the copy has as a `#` comment (`=begin=end`, #534),
            // its newline included as in any embedded document.
            if !embdoc && parser_gem && text[start..].starts_with("=begin") {
                embdoc = true;
                end += usize::from(text.as_bytes().get(end) == Some(&b'\n'));
            }
            Comment { start, end, embdoc }
        })
        .collect();
    let data = result.data_loc().map(|d| d.start_offset());
    let mut src = Source::new(display, text, comments, data, version);
    src.raw_text = raw_text;
    src.parser_gem = parser_gem;
    src.target_ruby = cfg.target_ruby;
    src.lex_edits = edits;
    if cfg.target_ruby < 2.7 {
        src.numbered_sends = crate::syntax::legacy_numbered_sends(&result.node());
    }
    if parser_gem && text.contains("\\\n") {
        src.gem_newlines = crate::legacy_syntax::newline_offsets(&tokens, text, cfg.target_ruby);
    }
    // From 3.3 the runner's parse is parse_warnings::parse (unedited text, same version and
    // encoding): its warnings save that second parse.
    if !parser_gem {
        let _ = src.parse_warnings.set(crate::helpers::parse_warnings::from_result(&result));
    }

    // CommentConfig is analyzed once per investigation, not again for the redundant-directive pass
    // (it reuses the processed source).
    let warn = !matches!(redundant, Redundant::Only(_));
    if !errors.is_empty() {
        // ponytail: without a tree there are no statement bounds, so a detached disable-next or
        // enable-next warns here where RuboCop's would not.
        if warn && src.text.contains("rubocop") {
            for c in &src.comments {
                if let Some(d) = directives::parse(src.slice(c.start, c.end)).filter(|d| d.mode != "pop") {
                    directives::warn_unqualified(&src, &d);
                }
            }
        }
        // RuboCop runs only Lint/Syntax on a file that does not parse, one offense per diagnostic,
        // and does so even under --only (which always lists it); not under -x or the style-guide
        // filter, which leave it out.
        if !selected("Lint/Syntax") || !cfg.relevant(cfg.cop("Lint/Syntax"), path) {
            return (Vec::new(), None, Vec::new());
        }
        let ruby = format_ruby_version(cfg.target_ruby);
        let mut out: Vec<Offense> = errors
            .into_iter()
            .map(|(start, end, msg)| Offense {
                start,
                end,
                cop: "Lint/Syntax",
                // Lint/Syntax#add_offense_from_diagnostic: no parser line under LSP (--editor-mode).
                message: if crate::helpers::lsp::enabled() {
                    msg
                } else {
                    format!(
                        "{msg}\n(Using Ruby {ruby} parser; configure using `TargetRubyVersion` parameter, under `AllCops`)"
                    )
                },
                severity: Severity::Fatal,
                correctable: false,
                global: false,
                corrected: false,
                source: None,
            })
            .collect();
        // The parser translation layer reports one diagnostic per range; Prism can stack two
        // (`unexpected '+'` and `expected a receiver for unary +`), and only the first survives.
        let mut seen = std::collections::HashSet::new();
        out.retain(|o| seen.insert((o.start, o.end)));
        // Then Runner's uniq (same position and message, any range), as for every cop.
        dedup(&mut out);
        return (out, None, Vec::new());
    }

    let opted_in = directives::opted_in(&src);
    let only_redundant = matches!(redundant, Redundant::Only(_));
    let active: Vec<Active> = crate::cops::ALL
        .iter()
        .filter(|r| selected(r.name) && !only_redundant && cfg.team_has(r.name))
        .filter_map(|r| {
            let c = cfg.cop(r.name);
            let (mut min, max) = r.cop.ruby_range();
            // rubocop-rails: `Style::HashExcept.minimum_target_ruby_version(2.0)` once it is loaded.
            if r.name == "Style/HashExcept" && rails {
                min = 2.0;
            }
            let ruby_ok = min <= cfg.target_ruby && cfg.target_ruby <= max;
            // Base#relevant_file?: target_satisfies_all_gem_version_requirements? (railties).
            let rails_ok = r.cop.min_rails() == 0.0 || cfg.target_rails_version() >= r.cop.min_rails();
            // Team#roundup_relevant_cops: excluded_file? (Include/Exclude, the gem requirements)
            // first, then an enable directive opts the cop in past both `Enabled` and the target
            // Ruby range.
            let on = opted_in.contains(&r.name) || (c.enabled && ruby_ok);
            (on && rails_ok && cfg.relevant(c, path)).then_some(Active { name: r.name, cop: r.cop, cfg: c })
        })
        .collect();

    let program = result.node();
    // Deliberate difference: RuboCop dies with SystemStackError on very deep nesting; rbcop skips the
    // file with an error instead, so cops (many of them recursive) never see such a tree.
    if crate::ast::deeper_than(&program, MAX_AST_DEPTH) {
        file_error(format!("rbcop: {}: nested more than {MAX_AST_DEPTH} levels deep, not inspected.", src.path));
        return (Vec::new(), None, Vec::new());
    }
    src.root = Some(result.node());
    let mut facts = Facts::default();
    let mut collector = FactCollector { src: &src, facts: &mut facts };
    collector.facts.string_lines = vec![false; src.line_count() + 2];
    collector.visit(&program);
    let pn = program.as_program_node().unwrap();
    let first_code = pn.statements().body().iter().next().map(|n| n.location().start_offset());
    facts.first_code = first_code;
    // Registry#disabled_names of the --only/--except filtered registry.
    let config_disabled: Vec<&'static str> = crate::helpers::cop_registry::REGISTRY
        .iter()
        .copied()
        .filter(|&c| selected(c) && !cfg.cop_enabled(c))
        .collect();
    if src.text.contains("rubocop") {
        // DisableNext needs statement bounds before the cops run (disabled ranges gate `add_fix`).
        let bounds = directives::needs_statement_bounds(&src).then(|| StatementBounds::build(&src, &program));
        let bounds = bounds.unwrap_or_default();
        let code = code_lines(&src, &facts);
        let statement_bounds = |line: usize| bounds.at(line);
        let info = directives::LineInfo { code: &code, statement_bounds: &statement_bounds };
        let cc = directives::analyze(&src, &config_disabled, &info, directives::prevent_style(cfg), warn);
        facts.disabled = directives::disabled_ranges(&cc);
        facts.comment_config = cc;
        facts.code_lines = code;
        facts.config_disabled = config_disabled;
    }
    facts.text = shared.clone();
    // Directives naming an ambiguous cop: see poison_raise.
    let poison = facts.comment_config.poison.clone().filter(|_| !only_redundant);
    if let Some(p) = &poison {
        POISON.with(|s| *s.borrow_mut() = Some((p.warnings.clone(), Vec::new())));
        FORCE.with(|f| *f.borrow_mut() = (force_scopes(&program, src.text.len()), None));
    }
    let mut fixes: Vec<Fixes> = active
        .iter()
        .map(|a| Fixes {
            autocorrect: mode.is_some_and(|m| a.cop.correctable() && autocorrect::cop_autocorrects(a.cfg, m)),
            ..Fixes::default()
        })
        .collect();

    let mut dispatch: Vec<Vec<usize>> = vec![Vec::new(); node_kind::COUNT];
    for (i, a) in active.iter().enumerate() {
        for &k in a.cop.kinds() {
            dispatch[k].push(i);
        }
    }
    let mut out = Vec::new();
    let mut states: Vec<Option<Box<dyn std::any::Any>>> = (0..active.len()).map(|_| None).collect();
    let mut walker = Walker {
        src: &src,
        facts: &facts,
        config: cfg,
        active: &active,
        dispatch: &dispatch,
        stack: Vec::new(),
        leaf: None,
        out: &mut out,
        states: &mut states,
        fixes: &mut fixes,
    };
    walker.visit(&program);

    for ((a, state), fixes) in active.iter().zip(states.iter_mut()).zip(fixes.iter_mut()) {
        let mut cx = Cx {
            src: &src,
            facts: &facts,
            ancestors: &[],
            cfg: a.cfg,
            config: cfg,
            cop: a.name,
            correctable: a.cop.correctable(),
            out: &mut out,
            state,
            fixes,
        };
        guarded(a.name, &src, None, || a.cop.on_file(&mut cx));
    }
    if let Some(p) = poison {
        // The VariableForce investigation ended at its first raising hook: one cop error, for that
        // joining cop, without a location (Force::HookError), after one comment config analysis.
        if let Some((_, cop)) = FORCE.with(|f| std::mem::take(&mut *f.borrow_mut()).1) {
            directives::push_warnings(&p.warnings);
            cop_error_at(cop, &src, None);
        }
        // Team#process_errors prints the cop errors after the investigation; then
        // Runner#check_for_redundant_disables? analyzes once more (unless --except names the
        // directive cop) and the AmbiguousCopName ends the run.
        let errors = POISON.with(|s| s.borrow_mut().take()).map(|(_, e)| e).unwrap_or_default();
        directives::push_warnings(&errors);
        if !EXCEPT_REDUNDANT.load(std::sync::atomic::Ordering::Relaxed) {
            directives::push_warnings(&p.warnings);
            ABORT.with(|a| *a.borrow_mut() = Some(format!("Error: {}", p.message)));
        }
        return (Vec::new(), None, Vec::new());
    }

    dedup(&mut out);
    // Parser's Range#last_line is the line of end_pos.
    let span = |o: &Offense| (o.cop, src.line_of(o.start), src.line_of(o.end.max(o.start)));
    let mut disabled: Vec<Span> = Vec::new();
    out.retain(|o| {
        let s = span(o);
        let off = directives::suppressed(&facts.disabled, s.0, s.1, s.2);
        if off {
            disabled.push(s);
        }
        !off
    });

    let errors_after_cops = file_errors_len();
    // Runner#add_redundant_disables: offenses_to_check are all offenses, the disabled ones included.
    let checked: Option<Vec<Span>> = match redundant {
        Redundant::Own if mode.is_none() => Some(out.iter().map(span).chain(disabled.iter().copied()).collect()),
        Redundant::Only(c) => Some(c.to_vec()),
        _ => None,
    };
    let mut redundant_fix = None;
    let rcfg = cfg.cop(REDUNDANT_DISABLE);
    if let Some(checked) = checked
        && selected(REDUNDANT_DISABLE)
        && (rcfg.enabled || opted_in.contains(&REDUNDANT_DISABLE))
        && cfg.relevant(rcfg, path)
        && (!facts.disabled.is_empty() || !facts.comment_config.detached.is_empty())
    {
        let mut fixes =
            Fixes { autocorrect: mode.is_some_and(|m| autocorrect::cop_autocorrects(rcfg, m)), ..Fixes::default() };
        let (mut found, mut state) = (Vec::new(), None);
        let mut cx = Cx {
            src: &src,
            facts: &facts,
            ancestors: &[],
            cfg: rcfg,
            config: cfg,
            cop: REDUNDANT_DISABLE,
            correctable: true,
            out: &mut found,
            state: &mut state,
            fixes: &mut fixes,
        };
        guarded(REDUNDANT_DISABLE, &src, None, || {
            crate::cops::lint_redundant_cop_disable_directive::check(&mut cx, &checked)
        });
        found.retain(|o| {
            let s = span(o);
            !directives::suppressed(&facts.disabled, s.0, s.1, s.2)
        });
        out.extend(found);
        redundant_fix = fixes.corrector;
    }

    let mut corrector = None;
    if mode.is_some() {
        let mut by_cop: Vec<(&'static str, Corrector)> =
            active.iter().zip(fixes).filter_map(|(a, f)| Some((a.name, f.corrector?))).collect();
        by_cop.extend(redundant_fix.map(|c| (REDUNDANT_DISABLE, c)));
        corrector = autocorrect::collate(by_cop, shared.clone());
        if corrector.is_some() && !only_redundant {
            // Team#investigate_with_corrector: the other cops do not run in a correcting iteration.
            let correcting: Vec<&str> = active
                .iter()
                .filter(|a| a.cop.correctable() && autocorrect::cop_autocorrects(a.cfg, mode.unwrap()))
                .map(|a| a.name)
                .collect();
            out.retain(|o| correcting.contains(&o.cop));
            // Nor do they raise there: only the correcting cops' errors count for this pass.
            FILE_ERRORS.with(|e| {
                let mut i = 0;
                e.borrow_mut().retain(|(cop, _, _)| {
                    i += 1;
                    i <= errors_before || cop.as_deref().is_none_or(|c| correcting.contains(&c))
                })
            });
        }
    }
    for o in out.iter_mut() {
        let c = cfg.cop(o.cop);
        // find_severity: the configured Severity, else one the cop passed to add_offense (any
        // but the Convention placeholder), else the department default.
        o.severity = c
            .severity
            .unwrap_or_else(|| if o.severity == Severity::Convention { default_severity(o.cop) } else { o.severity });
        o.correctable &= autocorrect_allowed(c);
    }
    // CommentConfig#cop_disabled_line_ranges analyzes lazily and the `no department given`
    // warnings print then: during the investigation when a cop reads it (comment_config_read),
    // so before Team#process_errors prints the cop errors; else in
    // Runner#check_for_redundant_disables?, after them, which reaches only the source the loop
    // ended with and returns before analyzing when --except names the directive cop. A correcting
    // iteration read it unless --ignore-disable-comments: its offenses asked enabled_lines?.
    // ponytail: the loop Runner reruns after Lint/RedundantCopDisableDirective corrects is not
    // followed by another check, so an unread last iteration there would print nothing.
    let warnings = directives::take_warnings_since(warned_before);
    let read = COMMENTS_READ.with(|r| r.get()) || (corrector.is_some() && !directives::ignoring_disable_comments());
    let checked_after = only_redundant
        || ((mode.is_none() || corrector.is_none()) && !EXCEPT_REDUNDANT.load(std::sync::atomic::Ordering::Relaxed));
    if read || checked_after {
        let at = if read { errors_before } else { errors_after_cops };
        FILE_ERRORS.with(|e| {
            let mut e = e.borrow_mut();
            let at = at.min(e.len());
            e.splice(at..at, warnings.into_iter().map(|w| (None, w, true)));
        });
    }
    (out, corrector, disabled)
}

thread_local! {
    static IN_COP: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Start of the node the running cop callback was given (None in on_file): where
    /// Commissioner#with_cop_error_handling locates an error.
    static VISITING: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

/// Runner#errors: every cop error of the run, for the end-of-run summary and the exit status.
static ERRORS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// Print a cop error now (Commissioner) and keep it for the summary.
pub fn cop_error(cop: &str, src: &Source, at: usize) {
    cop_error_at(cop, src, Some(at));
}

/// A cop error at the node being visited (Team#process_errors: `path:line:column`, or the bare
/// path for a callback without a node, such as on_new_investigation).
pub fn cop_error_at(cop: &str, src: &Source, at: Option<usize>) {
    // rubocop-rspec's TopLevelGroup cops run from on_new_investigation, whose errors Team reports
    // without a location; rbcop runs them as node cops. So does RSpec/LeakyLocalVariable, a
    // VariableForce cop (its hooks run from the force's on_new_investigation), and
    // Style/DirectiveScope (an on_new_investigation cop that rbcop runs on the program node).
    const ON_NEW_INVESTIGATION: &[&str] = &[
        "RSpec/DescribeClass",
        "RSpec/DescribeMethod",
        "RSpec/InstanceVariable",
        "RSpec/LeakyLocalVariable",
        "RSpec/MultipleDescribes",
        "RSpec/NestedGroups",
        "RSpec/RepeatedSubjectCall",
        "RSpec/SpecFilePathFormat",
        "RSpec/SpecFilePathSuffix",
        "RSpec/SubjectStub",
        "Style/DirectiveScope",
    ];
    let at = at.filter(|_| !ON_NEW_INVESTIGATION.contains(&cop));
    let location = match at {
        // Range#line and the 0-based Range#column.
        Some(at) => {
            let (line, col) = crate::formatter::line_col(src.text, at);
            format!("{}:{line}:{}", src.absolute_path(), col - 1)
        }
        None => src.absolute_path(),
    };
    record_error(Some(cop), format!("An error occurred while {cop} cop was inspecting {location}."));
}

/// The running cop callback stands for RuboCop's callback on another node starting at `at` (an
/// `on_args` that rbcop runs while visiting the def or block).
pub fn visit_at(at: usize) {
    VISITING.with(|v| v.set(Some(at)));
}

/// The node the running cop callback was given (see VISITING).
pub fn visiting() -> Option<usize> {
    VISITING.with(|v| v.get())
}

thread_local! {
    /// Team#warnings: every RuboCop::Warning a cop raised for the file this thread is linting,
    /// one per raise, in order. The reporting loop prints them when it reaches the file (RuboCop
    /// prints them as it inspects, in target order) and keeps them for the end-of-run summary.
    static FILE_WARNINGS: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Team#handle_warning: a cop's Warning for the file being linted.
pub fn cop_warning(message: &str, src: &Source) {
    FILE_WARNINGS.with(|w| w.borrow_mut().push(format!("{message} (from file: {})", src.absolute_path())));
}

/// The warnings the cops raised on this thread since the last call (one file's, in order).
pub fn take_file_warnings() -> Vec<String> {
    FILE_WARNINGS.with(|w| std::mem::take(&mut *w.borrow_mut()))
}

thread_local! {
    /// The cop errors of the file this thread is linting, printed by the reporting loop when it
    /// reaches the file: RuboCop prints them as it inspects, in target order, and rbcop's workers
    /// finish files in any order.
    /// With the cop that raised, if a cop did; directive warnings (`true`) sit among them where
    /// RuboCop prints them.
    #[allow(clippy::type_complexity)]
    static FILE_ERRORS: std::cell::RefCell<Vec<(Option<String>, String, bool)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The errors `file_error` recorded on this thread since the last call (one file's, in order,
/// with the directive warnings printed among them), the errors now kept for the summary.
pub fn take_file_errors() -> Vec<String> {
    let entries = FILE_ERRORS.with(|e| std::mem::take(&mut *e.borrow_mut()));
    let errors = entries.iter().filter(|e| !e.2).map(|e| e.1.clone());
    ERRORS.lock().unwrap_or_else(|e| e.into_inner()).extend(errors);
    entries.into_iter().map(|e| e.1).collect()
}

fn file_errors_len() -> usize {
    FILE_ERRORS.with(|e| e.borrow().len())
}

/// Record an error for printing with its file (`take_file_errors`) and keep it for the summary
/// (it makes the run fail). In a poisoned file (see `poison_raise`) cop errors wait for the end
/// of the investigation, as Team prints them.
pub fn file_error(msg: String) {
    record_error(None, msg);
}

fn record_error(cop: Option<&str>, msg: String) {
    let deferred = POISON.with(|p| match &mut *p.borrow_mut() {
        Some((_, errors)) => {
            errors.push(msg.clone());
            true
        }
        None => false,
    });
    if deferred {
        ERRORS.lock().unwrap_or_else(|e| e.into_inner()).push(msg);
    } else {
        FILE_ERRORS.with(|e| e.borrow_mut().push((cop.map(String::from), msg, false)));
    }
}

thread_local! {
    /// The file being inspected names an ambiguous cop in a directive: the warnings each comment
    /// config analysis prints, and the cop errors so far.
    static POISON: std::cell::RefCell<Option<(Vec<String>, Vec<String>)>> = const { std::cell::RefCell::new(None) };
    /// A cop read the comment config during the current investigation (see comment_config_read).
    static COMMENTS_READ: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// rubocop-rails MigrationFileSkippable: add_offense returns at once (see already_migrated_file).
    static SKIP_OFFENSES: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The error that ends the run at the file this thread inspected last.
    static ABORT: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// `--except` names Lint/RedundantCopDisableDirective or Lint (Runner#except_redundant_cop_disable_directive?).
static EXCEPT_REDUNDANT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_except_redundant(on: bool) {
    EXCEPT_REDUNDANT.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// A running cop reads CommentConfig#cop_disabled_line_ranges (Base#add_offense's enabled_lines?,
/// Lint/MissingCopEnableDirective, CommentsHelp, StatementModifier, Style/DirectiveScope): the
/// lazy analysis happens now (see the end of `investigate`), and raises in a poisoned file.
pub fn comment_config_read() {
    COMMENTS_READ.with(|r| r.set(true));
    poison_raise(true);
}

/// Whether add_offense is skipped for the file being inspected (MigrationFileSkippable).
pub fn skipping_offenses() -> bool {
    SKIP_OFFENSES.with(|s| s.get())
}

/// CommentConfig#analyze raising AmbiguousCopName (directives::Poison): a cop that reaches the
/// comment config (Base#add_offense, Lint/MissingCopEnableDirective, Lint/CopDirectiveSyntax)
/// raises inside its callback, after the analysis' warnings when `warn`. A no-op elsewhere.
pub fn poison_raise(warn: bool) {
    let poisoned = POISON.with(|p| match &*p.borrow() {
        Some((warnings, _)) => {
            if warn {
                directives::push_warnings(warnings);
            }
            true
        }
        None => false,
    });
    if poisoned {
        // Unwinds to `guarded` (a cop error) without running the panic hook.
        std::panic::resume_unwind(Box::new(()));
    }
}

/// The VariableForce joining cops whose offenses come from its hooks, in team (registry) order.
/// Rails/SaveBang's CREATE_MSG offenses come from its after_leaving_scope; its others, and those
/// of Style/InfiniteLoop and Style/MapIntoArray, from ordinary callbacks.
const FORCE_COPS: &[&str] = &[
    "Lint/ShadowedArgument",
    "Lint/ShadowingOuterLocalVariable",
    "Lint/UnderscorePrefixedVariableName",
    "Lint/UnusedBlockArgument",
    "Lint/UnusedMethodArgument",
    "Lint/UselessAssignment",
    "Rails/SaveBang",
    "RSpec/LeakyLocalVariable",
];

/// The unwind payload of a VariableForce hook raising in a poisoned file (no cop error of its own).
struct ForceHook;

thread_local! {
    /// In a poisoned file: the VariableForce scopes (start, end), and the first hook to raise as
    /// (event offset, team order, cop).
    #[allow(clippy::type_complexity)]
    static FORCE: std::cell::RefCell<(Vec<(usize, usize)>, Option<((usize, usize), &'static str)>)> =
        const { std::cell::RefCell::new((Vec::new(), None)) };
}

/// The scopes VariableForce enters (def, defs, block, lambda, class, module, sclass, the file).
fn force_scopes(program: &Node, len: usize) -> Vec<(usize, usize)> {
    struct Scopes(Vec<(usize, usize)>);
    impl<'pr> Visit<'pr> for Scopes {
        fn visit_branch_node_enter(&mut self, node: Node<'pr>) {
            if matches!(
                node,
                Node::DefNode { .. }
                    | Node::BlockNode { .. }
                    | Node::LambdaNode { .. }
                    | Node::ClassNode { .. }
                    | Node::ModuleNode { .. }
                    | Node::SingletonClassNode { .. }
            ) {
                self.0.push((node.location().start_offset(), node.location().end_offset()));
            }
        }
    }
    let mut s = Scopes(vec![(0, len)]);
    s.visit(program);
    s.0
}

/// Base#add_offense in a poisoned file (see `poison_raise`). RuboCop runs the VariableForce cops
/// as one force before the AST walk: the first hook whose cop adds an offense raises and ends the
/// force for the file, so only that cop errors. rbcop runs them as separate cops, so each raise
/// is a candidate: the earliest force event (a declaration for Lint/ShadowingOuterLocalVariable,
/// leaving the innermost scope of the offense for the others; ties in team order) wins.
// ponytail: an offense of a variable declared in an outer scope but flagged inside an inner one
// is keyed by the inner scope; only the ordering between force cops depends on it.
pub fn poison_raise_offense(cop: &'static str, at: usize, message: &str) {
    let force =
        FORCE_COPS.iter().position(|c| *c == cop).filter(|_| cop != "Rails/SaveBang" || message.contains("persisted?"));
    let raised = force.is_some_and(|order| {
        POISON.with(|p| p.borrow().is_some())
            && FORCE.with(|f| {
                let mut f = f.borrow_mut();
                let offset = if cop == "Lint/ShadowingOuterLocalVariable" {
                    at
                } else {
                    f.0.iter().filter(|(s, e)| *s <= at && at < *e).map(|(_, e)| *e).min().unwrap_or(usize::MAX)
                };
                if f.1.is_none_or(|(key, _)| (offset, order) < key) {
                    f.1 = Some(((offset, order), cop));
                }
                true
            })
    });
    if raised {
        std::panic::resume_unwind(Box::new(ForceHook));
    }
    comment_config_read();
}

/// The error that ends the run at the file just inspected on this thread, if any.
pub fn take_abort() -> Option<String> {
    ABORT.with(|a| a.borrow_mut().take())
}

/// Whether the file just inspected on this thread ends the run.
pub fn aborting() -> bool {
    ABORT.with(|a| a.borrow().is_some())
}

/// AST depth beyond which a file is not inspected. Real code stays under 50 (the deepest of the
/// rails, discourse and RuboCop corpora is 46). At this depth every cop, inspecting and correcting,
/// ran in the default 2 MiB thread stack; main.rs gives worker threads 64 MiB of headroom.
pub const MAX_AST_DEPTH: usize = 1000;

/// The run's cop errors, in file order.
pub fn take_errors() -> Vec<String> {
    let mut errors = std::mem::take(&mut *ERRORS.lock().unwrap_or_else(|e| e.into_inner()));
    // ExecuteRunner#display_error_summary lists one entry per raise, duplicates included.
    errors.sort();
    errors
}

/// Whether this thread is running a cop callback (its panics are reported by `guarded`).
pub fn in_cop() -> bool {
    IN_COP.with(|c| c.get())
}

/// Commissioner#with_cop_error_handling: a cop that fails on one node is reported and skipped
/// there, and inspection goes on (other nodes, other cops, other files).
fn guarded(cop: &str, src: &Source, at: Option<usize>, f: impl FnOnce()) {
    IN_COP.with(|c| c.set(true));
    VISITING.with(|v| v.set(at));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    IN_COP.with(|c| c.set(false));
    // The callback may stand for one on another node (`visit_at`).
    let at = VISITING.with(|v| v.replace(None));
    if let Err(payload) = result
        && !payload.is::<ForceHook>()
    {
        cop_error_at(cop, src, at);
    }
}

/// Cop::Base#use_corrector: an offense with a corrector is only `[Correctable]` when the cop's
/// `AutoCorrect` is `always`/`true` (the default) or `contextual` (not under `--editor-mode`);
/// `false`/`disabled` turn it off.
fn autocorrect_allowed(cfg: &CopConfig) -> bool {
    use crate::config::value::Value;
    match cfg.get("AutoCorrect") {
        None | Some(Value::Bool(true)) => true,
        Some(Value::Str(s)) => s == "always" || (s == "contextual" && !crate::helpers::lsp::enabled()),
        _ => false,
    }
}

/// Prism reports some semantic errors the parser gem accepts (e.g. top-level `yield` in .builder
/// templates, `retry` outside `rescue`, void value expressions such as `x = return 1`: the parser
/// gem has no void-value check; a variable bound in an alternative pattern, `in a | 1`: its
/// grammars through Ruby32 accept it, #559).
// ponytail: only verified ones are tolerated; add more as real code turns them up.
const TOLERATED_ERRORS: &[&str] =
    &["invalid_yield", "invalid_retry_without_rescue", "void_expression", "pattern_capture_in_alternative"];

/// Prism errors the parser gem accepts for the target. Up to Ruby 2.5, `begin..else..end` without
/// `rescue` is only a `useless_else` warning (reported by Lint/UselessElseWithoutRescue); before
/// 2.7 `def m(x = x)` is only a warning (Lint/CircularArgumentReference); before 3.0 `_1 = x` is
/// accepted (Builders::Default#check_reserved_for_numparam; Lint/NumberedParameterAssignment).
/// Before 2.7 `_1` is an ordinary identifier, so Prism's numbered-parameter checks (`|a| a + _1`,
/// `_1` in nested blocks, `_1 = x` in a block that reads `_1`) do not apply.
/// The parser gem also ends a `<<''` heredoc (empty delimiter) at the end of the file, where Prism
/// finds it unterminated. Before 2.5 `return` in a class/module body is accepted (the builder's
/// `invalid_return` check is 2.5+). Before the target that has anonymous forwarding (`&` 3.1, `*`
/// and `**` 3.2) the grammar fails on the argument, with no "no anonymous ... parameter" error.
pub fn tolerated(e: &ruby_prism::Diagnostic, parser_gem: bool, target_ruby: f64, text: &str) -> bool {
    let kind = e.type_name();
    (parser_gem && TOLERATED_ERRORS.contains(&kind))
        || (kind == "return_invalid" && target_ruby < 2.5)
        || (kind == "argument_no_forwarding_ampersand" && target_ruby < 3.1)
        || (matches!(kind, "argument_no_forwarding_star" | "argument_no_forwarding_star_star") && target_ruby < 3.2)
        || (kind == "begin_lonely_else" && target_ruby <= 2.5)
        || (kind == "parameter_circular" && target_ruby < 2.7)
        || (kind == "parameter_numbered_reserved" && target_ruby < 3.0)
        || ((kind.starts_with("numbered_parameter_") || kind == "expression_not_writable_numbered")
            && target_ruby < 2.7)
        || (parser_gem && e.message().starts_with("unterminated heredoc; can't find string \"\"") && {
            // The parser gem ends the heredoc at the empty line after a final newline (a line of
            // spaces and tabs before it is rewritten into an empty line by `parse_for_target`).
            text.ends_with('\n')
        })
}

/// Cop::Base#default_severity.
pub fn default_severity(cop: &str) -> Severity {
    if cop == "Lint/Syntax" {
        Severity::Fatal
    } else if cop.starts_with("Lint/") || cop.starts_with("Security/") {
        // Cop::Base::DEPARTMENT_SEVERITIES
        Severity::Warning
    } else if cop.starts_with("Metrics/") {
        Severity::Refactor
    } else {
        Severity::Convention
    }
}

fn same(a: &Node, b: &Node) -> bool {
    let (la, lb) = (a.location(), b.location());
    node_kind::of(a) == node_kind::of(b) && la.start_offset() == lb.start_offset() && la.end_offset() == lb.end_offset()
}

struct Walker<'s, 'pr, 'c> {
    src: &'s Source<'s>,
    facts: &'s Facts,
    config: &'s Resolved,
    active: &'s [Active<'c>],
    dispatch: &'s [Vec<usize>],
    stack: Vec<Node<'pr>>,
    leaf: Option<Node<'pr>>,
    out: &'s mut Vec<Offense>,
    states: &'s mut [Option<Box<dyn std::any::Any>>],
    fixes: &'s mut [Fixes],
}

impl<'pr> Walker<'_, 'pr, '_> {
    fn dispatch(&mut self, node: &Node<'pr>) {
        let kind = node_kind::of(node);
        // A hash pattern key without a value (`in {a:}`, `in {"a":}`) is the parser gem's
        // match_var, no `sym`: no cop sees a symbol there (#548).
        if kind == node_kind::SYMBOL_NODE && self.pattern_key() {
            return;
        }
        // The parser gem's block node starts at its call; Prism's BlockNode at `{` / `do`.
        let at = match (node, self.stack.last()) {
            (Node::BlockNode { .. }, Some(call)) => call.location().start_offset(),
            _ => node.location().start_offset(),
        };
        for &i in &self.dispatch[kind] {
            let a = &self.active[i];
            let mut cx = Cx {
                src: self.src,
                facts: self.facts,
                ancestors: &self.stack,
                cfg: a.cfg,
                config: self.config,
                cop: a.name,
                correctable: a.cop.correctable(),
                out: self.out,
                state: &mut self.states[i],
                fixes: &mut self.fixes[i],
            };
            guarded(a.name, self.src, Some(at), || a.cop.on_node(&mut cx, node));
        }
    }

    /// The node being dispatched is the key of a valueless hash pattern pair.
    fn pattern_key(&self) -> bool {
        let n = self.stack.len();
        n >= 2
            && self.stack[n - 1].as_assoc_node().is_some_and(|a| a.value().as_implicit_node().is_some())
            && self.stack[n - 2].as_hash_pattern_node().is_some()
    }

    /// For node types Prism's default visitor reaches without the enter/leave hooks.
    fn branch(&mut self, node: Node<'pr>, walk: impl FnOnce(&mut Self)) {
        if self.stack.last().is_some_and(|top| same(top, &node)) {
            walk(self);
        } else {
            self.visit_branch_node_enter(node);
            walk(self);
            self.visit_branch_node_leave();
        }
    }

    fn leaf(&mut self, node: Node<'pr>) {
        if !self.leaf.as_ref().is_some_and(|l| same(l, &node)) {
            self.dispatch(&node);
        }
    }
}

impl<'pr> Visit<'pr> for Walker<'_, 'pr, '_> {
    fn visit_branch_node_enter(&mut self, node: Node<'pr>) {
        self.dispatch(&node);
        self.stack.push(node);
    }

    fn visit_branch_node_leave(&mut self) {
        self.stack.pop();
    }

    fn visit_leaf_node_enter(&mut self, node: Node<'pr>) {
        self.dispatch(&node);
        self.leaf = Some(node);
    }

    fn visit_leaf_node_leave(&mut self) {
        self.leaf = None;
    }

    fn visit_arguments_node(&mut self, node: &ArgumentsNode<'pr>) {
        self.branch(node.as_node(), |s| ruby_prism::visit_arguments_node(s, node));
    }
    fn visit_block_argument_node(&mut self, node: &BlockArgumentNode<'pr>) {
        self.branch(node.as_node(), |s| ruby_prism::visit_block_argument_node(s, node));
    }
    fn visit_block_node(&mut self, node: &BlockNode<'pr>) {
        self.branch(node.as_node(), |s| ruby_prism::visit_block_node(s, node));
    }
    fn visit_block_parameter_node(&mut self, node: &BlockParameterNode<'pr>) {
        self.leaf(node.as_node());
    }
    fn visit_call_node(&mut self, node: &CallNode<'pr>) {
        self.branch(node.as_node(), |s| ruby_prism::visit_call_node(s, node));
    }
    fn visit_constant_path_node(&mut self, node: &ConstantPathNode<'pr>) {
        self.branch(node.as_node(), |s| ruby_prism::visit_constant_path_node(s, node));
    }
    fn visit_else_node(&mut self, node: &ElseNode<'pr>) {
        self.branch(node.as_node(), |s| ruby_prism::visit_else_node(s, node));
    }
    fn visit_ensure_node(&mut self, node: &EnsureNode<'pr>) {
        self.branch(node.as_node(), |s| ruby_prism::visit_ensure_node(s, node));
    }
    fn visit_local_variable_target_node(&mut self, node: &LocalVariableTargetNode<'pr>) {
        self.leaf(node.as_node());
    }
    fn visit_parameters_node(&mut self, node: &ParametersNode<'pr>) {
        self.branch(node.as_node(), |s| ruby_prism::visit_parameters_node(s, node));
    }
    fn visit_rescue_node(&mut self, node: &RescueNode<'pr>) {
        self.branch(node.as_node(), |s| ruby_prism::visit_rescue_node(s, node));
    }
    fn visit_splat_node(&mut self, node: &SplatNode<'pr>) {
        self.branch(node.as_node(), |s| ruby_prism::visit_splat_node(s, node));
    }
    fn visit_statements_node(&mut self, node: &StatementsNode<'pr>) {
        self.branch(node.as_node(), |s| ruby_prism::visit_statements_node(s, node));
    }
}

/// Collects `Facts` (string and heredoc layout) in a separate, cheap walk.
struct FactCollector<'s, 'a> {
    src: &'s Source<'a>,
    facts: &'s mut Facts,
}

impl FactCollector<'_, '_> {
    fn cover(&mut self, start: usize, end: usize) {
        if end > start {
            for l in self.src.line_of(start)..=self.src.line_of(end - 1) {
                self.facts.string_lines[l] = true;
            }
        }
    }

    fn string_like(&mut self, start: usize, end: usize, opening: Option<&[u8]>, body: Option<(usize, usize, usize)>) {
        let Some(open) = opening else { return };
        if !open.starts_with(b"<<") {
            self.facts.string_ranges.push((start, end));
        } else if let Some((b, t, t_end)) = body {
            self.heredoc(b, t, t_end, open);
            // loc.heredoc_body starts at the beginning of the body's first line; Prism's first
            // part starts after a `<<~` indentation when it is an interpolation (`\t#{a} b`).
            self.facts.string_ranges.push((self.src.line_start(self.src.line_of(b)), t));
        }
    }

    /// Body from `body` to the terminator line starting at `term` (ending at `term_end`).
    fn heredoc(&mut self, body: usize, term: usize, term_end: usize, opening: &[u8]) {
        let delimiter = self.src.slice(term, term_end).trim().to_string();
        self.facts.heredocs.push(Heredoc {
            first: self.src.line_of(body),
            terminator: self.src.line_of(term),
            delimiter,
            quoted: opening.ends_with(b"'"),
        });
    }
}

impl<'pr> Visit<'pr> for FactCollector<'_, '_> {
    fn visit_string_node(&mut self, node: &StringNode<'pr>) {
        let c = node.content_loc();
        self.cover(c.start_offset(), c.end_offset());
        let loc = node.location();
        let body = node.closing_loc().map(|t| (c.start_offset(), t.start_offset(), t.end_offset()));
        self.string_like(loc.start_offset(), loc.end_offset(), node.opening_loc().map(|o| o.as_slice()), body);
    }

    fn visit_interpolated_string_node(&mut self, node: &InterpolatedStringNode<'pr>) {
        let loc = node.location();
        let body = match (node.parts().first(), node.closing_loc()) {
            (Some(p), Some(t)) => Some((p.location().start_offset(), t.start_offset(), t.end_offset())),
            _ => None,
        };
        self.string_like(loc.start_offset(), loc.end_offset(), node.opening_loc().map(|o| o.as_slice()), body);
        ruby_prism::visit_interpolated_string_node(self, node);
    }

    fn visit_x_string_node(&mut self, node: &XStringNode<'pr>) {
        let c = node.content_loc();
        self.cover(c.start_offset(), c.end_offset());
        if node.opening_loc().as_slice().starts_with(b"<<") {
            let t = node.closing_loc();
            self.heredoc(c.start_offset(), t.start_offset(), t.end_offset(), node.opening_loc().as_slice());
        }
    }

    fn visit_regular_expression_node(&mut self, node: &RegularExpressionNode<'pr>) {
        let c = node.content_loc();
        self.cover(c.start_offset(), c.end_offset());
    }
}

/// Ruby's Float#to_s for a version (3.4, 2.7, 4.0).
fn format_ruby_version(v: f64) -> String {
    let s = format!("{v}");
    if s.contains('.') { s } else { format!("{s}.0") }
}

/// Cop::Base#add_offense ignores a second offense on a range the cop already reported.
/// Runner's `offenses_by_iteration.flatten.uniq`: Offense equality is line, column, cop, message and
/// severity (fixed per cop), not the range, so two ranges starting at one position collapse.
fn dedup(out: &mut Vec<Offense>) {
    let mut seen = std::collections::HashSet::new();
    out.retain(|o| seen.insert((o.cop, o.start, o.message.clone())));
}
