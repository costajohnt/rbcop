use crate::helpers::alignment_mixin::enabled_cop_str;
use crate::prelude::*;
use crate::source::Source;

pub const NAME: &str = "Layout/HashAlignment";
pub static COP: HashAlignment = HashAlignment;
pub struct HashAlignment;

/// HashAlignmentStyles classes; `offenses_by` / `column_deltas` are keyed by class.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Style {
    Key,
    Table,
    Separator,
    Splat,
}

impl Style {
    fn message(self) -> &'static str {
        match self {
            Style::Key => "Align the keys of a hash literal if they span more than one line.",
            Style::Separator => "Align the separators of a hash literal if they span more than one line.",
            Style::Table => "Align the keys and values of a hash literal if they span more than one line.",
            Style::Splat => "Align keyword splats with the rest of the hash if it spans more than one line.",
        }
    }
}

/// A `pair` or `kwsplat` with parser-gem ranges (a `kwsplat` is its own key and value).
struct Elem {
    splat: bool,
    rocket: bool,
    omission: bool,
    range: (usize, usize),
    key: (usize, usize),
    op: (usize, usize),
    value: (usize, usize),
}

fn span(n: &Node) -> (usize, usize) {
    let l = n.location();
    (l.start_offset(), l.end_offset())
}

fn elem(n: &Node) -> Elem {
    let range = span(n);
    if let Some(a) = n.as_assoc_node() {
        let (key, value) = (span(&a.key()), span(&a.value()));
        let omission = a.value().as_implicit_node().is_some();
        match a.operator_loc() {
            Some(op) => Elem {
                splat: false,
                rocket: true,
                omission,
                range,
                key,
                op: (op.start_offset(), op.end_offset()),
                value,
            },
            // `a: 1`: parser's key excludes the colon, which is the pair's operator.
            None => Elem {
                splat: false,
                rocket: false,
                omission,
                range,
                key: (key.0, key.1 - 1),
                op: (key.1 - 1, key.1),
                value,
            },
        }
    } else {
        let op = n.as_assoc_splat_node().unwrap().operator_loc();
        Elem { splat: true, rocket: false, omission: false, range, key: range, op: (op.start_offset(), op.end_offset()), value: range }
    }
}

/// Column deltas; None = key absent from the Ruby hash.
#[derive(Default, Clone, Copy)]
struct Deltas {
    key: Option<i64>,
    sep: Option<i64>,
    value: Option<i64>,
}

impl Deltas {
    fn good(&self) -> bool {
        [self.key, self.sep, self.value].iter().all(|d| d.is_none_or(|v| v == 0))
    }
}

struct Ctx<'a> {
    src: &'a Source<'a>,
    elems: Vec<Elem>,
}

