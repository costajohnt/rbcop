use crate::prelude::*;
use crate::source::Source;
use helpers::alignment_corrector::{self, Settings};
use helpers::parser_parent::parser_parent;
use ruby_prism::Visit;

pub const NAME: &str = "Style/ClassAndModuleChildren";
pub static COP: ClassAndModuleChildren = ClassAndModuleChildren;
pub struct ClassAndModuleChildren;

const NESTED_MSG: &str = "Use nested module/class definitions instead of compact style.";
const COMPACT_MSG: &str = "Use compact module/class definition instead of nested style.";

fn span(n: &Node) -> (usize, usize) {
    (n.location().start_offset(), n.location().end_offset())
}

/// A class or module: (is_class, keyword, identifier, superclass?, body, end keyword).
struct Def<'pr> {
    class: bool,
    keyword: (usize, usize),
    ident: Node<'pr>,
    superclass: Option<Node<'pr>>,
    body: Option<Node<'pr>>,
    end_kw: (usize, usize),
}

fn def<'pr>(n: &Node<'pr>) -> Option<Def<'pr>> {
    let l = |loc: ruby_prism::Location| (loc.start_offset(), loc.end_offset());
    if let Some(c) = n.as_class_node() {
        return Some(Def {
            class: true,
            keyword: l(c.class_keyword_loc()),
            ident: c.constant_path(),
            superclass: c.superclass(),
            body: c.body(),
            end_kw: l(c.end_keyword_loc()),
        });
    }
    let m = n.as_module_node()?;
    Some(Def {
        class: false,
        keyword: l(m.module_keyword_loc()),
        ident: m.constant_path(),
        superclass: None,
        body: m.body(),
        end_kw: l(m.end_keyword_loc()),
    })
}

/// The parser body when it is a single class/module statement.
fn single_def<'pr>(body: &Option<Node<'pr>>) -> Option<Node<'pr>> {
    let s = body.as_ref()?.as_statements_node()?;
    let mut it = s.body().iter();
    let only = it.next()?;
    if it.next().is_some() {
        return None;
    }
    (only.as_class_node().is_some() || only.as_module_node().is_some()).then_some(only)
}

/// rubocop-ast `const_name` (a cbase is dropped, a non-constant namespace prints as "").
fn const_name(n: &Node) -> String {
    if let Some(c) = n.as_constant_read_node() {
        return String::from_utf8_lossy(c.name().as_slice()).into_owned();
    }
    if let Some(p) = n.as_constant_path_node() {
        let name = p.name().map(|x| String::from_utf8_lossy(x.as_slice()).into_owned()).unwrap_or_default();
        return match p.parent() {
            Some(parent) => format!("{}::{name}", const_name(&parent)),
            None => name,
        };
    }
    String::new()
}

/// `node.source_range.source_line[/\A\s*/]`
fn leading_spaces<'s>(src: &Source<'s>, off: usize) -> &'s str {
    let line = src.lines.get(src.line_of(off) - 1).copied().unwrap_or("");
    let n = line.len() - line.trim_start_matches([' ', '\t', '\r', '\x0b', '\x0c']).len();
    &line[..n]
}

struct FindClass<'a> {
    target: &'a str,
    found: bool,
}

impl<'pr> Visit<'pr> for FindClass<'_> {
    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode<'pr>) {
        if node_text(&node.constant_path()) == self.target {
            self.found = true;
        }
        ruby_prism::visit_class_node(self, node);
    }
}

