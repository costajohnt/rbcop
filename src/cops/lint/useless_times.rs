use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Lint/UselessTimes";
pub static COP: UselessTimes = UselessTimes;
pub struct UselessTimes;

/// `orphans_loop_control_keyword?`: a next/break/redo in the body not nested in an inner
/// block/lambda/loop.
#[derive(Default)]
struct Orphans {
    depth: usize,
    found: bool,
}

impl<'pr> Visit<'pr> for Orphans {
    fn visit_branch_node_enter(&mut self, node: Node<'pr>) {
        use kind::*;
        if matches!(kind::of(&node), NEXT_NODE | BREAK_NODE) && self.depth == 0 {
            self.found = true;
        }
    }
    fn visit_leaf_node_enter(&mut self, node: Node<'pr>) {
        if matches!(kind::of(&node), kind::NEXT_NODE | kind::BREAK_NODE | kind::REDO_NODE) && self.depth == 0 {
            self.found = true;
        }
    }
    fn visit_block_node(&mut self, node: &ruby_prism::BlockNode<'pr>) {
        self.depth += 1;
        ruby_prism::visit_block_node(self, node);
        self.depth -= 1;
    }
    fn visit_lambda_node(&mut self, node: &ruby_prism::LambdaNode<'pr>) {
        self.depth += 1;
        ruby_prism::visit_lambda_node(self, node);
        self.depth -= 1;
    }
    fn visit_while_node(&mut self, node: &ruby_prism::WhileNode<'pr>) {
        self.depth += 1;
        ruby_prism::visit_while_node(self, node);
        self.depth -= 1;
    }
    fn visit_until_node(&mut self, node: &ruby_prism::UntilNode<'pr>) {
        self.depth += 1;
        ruby_prism::visit_until_node(self, node);
        self.depth -= 1;
    }
    fn visit_for_node(&mut self, node: &ruby_prism::ForNode<'pr>) {
        self.depth += 1;
        ruby_prism::visit_for_node(self, node);
        self.depth -= 1;
    }
}

/// `block_reassigns_arg?`: an `lvasgn` of the argument anywhere in the block.
struct Reassigns<'a> {
    name: &'a [u8],
    found: bool,
}

impl<'pr> Visit<'pr> for Reassigns<'_> {
    // Deliberate difference: RuboCop's `(lvasgn %)` misses `i = x` (an lvasgn with a value), so it
    // inlines the body as `0 = x`, a syntax error. rbcop counts it and leaves the block
    // (docs/known-gaps.md).
    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode<'pr>) {
        self.found |= node.name().as_slice() == self.name;
        ruby_prism::visit_local_variable_write_node(self, node);
    }
    fn visit_local_variable_target_node(&mut self, node: &ruby_prism::LocalVariableTargetNode<'pr>) {
        self.found |= node.name().as_slice() == self.name;
    }
    fn visit_local_variable_operator_write_node(&mut self, node: &ruby_prism::LocalVariableOperatorWriteNode<'pr>) {
        self.found |= node.name().as_slice() == self.name;
        ruby_prism::visit_local_variable_operator_write_node(self, node);
    }
    fn visit_local_variable_or_write_node(&mut self, node: &ruby_prism::LocalVariableOrWriteNode<'pr>) {
        self.found |= node.name().as_slice() == self.name;
        ruby_prism::visit_local_variable_or_write_node(self, node);
    }
    fn visit_local_variable_and_write_node(&mut self, node: &ruby_prism::LocalVariableAndWriteNode<'pr>) {
        self.found |= node.name().as_slice() == self.name;
        ruby_prism::visit_local_variable_and_write_node(self, node);
    }
}

enum Fix {
    None,
    Remove(usize, usize),
    Replace(usize, usize, String),
}