impl Ctx<'_> {
    fn col(&self, off: usize) -> i64 {
        self.src.column(off) as i64
    }
    fn line(&self, off: usize) -> usize {
        self.src.line_of(off)
    }
    /// HashElementNode#same_line?
    fn same_line(&self, a: &Elem, b: &Elem) -> bool {
        self.line(a.range.1) == self.line(b.range.0) || self.line(a.range.0) == self.line(b.range.1)
    }
    fn key_delta(&self, f: &Elem, c: &Elem, right: bool) -> i64 {
        if self.same_line(f, c) || (right && (f.splat || c.splat)) {
            0
        } else if right {
            self.col(f.key.1) - self.col(c.key.1)
        } else {
            self.col(f.key.0) - self.col(c.key.0)
        }
    }
    fn value_delta(&self, f: &Elem, c: &Elem) -> i64 {
        if self.same_line(f, c) || f.splat || c.splat { 0 } else { self.col(f.value.0) - self.col(c.value.0) }
    }
    fn delimiter_delta(&self, f: &Elem, c: &Elem) -> i64 {
        if self.same_line(f, c) || f.rocket != c.rocket { 0 } else { self.col(f.op.0) - self.col(c.op.0) }
    }
    fn begins_its_line(&self, off: usize) -> bool {
        let s = self.src.line_start(self.line(off));
        let line = &self.src.text[s..];
        let first = line.bytes().position(|b| !matches!(b, b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r'));
        first == Some(off - s)
    }
    fn pairs(&self) -> impl Iterator<Item = &Elem> {
        self.elems.iter().filter(|e| !e.splat)
    }

    // KeyAlignment
    fn key_sep(&self, p: &Elem) -> i64 {
        if p.rocket { self.col(p.key.1) + 1 - self.col(p.op.0) } else { 0 }
    }
    fn key_value(&self, p: &Elem) -> i64 {
        if self.line(p.key.0) != self.line(p.value.0) || p.omission {
            0
        } else {
            self.col(p.op.1) + 1 - self.col(p.value.0)
        }
    }

    // TableAlignment
    fn target_operator_column(&self, f: &Elem) -> i64 {
        let src = self.src;
        let single = |k: &(usize, usize)| self.line(k.0) == self.line(k.1);
        let mut candidates: Vec<i64> =
            self.pairs().filter(|p| !single(&p.key)).map(|p| self.col(p.key.1) + 1).collect();
        let width =
            self.pairs().filter(|p| single(&p.key)).map(|p| src.slice(p.key.0, p.key.1).chars().count()).max().unwrap_or(0);
        if width > 0 {
            candidates.push(self.col(f.range.0) + width as i64 + 1);
        }
        candidates.into_iter().max().unwrap_or(0)
    }
    fn table_value(&self, f: &Elem, c: &Elem) -> i64 {
        let delim = self.pairs().map(|p| if p.rocket { 4 } else { 2 }).max().unwrap_or(2);
        if c.omission { 0 } else { self.target_operator_column(f) + delim - 1 - self.col(c.value.0) }
    }

    fn checkable(&self, style: Style) -> bool {
        if style == Style::Key {
            return true;
        }
        let pairs: Vec<&Elem> = self.pairs().collect();
        let on_same_line = pairs.windows(2).any(|w| self.same_line(w[0], w[1]));
        let mixed = pairs.iter().any(|p| p.rocket != pairs[0].rocket);
        let later = pairs.iter().any(|p| self.line(p.key.1) != self.line(p.value.0));
        !on_same_line && !mixed && !pairs[0].omission && !later
    }

    fn first_deltas(&self, style: Style, f: &Elem) -> Deltas {
        match style {
            Style::Key => Deltas { key: None, sep: Some(self.key_sep(f)), value: Some(self.key_value(f)) },
            Style::Table => {
                let sep = if f.rocket { self.target_operator_column(f) - self.col(f.op.0) } else { 0 };
                Deltas { key: None, sep: Some(sep), value: Some(self.table_value(f, f) - sep) }
            }
            _ => Deltas::default(),
        }
    }

    fn deltas(&self, style: Style, f: &Elem, c: &Elem) -> Deltas {
        match style {
            Style::Key | Style::Splat if !self.begins_its_line(c.range.0) => Deltas::default(),
            Style::Key => Deltas {
                key: Some(self.key_delta(f, c, false)),
                sep: Some(self.key_sep(c)),
                value: Some(self.key_value(c)),
            },
            Style::Splat => Deltas { key: Some(self.key_delta(f, c, false)), ..Deltas::default() },
            Style::Table | Style::Separator => {
                let table = style == Style::Table;
                let key = self.key_delta(f, c, !table);
                let sep = if c.rocket {
                    let rocket = if table {
                        self.target_operator_column(f) - self.col(c.op.0)
                    } else {
                        self.delimiter_delta(f, c)
                    };
                    rocket - key
                } else {
                    0
                };
                let value = if table {
                    self.table_value(f, c)
                } else if c.omission {
                    0
                } else {
                    self.value_delta(f, c)
                };
                Deltas { key: Some(key), sep: Some(sep), value: Some(value - key - sep) }
            }
        }
    }
}

fn styles(cx: &Cx, key: &str) -> Vec<Style> {
    let mut out = Vec::new();
    for s in cx.cfg.strings(key) {
        let st = match s.as_str() {
            "key" => Style::Key,
            "table" => Style::Table,
            "separator" => Style::Separator,
            _ => continue,
        };
        // `formats.uniq`
        if !out.contains(&st) {
            out.push(st);
        }
    }
    out
}

/// `ignore_hash_argument?` for the last argument hash of a send/csend/super/yield.
fn ignored_last_argument(cx: &Cx, node: &Node, braces: bool) -> bool {
    let n = cx.ancestors.len();
    if n < 2 || kind::of(&cx.ancestors[n - 1]) != kind::ARGUMENTS_NODE {
        return false;
    }
    let args = cx.ancestors[n - 1].as_arguments_node().unwrap();
    if args.arguments().iter().last().is_none_or(|l| span(&l) != span(node)) {
        return false;
    }
    let owner = &cx.ancestors[n - 2];
    let block_pass = |b: Option<Node>| b.is_some_and(|b| kind::of(&b) == kind::BLOCK_ARGUMENT_NODE);
    let dispatch = match kind::of(owner) {
        kind::CALL_NODE => !block_pass(owner.as_call_node().unwrap().block()),
        kind::SUPER_NODE => !block_pass(owner.as_super_node().unwrap().block()),
        kind::YIELD_NODE => true,
        _ => false,
    };
    dispatch
        && match cx.cfg.str("EnforcedLastArgumentHashStyle") {
            Some("always_ignore") => true,
            Some("ignore_explicit") => braces,
            Some("ignore_implicit") => !braces,
            _ => false,
        }
}

impl Cop for HashAlignment {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::HASH_NODE, kind::KEYWORD_HASH_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let (children, braces): (Vec<Node>, bool) = match node.as_hash_node() {
            Some(h) => (h.elements().iter().collect(), true),
            None => (node.as_keyword_hash_node().unwrap().elements().iter().collect(), false),
        };
        let src = cx.src;
        let ctx = Ctx { src, elems: children.iter().map(elem).collect() };
        let Some(first_idx) = ctx.elems.iter().position(|e| !e.splat) else { return };
        let range = span(node);
        if src.line_of(range.0) == src.line_of(range.1) || ignored_last_argument(cx, node, braces) {
            return;
        }
        if autocorrect_incompatible(cx, node, &children, &ctx, first_idx) {
            return;
        }
        let rockets = styles(cx, "EnforcedHashRocketStyle");
        let colons = styles(cx, "EnforcedColonStyle");
        if !rockets.iter().any(|&s| ctx.checkable(s)) || !colons.iter().any(|&s| ctx.checkable(s)) {
            return;
        }
        let alignment_for = |e: &Elem| -> Vec<Style> {
            if e.splat {
                vec![Style::Splat]
            } else if e.rocket {
                rockets.clone()
            } else {
                colons.clone()
            }
        };

        // offenses_by: insertion-ordered class => offending element indexes (duplicates kept).
        let mut offenses_by: Vec<(Style, Vec<usize>)> = Vec::new();
        let mut column_deltas: Vec<(Style, usize, Deltas)> = Vec::new();
        let mut check = |style: Style, idx: usize, d: Deltas| {
            let pos = match offenses_by.iter().position(|(s, _)| *s == style) {
                Some(p) => p,
                None => {
                    offenses_by.push((style, Vec::new()));
                    offenses_by.len() - 1
                }
            };
            if d.good() {
                return;
            }
            column_deltas.retain(|(s, i, _)| !(*s == style && *i == idx));
            column_deltas.push((style, idx, d));
            offenses_by[pos].1.push(idx);
        };
        let first = &ctx.elems[first_idx];
        for style in alignment_for(first) {
            check(style, first_idx, ctx.first_deltas(style, first));
        }
        for (i, e) in ctx.elems.iter().enumerate() {
            for style in alignment_for(e) {
                check(style, i, ctx.deltas(style, first, e));
            }
        }

        let mut register: Vec<(Style, Vec<usize>)> = Vec::new();
        if let Some(p) = offenses_by.iter().position(|(s, _)| *s == Style::Splat) {
            register.push(offenses_by.remove(p));
        }
        // `min_by { |_, v| v.length }`: the first class with the fewest entries.
        if let Some(min) = offenses_by.iter().min_by_key(|(_, v)| v.len()) {
            register.push(min.clone());
        }
        for (style, idxs) in register {
            for i in idxs {
                let e = &ctx.elems[i];
                let first_style = alignment_for(e)[0];
                let correctable = column_deltas
                    .iter()
                    .find(|(s, j, _)| *s == first_style && *j == i)
                    .is_some_and(|(_, _, d)| corrects(&ctx, e, d));
                cx.add_with(e.range.0, e.range.1, style.message(), correctable);
            }
        }
    }
}

