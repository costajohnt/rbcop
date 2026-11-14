use crate::helpers::sorbet_rbi_type::{Error, rbs_annotation_after, rbs_of};
use crate::prelude::*;

pub const NAME: &str = "Sorbet/ForbidTLet";
pub static COP: ForbidTLet = ForbidTLet;
pub struct ForbidTLet;

const MSG: &str = "Do not use `T.let`.";

// A minimal port of rubocop-sorbet's RBSAssertionCorrection (batch S3 owns the shared
// `sorbet_rbs_assertion` helper; ForbidTLet and ForbidTTypeAlias use this copy until it lands).

/// `(send (const nil? :T) :let _ _)` (a block-pass counts as an argument).
fn t_let(node: &Node) -> bool {
    node.as_call_node().is_some_and(|c| {
        !is_csend(&c)
            && method_name(&c) == "let"
            && c.receiver().is_some_and(|r| r.as_constant_read_node().is_some_and(|k| k.name().as_slice() == b"T"))
            && call_args(&c).len() + usize::from(c.block().is_some_and(|b| b.as_block_argument_node().is_some())) == 2
            && c.block().is_none_or(|b| b.as_block_node().is_none())
    })
}

fn range(n: &Node) -> (usize, usize) {
    (n.location().start_offset(), n.location().end_offset())
}

/// `assertion_ends_line?`: after spaces and tabs, the end of the file, a newline or a comment.
fn ends_line(text: &str, end: usize) -> bool {
    let rest = text[end..].trim_start_matches([' ', '\t']);
    rest.is_empty() || rest.starts_with(['\n', '\r', '#'])
}

/// `rbs_assertion_autocorrectable?` for the node at `node` (range `r`) whose parser parent is
/// `parent`.
pub fn autocorrectable(cx: &Cx, r: (usize, usize), parent: Option<&Node>, allow_assignment: bool) -> bool {
    if !cx.cfg.params.truthy("AutocorrectToRBS") || !ends_line(cx.src.text, r.1) || rbs_annotation_after(cx.src, r.1).is_some() {
        return false;
    }
    // assertion_statement: the assignment whose value the assertion is, when allowed.
    let statement = match parent {
        Some(p) if helpers::node_types::is_assignment(p) && range(p).1 == r.1 && range(p).0 < r.0 => {
            if !allow_assignment {
                return false;
            }
            range(p).0
        }
        _ => r.0,
    };
    cx.src.begins_its_line(statement)
}

fn single_line(src: &crate::source::Source, (s, e): (usize, usize)) -> bool {
    src.line_of(s) == src.line_of(e)
}

/// The argument-list context: the call's send range, its opening/closing parens, the items and
/// the target's index, whether the target is a splat.
struct ArgContext {
    open_end: usize,
    close_start: usize,
    indentation: String,
    items: Vec<(usize, usize)>,
    target: usize,
    splat: bool,
}

/// `call_argument_context`.
fn call_argument_context(cx: &Cx, node: &Node, r: (usize, usize)) -> Option<ArgContext> {
    let anc = cx.ancestors;
    let n = anc.len();
    let parent_i = helpers::parser_parent::parser_parent_index(anc, n)?;
    let parent = &anc[parent_i];
    let (call_i, target) = if parent.as_call_node().is_some() {
        (parent_i, r)
    } else if parent.as_splat_node().is_some() {
        let ci = helpers::parser_parent::parser_parent_index(anc, parent_i)?;
        anc[ci].as_call_node()?;
        (ci, range(parent))
    } else if parent.as_assoc_splat_node().is_some() {
        let first = node.as_call_node().and_then(|c| call_args(&c).into_iter().next());
        if first.is_some_and(|a| a.as_keyword_hash_node().is_some()) {
            return None;
        }
        let hi = helpers::parser_parent::parser_parent_index(anc, parent_i)?;
        anc[hi].as_keyword_hash_node()?;
        let ci = helpers::parser_parent::parser_parent_index(anc, hi)?;
        anc[ci].as_call_node()?;
        (ci, range(parent))
    } else {
        let pair = parent.as_assoc_node()?;
        (range(&pair.value()) == r).then_some(())?;
        let hi = helpers::parser_parent::parser_parent_index(anc, parent_i)?;
        anc[hi].as_keyword_hash_node()?;
        let ci = helpers::parser_parent::parser_parent_index(anc, hi)?;
        anc[ci].as_call_node()?;
        (ci, range(parent))
    };
    let call = anc[call_i].as_call_node()?;
    let mut args = call_args(&call);
    let mut items: Vec<(usize, usize)> = Vec::new();
    for a in &args {
        match a.as_keyword_hash_node() {
            Some(h) => items.extend(h.elements().iter().map(|e| range(&e))),
            None => items.push(range(a)),
        }
    }
    if let Some(b) = call.block().filter(|b| b.as_block_argument_node().is_some()) {
        items.push(range(&b));
        args.push(b);
    }
    // `call.arguments.include?(...)`: the target's argument is the call's own.
    if !args.iter().any(|a| {
        let ar = range(a);
        ar == target || (a.as_keyword_hash_node().is_some() && ar.0 <= target.0 && target.1 <= ar.1)
    }) {
        return None;
    }
    let send = send_range(&call);
    let src = cx.src;
    let (open, close) = (call.opening_loc()?, call.closing_loc()?);
    if !single_line(src, send) || src.comments.iter().any(|c| c.start >= send.0 && c.end <= send.1) {
        return None;
    }
    // one_matching_assertion?: one `T.let`-named send with a `T` receiver in the call.
    struct Count<'a>(&'a [u8], usize);
    impl<'pr> ruby_prism::Visit<'pr> for Count<'_> {
        fn visit_call_node(&mut self, c: &ruby_prism::CallNode<'pr>) {
            if !is_csend(c)
                && c.name().as_slice() == self.0
                && c.receiver().is_some_and(|r| helpers::parser_parent::is_top_const(&r, "T"))
            {
                self.1 += 1;
            }
            ruby_prism::visit_call_node(self, c);
        }
    }
    let name = node.as_call_node()?.name().as_slice();
    let mut count = Count(name, 0);
    if let Some(rv) = call.receiver() {
        ruby_prism::Visit::visit(&mut count, &rv);
    }
    for a in &args {
        ruby_prism::Visit::visit(&mut count, a);
    }
    if !is_csend(&call) && call.name().as_slice() == name && call.receiver().is_some_and(|r| helpers::parser_parent::is_top_const(&r, "T")) {
        count.1 += 1;
    }
    if count.1 != 1 {
        return None;
    }
    let target_i = items.iter().position(|&i| i == target)?;
    let line_start = src.line_start(src.line_of(send.0));
    let indentation: String = src.text[line_start..].chars().take_while(|c| *c == ' ' || *c == '\t').collect();
    let splat = parent.as_splat_node().is_some() || parent.as_assoc_splat_node().is_some();
    Some(ArgContext {
        open_end: open.end_offset(),
        close_start: close.start_offset(),
        indentation,
        items,
        target: target_i,
        splat,
    })
}

