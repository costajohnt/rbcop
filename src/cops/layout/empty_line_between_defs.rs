use crate::prelude::*;
use helpers::macro_scope::is_macro;

pub const NAME: &str = "Layout/EmptyLineBetweenDefs";
pub static COP: EmptyLineBetweenDefs = EmptyLineBetweenDefs;
pub struct EmptyLineBetweenDefs;

/// What RuboCop needs to know about a def-like statement.
struct Candidate {
    /// node type for the message (`method`, `class`, `module`, `block`, `send`)
    kind: &'static str,
    /// first line of `def`/`class`/`module` keyword, or of the macro send
    start_line: usize,
    /// offense range (def_location)
    loc: (usize, usize),
    /// node source range
    range: (usize, usize),
}

fn candidate(cx: &Cx, node: &Node, parents: &[&Node]) -> Option<Candidate> {
    let cfg = &cx.cfg.params;
    let src = cx.src;
    let loc = node.location();
    let range = (loc.start_offset(), loc.end_offset());
    let keyword = |kw: ruby_prism::Location, name_end: usize, kind| {
        Some(Candidate { kind, start_line: src.line_of(kw.start_offset()), loc: (kw.start_offset(), name_end), range })
    };
    match kind::of(node) {
        kind::DEF_NODE if cfg.truthy("EmptyLineBetweenMethodDefs") => {
            let d = node.as_def_node().unwrap();
            keyword(d.def_keyword_loc(), d.name_loc().end_offset(), "method")
        }
        kind::CLASS_NODE if cfg.truthy("EmptyLineBetweenClassDefs") => {
            let c = node.as_class_node().unwrap();
            keyword(c.class_keyword_loc(), c.constant_path().location().end_offset(), "class")
        }
        kind::MODULE_NODE if cfg.truthy("EmptyLineBetweenModuleDefs") => {
            let m = node.as_module_node().unwrap();
            keyword(m.module_keyword_loc(), m.constant_path().location().end_offset(), "module")
        }
        kind::CALL_NODE => {
            let call = node.as_call_node().unwrap();
            // `macro? && cop_config.fetch('DefLikeMacros', []).map(...)`: a String raises (a cop
            // error) once a macro is a candidate.
            if !is_macro(&call, parents) || !cx.cfg.array("DefLikeMacros").iter().any(|m| m == method_name(&call)) {
                return None;
            }
            let send = send_range(&call);
            let block = call.block().is_some_and(|b| b.as_block_node().is_some());
            let kind = if block { "block" } else { "send" };
            Some(Candidate { kind, start_line: src.line_of(send.0), loc: send, range })
        }
        _ => None,
    }
}

impl Cop for EmptyLineBetweenDefs {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::STATEMENTS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // parser `begin`: a body of two or more statements
        let body: Vec<Node> = node.as_statements_node().unwrap().body().iter().collect();
        if body.len() < 2 {
            return;
        }
        // A plain `begin ... end` is a `kwbegin` holding the statements itself (no `begin` node).
        let kwbegin = cx.ancestors.last().and_then(|p| p.as_begin_node()).is_some_and(|b| {
            b.begin_keyword_loc().is_some() && b.rescue_clause().is_none() && b.ensure_clause().is_none() && b.else_clause().is_none()
        });
        if kwbegin {
            return;
        }
        let parents: Vec<&Node> = cx.ancestors.iter().chain([node]).collect();
        let (min, max) = match cx.cfg.get("NumberOfEmptyLines") {
            Some(Value::Seq(v)) => {
                let int = |x: Option<&Value>| match x {
                    Some(Value::Int(i)) => *i,
                    _ => 1,
                };
                (int(v.first()), int(v.last()))
            }
            _ => {
                let n = cx.cfg.int("NumberOfEmptyLines").unwrap_or(1);
                (n, n)
            }
        };
        let src = cx.src;
        let mut prev: Option<(Candidate, &Node)> = None;
        for stmt in &body {
            let cur = candidate(cx, stmt, &parents);
            if let (Some((p, pnode)), Some(c)) = (&prev, &cur) {
                // def_end: the node's last line, or a trailing heredoc's terminator line
                let mut end = p.range.1;
                // end_loc(prev).end_pos: heredoc_end excludes the terminator's newline.
                let mut end_pos = end;
                if let Some((hs, he)) = helpers::heredoc::last_heredoc_end(pnode)
                    && he > end
                {
                    end = hs;
                    end_pos = he - usize::from(src.text[..he].ends_with('\n'));
                }
                let from = src.line_of(end); // 0-based index of the line after def_end
                let to = c.start_line as isize - 2;
                let lines: &[&str] =
                    if to < from as isize { &[] } else { &src.lines[from.min(src.lines.len())..=(to as usize)] };
                let blank = |l: &str| helpers::ruby_string::strip(l).is_empty();
                let count = lines.iter().filter(|l| blank(l)).count() as i64;
                let blank_max = lines.iter().rposition(|l| blank(l));
                let non_blank_min = lines.iter().position(|l| !blank(l));
                let groups = matches!((blank_max, non_blank_min), (Some(b), Some(n)) if b > n);
                let one_line = |r: (usize, usize)| src.line_of(r.0) == src.line_of(r.1);
                let adjacent_ok = one_line(p.range) && one_line(c.range) && cx.cfg.params.truthy("AllowAdjacentOneLineDefs");
                if !(min..=max).contains(&count) && !groups && !adjacent_ok {
                    let expected = if min != max {
                        format!("{min}..{max} empty lines")
                    } else {
                        format!("{max} empty line{}", if max == 1 { "" } else { "s" })
                    };
                    let msg = format!("Expected {expected} between {} definitions; found {count}.", c.kind);
                    // RuboCop crashes (nil > Integer) without a newline after the previous def.
                    if let Some(nl) = src.text[end_pos..].find('\n').map(|i| end_pos + i) {
                        let begin = c.range.0;
                        let nl = if nl > begin { begin - 1 } else { nl };
                        cx.add_fix(c.loc.0, c.loc.1, msg, |cr| {
                            if count > max {
                                cr.remove(nl, nl + (count - max) as usize);
                            } else {
                                cr.insert_after(nl, nl + 1, &"\n".repeat((min - count) as usize));
                            }
                        });
                    }
                }
            }
            prev = cur.map(|c| (c, stmt));
        }
    }
}
