use crate::prelude::*;

pub const NAME: &str = "Naming/MemoizedInstanceVariableName";
pub static COP: MemoizedInstanceVariableName = MemoizedInstanceVariableName;
pub struct MemoizedInstanceVariableName;

const INITIALIZE_METHODS: &[&str] = &["initialize", "initialize_clone", "initialize_copy", "initialize_dup"];

fn range(n: &Node) -> (usize, usize) {
    let l = n.location();
    (l.start_offset(), l.end_offset())
}

/// find_definition: the nearest `def`/`defs`/`define_method(:x) do` ancestor: (method name, body).
fn find_definition<'pr>(ancestors: &[Node<'pr>]) -> Option<(String, Option<Node<'pr>>)> {
    for a in ancestors.iter().rev() {
        if let Some(def) = a.as_def_node() {
            return Some((String::from_utf8_lossy(def.name().as_slice()).into_owned(), def.body()));
        }
        let Some(call) = a.as_call_node() else { continue };
        // (block (send _ {define_method define_singleton_method} ({sym str} $_)) ...)
        let Some(block) = call.block().and_then(|b| b.as_block_node()) else { continue };
        if matches!(block.parameters(), Some(Node::NumberedParametersNode { .. } | Node::ItParametersNode { .. })) {
            continue;
        }
        if !matches!(method_name(&call), "define_method" | "define_singleton_method") {
            continue;
        }
        let args = call_args(&call);
        if args.len() != 1 {
            continue;
        }
        let name = match &args[0] {
            Node::SymbolNode { .. } => args[0].as_symbol_node().unwrap().unescaped().to_vec(),
            Node::StringNode { .. } if !is_parser_dstr(&args[0]) => args[0].as_string_node().unwrap().unescaped().to_vec(),
            _ => continue,
        };
        return Some((String::from_utf8_lossy(&name).into_owned(), block.body()));
    }
    None
}

fn bare(method: &str) -> String {
    method.chars().filter(|c| !matches!(c, '!' | '?' | '=')).collect()
}

/// nameable_method?: /\A[a-zA-Z_]\w*\z/ after deleting `!?=`.
fn nameable(method: &str) -> bool {
    let m = bare(method);
    let mut chars = m.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

struct Style<'a>(&'a str);

impl Style<'_> {
    fn matches(&self, method: &str, ivar: &str) -> bool {
        if INITIALIZE_METHODS.contains(&method) {
            return true;
        }
        let method = bare(method);
        let var = ivar.replacen('@', "", 1);
        let no_underscore = method.strip_prefix('_').unwrap_or(&method).to_string();
        let with_underscore = format!("_{method}");
        let candidates: Vec<&str> = match self.0 {
            "required" => {
                let mut c = vec![with_underscore.as_str()];
                if method.starts_with('_') {
                    c.push(&method);
                }
                c
            }
            "optional" => vec![&method, &with_underscore, &no_underscore],
            _ => vec![&method, &no_underscore],
        };
        candidates.contains(&var.as_str())
    }

    fn suggested(&self, method: &str) -> String {
        let s = bare(method);
        if self.0 == "required" { format!("_{s}") } else { s }
    }

    fn message(&self, ivar: &str, method: &str) -> String {
        let suggested = self.suggested(method);
        if self.0 == "required" && !ivar.replacen('@', "", 1).starts_with('_') {
            format!("Memoized variable `{ivar}` does not start with `_`. Use `@{suggested}` instead.")
        } else {
            format!("Memoized variable `{ivar}` does not match method name `{method}`. Use `@{suggested}` instead.")
        }
    }
}

/// `body.children.last` for the parser node a Prism body stands for.
// ponytail: begin/kwbegin/parens, `||`/`&&`, if/unless/case, blocks and loops; other node types not modelled.
fn last_child<'pr>(body: &Node<'pr>) -> Option<Node<'pr>> {
    let last_stmt = |s: Option<ruby_prism::StatementsNode<'pr>>| s.and_then(|s| s.body().iter().last());
    // a body as one parser node: the single statement (a multi-statement `begin` never matches)
    let one = |s: Option<ruby_prism::StatementsNode<'pr>>| s.filter(|s| s.body().len() == 1).and_then(|s| s.body().iter().next());
    match body {
        Node::StatementsNode { .. } => {
            let s = body.as_statements_node().unwrap();
            if s.body().len() == 1 { last_child(&s.body().iter().next().unwrap()) } else { s.body().iter().last() }
        }
        Node::BeginNode { .. } => {
            let b = body.as_begin_node().unwrap();
            if b.begin_keyword_loc().is_some() && b.rescue_clause().is_none() && b.ensure_clause().is_none() {
                last_stmt(b.statements())
            } else {
                None
            }
        }
        Node::ParenthesesNode { .. } => {
            let p = body.as_parentheses_node().unwrap();
            match p.body() {
                Some(Node::StatementsNode { .. }) => last_stmt(p.body().unwrap().as_statements_node()),
                b => b,
            }
        }
        Node::OrNode { .. } => Some(body.as_or_node().unwrap().right()),
        Node::AndNode { .. } => Some(body.as_and_node().unwrap().right()),
        Node::IfNode { .. } => match body.as_if_node().unwrap().subsequent()? {
            e @ Node::IfNode { .. } => Some(e),
            e => one(e.as_else_node()?.statements()),
        },
        Node::UnlessNode { .. } => one(body.as_unless_node().unwrap().statements()),
        Node::CaseNode { .. } => one(body.as_case_node().unwrap().else_clause()?.statements()),
        Node::CaseMatchNode { .. } => one(body.as_case_match_node().unwrap().else_clause()?.statements()),
        Node::WhileNode { .. } => one(body.as_while_node().unwrap().statements()),
        Node::UntilNode { .. } => one(body.as_until_node().unwrap().statements()),
        Node::CallNode { .. } => {
            let b = body.as_call_node().unwrap().block()?;
            let b = b.as_block_node()?;
            one(b.body()?.as_statements_node())
        }
        _ => None,
    }
}

