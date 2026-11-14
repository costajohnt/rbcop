use crate::helpers::sorbet_target_version::enabled_for;
use crate::prelude::*;

pub const NAME: &str = "Sorbet/ObsoleteStrictMemoization";
pub static COP: ObsoleteStrictMemoization = ObsoleteStrictMemoization;
pub struct ObsoleteStrictMemoization;

const MSG: &str = "This two-stage workaround for memoization in `#typed: strict` files is no longer necessary. \
See https://sorbet.org/docs/type-assertions#put-type-assertions-behind-memoization.";

/// A parser `begin` with 2+ children: a StatementsNode of 2+ statements, unless it is the body of
/// a plain `begin..end` (parser `kwbegin`, whose children are the statements themselves).
pub fn begin_statements<'pr>(node: &Node<'pr>, ancestors: &[Node<'pr>]) -> Option<Vec<Node<'pr>>> {
    let stmts: Vec<Node> = node.as_statements_node()?.body().iter().collect();
    if stmts.len() < 2 {
        return None;
    }
    if let Some(b) = ancestors.last().and_then(|p| p.as_begin_node())
        && b.begin_keyword_loc().is_some()
        && b.rescue_clause().is_none()
        && b.ensure_clause().is_none()
    {
        return None;
    }
    Some(stmts)
}

/// `(send (const {nil? cbase} :T) name args...)`: the arguments.
pub fn t_send<'pr>(node: &Node<'pr>, name: &str, argc: usize) -> Option<(Node<'pr>, Vec<Node<'pr>>)> {
    let c = node.as_call_node().filter(|c| !is_csend(c) && method_name(c) == name)?;
    if c.block().is_some_and(|b| b.as_block_node().is_some()) {
        return None;
    }
    let recv = c.receiver().filter(|r| helpers::parser_parent::is_top_const(r, "T"))?;
    let mut args = call_args(&c);
    if let Some(b) = c.block() {
        args.push(b);
    }
    (args.len() == argc).then_some((recv, args))
}

/// The legacy pattern's last two statements, as ranges and source.
pub struct Legacy<'pr> {
    /// The first statement (`@x = T.let(...)`).
    pub first: (usize, usize),
    pub ivar: &'pr str,
    pub t: &'pr str,
    /// T.let's first argument: Some(name) for an ivar read, the range for `nil`.
    pub let_ivar: Option<&'pr [u8]>,
    pub let_nil: Option<(usize, usize)>,
    pub ivar_type: &'pr str,
    pub init: &'pr str,
    pub second_end: usize,
}

fn range(n: &Node) -> (usize, usize) {
    (n.location().start_offset(), n.location().end_offset())
}

pub fn legacy<'pr>(stmts: &[Node<'pr>]) -> Option<Legacy<'pr>> {
    let [.., first, second] = stmts else { return None };
    let w = first.as_instance_variable_write_node()?;
    let ivar = std::str::from_utf8(w.name().as_slice()).ok()?;
    let (t, args) = t_send(&w.value(), "let", 2)?;
    let (_, nilable) = t_send(&args[1], "nilable", 1)?;
    let o = second.as_instance_variable_or_write_node()?;
    if o.name().as_slice() != ivar.as_bytes() {
        return None;
    }
    Some(Legacy {
        first: range(first),
        ivar,
        t: node_text(&t),
        let_ivar: args[0].as_instance_variable_read_node().map(|r| r.name().as_slice()),
        let_nil: args[0].as_nil_node().map(|_| range(&args[0])),
        ivar_type: node_text(&nilable[0]),
        init: node_text(&o.value()),
        second_end: second.location().end_offset(),
    })
}

impl Cop for ObsoleteStrictMemoization {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::STATEMENTS_NODE]
    }

    fn on_file(&self, cx: &mut Cx) {
        // relevant_file? raises without a lockfile next to the Gemfile (RuboCop aborts the run).
        if enabled_for("0.5.10210").is_err() {
            crate::runner::cop_error_at(NAME, cx.src, None);
        }
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if enabled_for("0.5.10210") != Ok(true) {
            return;
        }
        let Some(stmts) = begin_statements(node, cx.ancestors) else { return };
        let Some(l) = legacy(&stmts) else { return };
        if l.let_ivar != Some(l.ivar.as_bytes()) {
            return;
        }
        let src = cx.src;
        let begin = stmts[0].location().start_offset();
        let indent = " ".repeat(src.column(begin));
        let (t, init, ty) = (l.t, l.init, l.ivar_type);
        let mut correction = format!("{} ||= {t}.let({init}, {t}.nilable({ty}))", l.ivar);
        let too_long = helpers::statement_modifier::max_line_length(cx)
            .is_some_and(|max| helpers::statement_modifier::line_length(cx, &format!("{indent}{correction}")) as f64 > max);
        if too_long || correction.contains('\n') {
            let nl = format!("\n{indent}");
            correction = format!(
                "{} ||= {t}.let(\n{indent}  {},\n{indent}  {t}.nilable({}),\n{indent})",
                l.ivar,
                init.replace('\n', &nl),
                ty.replace('\n', &nl)
            );
        }
        let (s, e) = l.first;
        let end = l.second_end;
        cx.add_fix(s, e, MSG, |c| c.replace(s, end, &correction));
    }
}