/// `inside_single_line_block?`: a single-line `{}`/`do` block (numbered parameters included, `it`
/// not) or lambda literal around the node.
fn inside_single_line_block(cx: &Cx) -> bool {
    let src = cx.src;
    cx.ancestors.iter().any(|a| {
        let lines = |o: ruby_prism::Location, c: ruby_prism::Location| src.line_of(o.start_offset()) == src.line_of(c.start_offset());
        if let Some(b) = a.as_block_node() {
            return !b.parameters().is_some_and(|p| p.as_it_parameters_node().is_some()) && lines(b.opening_loc(), b.closing_loc());
        }
        a.as_lambda_node().is_some_and(|l| lines(l.opening_loc(), l.closing_loc()))
    })
}

/// `rbs_assertion_correction_context` + `autocorrect_rbs_assertion`: the edits for `expr #: ann`.
pub enum Context {
    Direct,
    Arguments(ArgsCtx),
}
pub struct ArgsCtx(ArgContext);

pub fn context(cx: &Cx, node: &Node, allow_assignment: bool) -> Option<Context> {
    let r = range(node);
    let parent = helpers::parser_parent::parser_parent(cx.ancestors);
    if autocorrectable(cx, r, parent, allow_assignment) {
        return Some(Context::Direct);
    }
    if !cx.cfg.params.truthy("AutocorrectToRBS")
        || !single_line(cx.src, r)
        || inside_single_line_block(cx)
        || rbs_annotation_after(cx.src, r.1).is_some()
    {
        return None;
    }
    call_argument_context(cx, node, r).map(|a| Context::Arguments(ArgsCtx(a)))
}

/// The replacements for the context (applied in the corrector).
pub fn edits(cx: &Cx, ctx: &Context, r: (usize, usize), expr: &str, ann: &str) -> Vec<(usize, usize, String)> {
    match ctx {
        Context::Direct => vec![(r.0, r.1, format!("{expr} #: {ann}"))],
        Context::Arguments(ArgsCtx(a)) => {
            let width = helpers::alignment::configured_indentation_width(cx).max(0) as usize;
            let item_ind = format!("{}{}", a.indentation, " ".repeat(width));
            let mut replacement = format!("{expr} #: {ann}");
            if a.splat {
                let nested = format!("{item_ind}{}", " ".repeat(width));
                replacement = format!("(\n{nested}{replacement}\n{item_ind})");
            } else if a.target < a.items.len() - 1 {
                replacement = format!("{expr}, #: {ann}");
            }
            let mut out = vec![(r.0, r.1, replacement)];
            out.push((a.open_end, a.items[0].0, format!("\n{item_ind}")));
            for (i, w) in a.items.windows(2).enumerate() {
                let sep = if i == a.target && !a.splat { "" } else { "," };
                out.push((w[0].1, w[1].0, format!("{sep}\n{item_ind}")));
            }
            out.push((a.items.last().unwrap().1, a.close_start, format!("\n{}", a.indentation)));
            out
        }
    }
}

impl Cop for ForbidTLet {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if !t_let(node) {
            return;
        }
        let call = node.as_call_node().unwrap();
        let r = send_range(&call);
        let mut args = call_args(&call);
        if let Some(b) = call.block() {
            args.push(b);
        }
        // autocorrect_t_let_to_rbs: not a nested T.let; a type RBI can parse.
        let mut fix: Vec<(usize, usize, String)> = Vec::new();
        if !t_let(&args[0])
            && let Some(ctx) = context(cx, node, true)
        {
            match rbs_of(node_text(&args[1])) {
                Ok(ann) => fix = edits(cx, &ctx, r, node_text(&args[0]), &ann),
                Err(Error::Type) => {}
                Err(Error::Other) => return crate::runner::cop_error_at(NAME, cx.src, crate::runner::visiting()),
            }
        }
        cx.add_fix(r.0, r.1, MSG, |c| {
            for (s, e, t) in &fix {
                c.replace(*s, *e, t);
            }
        });
    }
}