impl Cop for ClassAndModuleChildren {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CLASS_NODE, kind::MODULE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let d = def(node).unwrap();
        let style = cx.cfg.str("EnforcedStyle").unwrap_or("nested").to_string();
        let for_classes = cx.cfg.str("EnforcedStyleForClasses").map_or(style.clone(), str::to_string);
        let for_modules = cx.cfg.str("EnforcedStyleForModules").map_or(style.clone(), str::to_string);
        let style_for = |class: bool| if class { for_classes.as_str() } else { for_modules.as_str() };
        if d.class && d.superclass.is_some() && for_classes != "nested" {
            return;
        }
        let my_style = style_for(d.class);
        // check_style: no `::Foo`, and a namespace made of constants only.
        let namespace = d.ident.as_constant_path_node().map(|p| p.parent());
        if matches!(namespace, Some(None)) {
            return;
        }
        let mut ns = namespace.flatten();
        while let Some(n) = ns {
            if n.as_constant_read_node().is_some() {
                break;
            }
            let Some(p) = n.as_constant_path_node() else { return };
            ns = p.parent();
        }
        let parent = parser_parent(cx.ancestors);
        if parent.is_some_and(|p| p.as_class_node().is_some() || p.as_module_node().is_some()) {
            return;
        }
        let src = cx.src;
        let name = span(&d.ident);
        let width = helpers::alignment::configured_indentation_width(cx);
        let (start, end) = span(node);
        if my_style == "nested" {
            if !node_text(&d.ident).contains("::") {
                return;
            }
            // heuristic_namespace_keyword: a class named like the namespace in the left sibling.
            let ns_text = d.ident.as_constant_path_node().and_then(|p| p.parent()).map(|p| node_text(&p).to_string());
            let left = parent.and_then(|p| p.as_statements_node()).and_then(|s| {
                let stmts: Vec<Node> = s.body().iter().collect();
                let i = stmts.iter().position(|x| span(x) == (start, end))?;
                i.checked_sub(1).map(|j| span(&stmts[j]))
            });
            let indexed = indexed_namespace_keyword(cx, &d.ident);
            let mut keyword = indexed.unwrap_or("module");
            if indexed.is_none()
                && let (Some(ns_text), Some(_)) = (&ns_text, left)
            {
                let s = parent.unwrap().as_statements_node().unwrap();
                let prev = s.body().iter().find(|x| Some(span(x)) == left).unwrap();
                let mut f = FindClass { target: ns_text, found: false };
                f.visit(&prev);
                if f.found {
                    keyword = "class";
                }
            }
            let edits = (style_for(keyword == "class") != "compact").then(|| {
                let lead = leading_spaces(src, start);
                let padding = format!("{}{lead}", " ".repeat(src.column(start) + width as usize));
                let end_col = " ".repeat(src.column(d.end_kw.0));
                let trailing = padding.replacen(&end_col, "", 1);
                let dc = d.ident.as_constant_path_node().unwrap().delimiter_loc();
                let orig_kw = src.slice(d.keyword.0, d.keyword.1);
                vec![
                    (d.keyword, keyword.to_string()),
                    ((dc.start_offset(), dc.end_offset()), format!("\n{padding}{orig_kw} ")),
                    (d.end_kw, format!("{trailing}end\n{lead}end")),
                ]
            });
            cx.add_fix(name.0, name.1, NESTED_MSG, |c| {
                for (r, t) in edits.iter().flatten() {
                    c.replace(r.0, r.1, t);
                }
            });
            return;
        }
        let Some(inner) = single_def(&d.body) else { return };
        let b = def(&inner).unwrap();
        // An enclosing definition this cop compacted (ignore_node) is not corrected again.
        let outer: Vec<(usize, usize)> = cx
            .ancestors
            .iter()
            .filter(|a| a.as_class_node().is_some() || a.as_module_node().is_some())
            .map(span)
            .collect();
        let inside_compacted = {
            let ignored = cx.state::<Vec<(usize, usize)>>();
            outer.iter().any(|r| ignored.contains(r))
        };
        let compacting =
            style_for(b.class) != "nested" && compactible_namespace(cx, node, &d.ident) && !inside_compacted;
        if compacting && cx.block_runs(name.0, name.1) {
            cx.state::<Vec<(usize, usize)>>().push((start, end));
        }
        let fix = compacting.then(|| {
            // compact_replacement, with the comments the associator gives the inner node.
            let mut texts: Vec<&str> = Vec::new();
            let after_line = src.line_of(name.1);
            for cm in helpers::comment_association::leading(src, span(&inner).0, name.1, Some(after_line)) {
                texts.push(src.slice(cm.start, cm.end));
            }
            let last_child_end = [Some(span(&b.ident)), b.superclass.as_ref().map(span), b.body.as_ref().map(body_range)]
                .into_iter()
                .flatten()
                .map(|r| r.1)
                .max()
                .unwrap();
            let (is, ie) = span(&inner);
            for cm in &src.comments {
                let interior = cm.start >= last_child_end
                    && src.line_of(cm.start) > src.line_of(last_child_end)
                    && cm.end <= ie;
                let decorating = cm.start >= ie && src.line_of(cm.start) == src.line_of(ie);
                if interior || decorating {
                    texts.push(src.slice(cm.start, cm.end));
                }
            }
            let kw = if b.class { "class" } else { "module" };
            texts.push("");
            let header = format!("{kw} {}::{}", const_name(&d.ident), const_name(&b.ident));
            let last = texts.len() - 1;
            let mut joined: Vec<String> = texts.iter().map(|s| s.to_string()).collect();
            joined[last] = header;
            let replacement = joined.join("\n");
            let inner_name = span(&b.ident);
            // remove_end
            let raw = src.text.as_bytes();
            let remove = if src.line_of(inner_name.1) == src.line_of(b.end_kw.0) {
                // single_line_end_range: the `end` with the spaces and one `;` before it
                let mut pos = b.end_kw.0;
                while pos > 0 && raw[pos - 1] == b' ' {
                    pos -= 1;
                }
                if pos > 0 && raw[pos - 1] == b';' {
                    pos -= 1;
                }
                (pos, b.end_kw.1)
            } else {
                let remove_begin = b.end_kw.0 - leading_spaces(src, is).len();
                let adjust = if raw.get(remove_begin) == Some(&b';') { 0 } else { 1 };
                (remove_begin, b.end_kw.1 + adjust)
            };
            // unindent: only to the left, over the inner definition's body lines
            let tab = |s: &str| s.chars().map(|ch| if ch == '\t' { width } else { 1 }).sum::<i64>();
            let delta = b.body.as_ref().map(|bb| body_range(bb).0).and_then(|first| {
                let delta = tab(leading_spaces(src, start)) + width - tab(leading_spaces(src, first));
                let (first_line, last_line) = (src.line_of(inner_name.0) + 1, src.line_of(b.end_kw.0) - 1);
                (delta < 0 && first_line <= last_line).then(|| {
                    let line_end = |l: usize| src.line_start(l) + src.lines.get(l - 1).map_or(0, |t| t.len());
                    (delta, (src.line_start(first_line), line_end(last_line)))
                })
            });
            ((d.keyword.0, inner_name.1), replacement, remove, delta)
        });
        let set = Settings::of(cx);
        cx.add_fix(name.0, name.1, COMPACT_MSG, |c| {
            let Some((head, replacement, remove, delta)) = &fix else { return };
            c.replace(head.0, head.1, replacement);
            c.remove(remove.0, remove.1.min(src.text.len()));
            // a range, not a node: every string of the source is taboo
            if let (Some((delta, (s, e))), Some(root)) = (delta, &src.root) {
                alignment_corrector::correct(c, src, set, *s, *e, Some(root), *delta, true);
            }
        });
    }
}