impl Cop for UselessTimes {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || method_name(&call) != "times" || call.arguments().is_some() {
            return;
        }
        let Some(int) = call.receiver().and_then(|r| r.as_integer_node()) else { return };
        let count: i64 = match node_text(&int.as_node()).replace('_', "").parse() {
            Ok(v) => v,
            Err(_) => return,
        };
        let mut proc_name: Option<String> = None;
        let mut block = None;
        match call.block() {
            None => {}
            Some(b) => match (b.as_block_argument_node(), b.as_block_node()) {
                (Some(ba), _) => match ba.expression().and_then(|e| e.as_symbol_node()) {
                    Some(s) => proc_name = Some(String::from_utf8_lossy(s.unescaped()).into_owned()),
                    None => return,
                },
                (_, Some(bn)) => block = Some(bn),
                _ => return,
            },
        }
        if count > 1 {
            return;
        }
        let (start, end) = if block.is_some() {
            let l = node.location();
            (l.start_offset(), l.end_offset())
        } else {
            send_range(&call)
        };
        let src = cx.src;
        let line_start = src.line_start(src.line_of(start));
        let own_line = src.text[line_start..start].chars().all(|c| matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0c' | '\x0b'));
        let n = cx.ancestors.len();
        let parent_send = match cx.ancestors.last().map(kind::of) {
            Some(kind::CALL_NODE) => cx.ancestors[n - 1].as_call_node().is_some_and(|c| {
                !is_csend(&c) && c.receiver().is_some_and(|r| r.location().start_offset() == start)
            }),
            Some(kind::ARGUMENTS_NODE) => {
                n >= 2 && cx.ancestors[n - 2].as_call_node().is_some_and(|c| !is_csend(&c))
            }
            _ => false,
        };
        let fix = if !own_line || parent_send {
            Fix::None
        } else {
            let block_type = block.as_ref().is_some_and(|b| b.parameters().is_none_or(|p| p.as_block_parameters_node().is_some()));
            let body_nil = block.as_ref().is_some_and(|b| b.body().is_none());
            if count < 1 || (block_type && body_nil) {
                let (s, e) = helpers::whole_lines::range_by_whole_lines(src.text, start, end, true);
                Fix::Remove(s, e)
            } else if let Some(p) = &proc_name {
                Fix::Replace(start, end, p.clone())
            } else if block_type {
                // None: RuboCop's corrector raises and the offense is lost.
                let Some(f) = self.block_fix(cx, block.as_ref().unwrap(), start, end) else { return };
                f
            } else {
                Fix::None
            }
        };
        cx.add_fix(start, end, format!("Useless call to `{count}.times` detected."), |c| match fix {
            Fix::None => {}
            Fix::Remove(s, e) => c.remove(s, e),
            Fix::Replace(s, e, t) => c.replace(s, e, &t),
        });
    }
}

impl UselessTimes {
    fn block_fix(&self, cx: &Cx, b: &ruby_prism::BlockNode, start: usize, end: usize) -> Option<Fix> {
        let params: Vec<Node> = b
            .parameters()
            .and_then(|p| p.as_block_parameters_node())
            .map(|bp| {
                let mut v: Vec<Node> = Vec::new();
                if let Some(p) = bp.parameters() {
                    v.extend(p.requireds().iter());
                    v.extend(p.optionals().iter());
                    v.extend(p.rest().filter(|r| r.as_implicit_rest_node().is_none()));
                    v.extend(p.posts().iter());
                    v.extend(p.keywords().iter());
                    v.extend(p.keyword_rest());
                    v.extend(p.block().map(|x| x.as_node()));
                }
                v.extend(bp.locals().iter());
                v
            })
            .unwrap_or_default();
        let block_arg = (params.len() == 1).then(|| params[0].as_required_parameter_node()).flatten().map(|r| r.name());
        if params.len() > 1 {
            return Some(Fix::None);
        }
        let Some(body) = b.body() else { return Some(Fix::None) };
        let mut orphans = Orphans::default();
        orphans.visit(&body);
        if orphans.found || (params.len() == 1 && block_arg.is_none()) {
            return Some(Fix::None);
        }
        if let Some(name) = &block_arg {
            let mut r = Reassigns { name: name.as_slice(), found: false };
            r.visit(&b.as_node());
            if r.found {
                return Some(Fix::None);
            }
        }
        let (bs, be) = body_range(&body);
        let mut source = cx.src.text[bs..be].to_string();
        if let Some(name) = &block_arg {
            let name = String::from_utf8_lossy(name.as_slice());
            // Deliberate difference: a name too long for the regex size limit (~100k characters)
            // leaves the offense uncorrected instead of failing the cop.
            let Ok(re) = Regex::new(&format!(r"\b{}\b", regex::escape(&name))) else { return Some(Fix::None) };
            source = re.replace_all(&source, "0").into_owned();
        }
        let (from, to) = (cx.src.column(start), cx.src.column(bs));
        let lines: Vec<&str> = source.split('\n').collect();
        let mut out: Vec<String> = Vec::new();
        for (i, line) in lines.iter().enumerate() {
            if i == 0 || line.is_empty() {
                out.push(line.to_string());
                continue;
            }
            let chars: Vec<char> = line.chars().collect();
            if from > chars.len() {
                // `line[range] = ''` raises IndexError.
                return None;
            }
            let mut s: String = chars[..from].iter().collect();
            s.extend(chars[to.clamp(from, chars.len())..].iter());
            out.push(s);
        }
        // String#split drops trailing empty strings.
        while out.len() > 1 && out.last().is_some_and(|l| l.is_empty()) {
            out.pop();
        }
        Some(Fix::Replace(start, end, out.join("\n")))
    }
}
