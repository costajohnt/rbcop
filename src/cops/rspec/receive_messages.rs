use crate::prelude::*;

pub const NAME: &str = "RSpec/ReceiveMessages";
pub static COP: ReceiveMessages = ReceiveMessages;
pub struct ReceiveMessages;

static REQUIRES_QUOTES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?m)^:".*?"|=$|^[^A-Za-z0-9_]+$"#).unwrap());
static LABEL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A[\p{L}_][\p{L}\p{N}_]*[?!]?\z").unwrap());

/// One `allow(x).to receive(:m).and_return(v)` statement.
struct Item {
    range: (usize, usize),
    line: usize,
    /// `allow_argument`: the structure of `x`
    group: String,
    /// `receive_arg(item).first`: the structure of `:m`
    message_key: String,
    /// the `receive(...).and_return(...)` send
    receive: (usize, usize),
    name: String,
    ret: String,
}

/// allow_receive_message?: `(send (send nil? :allow ...) :to (send (send nil? :receive (sym _))
/// :and_return !#heredoc_or_splat?))`, with what the cop reads from it.
fn item(cx: &Cx, node: &Node) -> Option<Item> {
    let to = helpers::rspec::send_call(node).filter(|c| !is_csend(c) && method_name(c) == "to")?;
    let allow = helpers::rspec::nil_send(&to.receiver()?, "allow")?;
    let [arg] = &send_args(&to)[..] else { return None };
    let and_return = helpers::rspec::plain_send(arg, "and_return")?;
    let receive = helpers::rspec::nil_send(&and_return.receiver()?, "receive")?;
    let [sym] = &send_args(&receive)[..] else { return None };
    if !helpers::rspec::is_sym(cx, sym) {
        return None;
    }
    let [ret] = &send_args(&and_return)[..] else { return None };
    if (matches!(kind::of(ret), kind::STRING_NODE | kind::INTERPOLATED_STRING_NODE) && is_heredoc(ret)) || ret.as_splat_node().is_some() {
        return None;
    }
    let group_arg = send_args(&allow).into_iter().next()?;
    let src = cx.src;
    let ret_src = node_text(ret);
    // normalize_return_arg: a hash without braces gets them
    let ret = if ret.as_keyword_hash_node().is_some() { format!("{{ {ret_src} }}") } else { ret_src.to_string() };
    Some(Item {
        range: helpers::rspec::node_range(node),
        line: src.line_of(node.location().start_offset()),
        group: helpers::parser_sexp::sexp(&group_arg, src).unwrap_or_else(|| node_text(&group_arg).to_string()),
        message_key: helpers::parser_sexp::sexp(sym, src).unwrap_or_default(),
        receive: helpers::rspec::node_range(arg),
        name: helpers::rails::sym_value(sym).unwrap_or_default(),
        ret,
    })
}

impl Cop for ReceiveMessages {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::STATEMENTS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // on_begin: a statement list of two or more (not an explicit `begin ... end`)
        let stmts: Vec<Node> = node.as_statements_node().unwrap().body().iter().collect();
        // `begin a; b end` is a `kwbegin` holding the statements (with `rescue` they are a `begin`)
        let kwbegin = cx.ancestors.last().and_then(|p| p.as_begin_node()).is_some_and(|b| {
            b.begin_keyword_loc().is_some() && b.rescue_clause().is_none() && b.ensure_clause().is_none() && b.else_clause().is_none()
        });
        if stmts.len() < 2 || kwbegin {
            return;
        }
        let items: Vec<Item> = stmts.iter().filter_map(|s| item(cx, s)).collect();
        let mut groups: Vec<Vec<&Item>> = Vec::new();
        for it in &items {
            match groups.iter_mut().find(|g| g[0].group == it.group) {
                Some(g) => g.push(it),
                None => groups.push(vec![it]),
            }
        }
        for group in groups.into_iter().filter(|g| g.len() > 1) {
            // uniq_items: no other stub of the same message on another line
            let uniq: Vec<&Item> = group
                .iter()
                .copied()
                .filter(|i| !group.iter().any(|j| j.message_key == i.message_key && j.line != i.line))
                .collect();
            let lines: Vec<usize> = uniq.iter().map(|i| i.line).collect();
            // arguments(uniq_items); a message that is not a plain label is withheld (spliced as code)
            let mut inert = true;
            let args: Vec<String> = uniq
                .iter()
                .map(|i| {
                    let key = if REQUIRES_QUOTES.is_match(&i.name) {
                        inert &= !i.name.contains(['\'', '\\']);
                        format!("'{}'", i.name)
                    } else {
                        inert &= LABEL.is_match(&i.name);
                        i.name.clone()
                    };
                    format!("{key}: {}", i.ret)
                })
                .collect();
            let replacement = format!("receive_messages({})", args.join(", "));
            for it in &uniq {
                let others: Vec<usize> = lines.iter().copied().filter(|&l| l != it.line).collect();
                if others.is_empty() {
                    continue;
                }
                let loc = format!("[{}]", others.iter().map(|l| l.to_string()).collect::<Vec<_>>().join(", "));
                let msg = format!("Use `receive_messages` instead of multiple stubs on lines {loc}.");
                let (s, e) = it.range;
                // The merged stub is withheld, and so is the removal of the others.
                if !inert {
                    cx.add_withheld_fix(s, e, msg);
                    continue;
                }
                if it.line > *others.iter().max().unwrap() {
                    let (rs, re) = it.receive;
                    let replacement = replacement.clone();
                    cx.add_fix(s, e, msg, |c| c.replace(rs, re, &replacement));
                } else {
                    let (rs, re) = helpers::whole_lines::range_by_whole_lines(cx.src.text, s, e, true);
                    cx.add_fix(s, e, msg, |c| c.remove(rs, re));
                }
            }
        }
    }
}