/// `indexed_namespace_keyword`: with the project index, the keyword of the declaration the
/// compact name's namespace resolves to.
fn indexed_namespace_keyword(cx: &Cx, ident: &Node) -> Option<&'static str> {
    let index = crate::project_index::get()?;
    let namespace = ident.as_constant_path_node()?.parent()?;
    let decl = crate::project_index::resolve_const_node(index, &namespace, cx.ancestors)?;
    let ix = index.read();
    if ix.is_class(decl) {
        Some("class")
    } else if ix.is_module(decl) {
        Some("module")
    } else {
        None
    }
}

/// `compactible_namespace?`: without the project index, always; with it, the outer namespace
/// must also be defined somewhere else (another file, or another line).
fn compactible_namespace(cx: &Cx, node: &Node, ident: &Node) -> bool {
    let Some(index) = crate::project_index::get() else { return true };
    let Some(decl) = crate::project_index::resolve_const_node(index, ident, cx.ancestors) else { return false };
    let ix = index.read();
    if !ix.is_namespace(decl) {
        return false;
    }
    let current = cx.src.absolute_path();
    let first_line = cx.src.line_of(node.location().start_offset()) as u32;
    ix.definitions(decl).into_iter().any(|def| match ix.location(def) {
        Some((None, _)) => true,
        Some((Some(path), line)) => {
            !crate::project_index::same_file(&path, std::path::Path::new(&current)) || line != first_line
        }
        None => false,
    })
}