/// The parser node for a body: the single statement, or None for a multi-statement `begin`.
fn single(body: &Node) -> Option<(usize, usize)> {
    match body.as_statements_node() {
        Some(s) if s.body().len() == 1 => Some(range(&s.body().iter().next().unwrap())),
        Some(_) => None,
        None => Some(range(body)),
    }
}

impl Cop for MemoizedInstanceVariableName {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::INSTANCE_VARIABLE_OR_WRITE_NODE, kind::DEFINED_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let style = Style(cx.cfg.str("EnforcedStyleForLeadingUnderscores").unwrap_or("disallowed"));
        if let Some(or) = node.as_instance_variable_or_write_node() {
            let Some((method, Some(body))) = find_definition(cx.ancestors) else { return };
            if !nameable(&method) {
                return;
            }
            let me = range(node);
            let is_body = single(&body).is_some_and(|b| helpers::node_equality::same(cx.src, b, me));
            let is_last = last_child(&body).is_some_and(|l| helpers::node_equality::same(cx.src, range(&l), me));
            if !is_body && !is_last {
                return;
            }
            let ivar = String::from_utf8_lossy(or.name().as_slice()).into_owned();
            if style.matches(&method, &ivar) {
                return;
            }
            let msg = style.message(&ivar, &method);
            let replacement = format!("@{}", style.suggested(&method));
            let l = or.name_loc();
            cx.add_fix(l.start_offset(), l.end_offset(), msg, |c| c.replace(l.start_offset(), l.end_offset(), &replacement));
            return;
        }
        let defined = node.as_defined_node().unwrap();
        let Some(arg) = defined.value().as_instance_variable_read_node() else { return };
        let Some((method, Some(body))) = find_definition(cx.ancestors) else { return };
        if !nameable(&method) {
            return;
        }
        let name = arg.name().as_slice().to_vec();
        let Some((defined_ivar, return_ivar, assign)) = defined_memoized(&body, &name) else { return };
        let ivar = String::from_utf8_lossy(&name).into_owned();
        if style.matches(&method, &ivar) {
            return;
        }
        let msg = style.message(&ivar, &method);
        let replacement = format!("@{}", style.suggested(&method));
        for (s, e) in [defined_ivar, return_ivar, assign] {
            cx.add_fix(s, e, msg.clone(), |c| c.replace(s, e, &replacement));
        }
    }
}

/// `(begin (if (defined $(ivar %1)) (return $(ivar %1)) nil?) ... $(ivasgn %1 _))`
type Ranges = ((usize, usize), (usize, usize), (usize, usize));
fn defined_memoized(body: &Node, name: &[u8]) -> Option<Ranges> {
    let stmts: Vec<Node> = body.as_statements_node()?.body().iter().collect();
    if stmts.len() < 2 {
        return None;
    }
    let ivar = |n: &Node| n.as_instance_variable_read_node().is_some_and(|i| i.name().as_slice() == name);
    let guard = stmts[0].as_if_node()?;
    guard.if_keyword_loc()?;
    if guard.subsequent().is_some() {
        return None;
    }
    let defined = guard.predicate().as_defined_node()?;
    let defined_ivar = defined.value();
    if !ivar(&defined_ivar) {
        return None;
    }
    let then = guard.statements()?;
    if then.body().len() != 1 {
        return None;
    }
    let ret = then.body().iter().next()?;
    let ret_args = ret.as_return_node()?.arguments()?.arguments();
    if ret_args.len() != 1 {
        return None;
    }
    let return_ivar = ret_args.iter().next()?;
    if !ivar(&return_ivar) {
        return None;
    }
    let assign = stmts.last()?.as_instance_variable_write_node()?;
    if assign.name().as_slice() != name {
        return None;
    }
    let l = assign.name_loc();
    Some((range(&defined_ivar), range(&return_ivar), (l.start_offset(), l.end_offset())))
}