/// Whether `correct_node` makes an edit.
fn corrects(ctx: &Ctx, e: &Elem, d: &Deltas) -> bool {
    let key = d.key.unwrap_or(0).max(-ctx.col(e.key.0));
    if e.splat || e.omission {
        return key != 0;
    }
    key != 0 || d.sep.unwrap_or(0) != 0 || d.value.unwrap_or(0) != 0
}

/// `autocorrect_incompatible_with_other_cops?`
fn autocorrect_incompatible(cx: &Cx, node: &Node, children: &[Node], ctx: &Ctx, first_idx: usize) -> bool {
    if enabled_cop_str(cx, "Layout/ArgumentAlignment", "EnforcedStyle").as_deref() != Some("with_fixed_indentation") {
        return false;
    }
    let n = cx.ancestors.len();
    let me = span(node);
    // Parser parent: the call holding the hash as receiver or argument.
    let (call, sibling) = match cx.ancestors.last().map(kind::of) {
        Some(kind::CALL_NODE) => (cx.ancestors[n - 1].as_call_node().unwrap(), None),
        Some(kind::ARGUMENTS_NODE) if n >= 2 => {
            let Some(c) = cx.ancestors[n - 2].as_call_node() else { return false };
            let args = call_args(&c);
            let i = args.iter().position(|a| span(a) == me).unwrap_or(0);
            (c, i.checked_sub(1).map(|j| span(&args[j])))
        }
        _ => return false,
    };
    // argument_before_hash: a leading kwsplat's expression, else the left sibling node.
    let left = match children.first().and_then(|c| c.as_assoc_splat_node()) {
        Some(s) => s.value().map(|v| span(&v)),
        None => sibling,
    };
    let selector = left
        .or_else(|| call.message_loc().map(|l| (l.start_offset(), l.end_offset())))
        .unwrap_or_else(|| send_range(&call));
    let src = cx.src;
    let pair_line = src.line_of(ctx.elems[first_idx].range.0);
    src.line_of(selector.0) == pair_line || src.line_of(selector.1) == pair_line
}
