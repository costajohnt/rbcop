use crate::cops::sorbet_forbid_t_let::autocorrectable;
use crate::helpers::sorbet_rbi_type::{Error, rbs_of};
use crate::prelude::*;

pub const NAME: &str = "Sorbet/ForbidTTypeAlias";
pub static COP: ForbidTTypeAlias = ForbidTTypeAlias;
pub struct ForbidTTypeAlias;

const MSG: &str = "Do not use `T.type_alias`.";

static ACRONYM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"([A-Z]+)([A-Z][a-z])").unwrap());
static CAMEL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"([a-z0-9])([A-Z])").unwrap());
static SKIP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A#[:|]|(?-u:\b)rubocop[ \t\r\n\x0b\x0c]*:").unwrap());

impl Cop for ForbidTTypeAlias {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let Some(block) = call.block().and_then(|b| b.as_block_node()) else { return };
        if block.parameters().is_some_and(|p| p.as_numbered_parameters_node().is_some() || p.as_it_parameters_node().is_some())
            || method_name(&call) != "type_alias"
            || call.arguments().is_some()
            || !call.receiver().is_some_and(|r| r.as_constant_read_node().is_some_and(|k| k.name().as_slice() == b"T"))
        {
            return;
        }
        let loc = node.location();
        let (s, e) = (loc.start_offset(), loc.end_offset());
        let mut fix = None;
        match self.correction(cx, &block, (s, e)) {
            Ok(f) => fix = f,
            Err(Error::Type) => {}
            Err(Error::Other) => return crate::runner::cop_error_at(NAME, cx.src, crate::runner::visiting()),
        }
        cx.add_fix(s, e, MSG, |c| {
            if let Some((rs, re, t)) = &fix {
                c.replace(*rs, *re, t);
            }
        });
    }
}

impl ForbidTTypeAlias {
    fn correction(
        &self,
        cx: &Cx,
        block: &ruby_prism::BlockNode,
        r: (usize, usize),
    ) -> Result<Option<(usize, usize, String)>, Error> {
        let anc = cx.ancestors;
        let Some(pi) = helpers::parser_parent::parser_parent_index(anc, anc.len()) else { return Ok(None) };
        let Some(assignment) = anc[pi].as_constant_write_node() else { return Ok(None) };
        // standalone_alias?
        let mut parent = helpers::parser_parent::parser_parent(&anc[..pi]);
        if let Some(p) = parent
            && matches!(p, Node::StatementsNode { .. } | Node::ParenthesesNode { .. })
        {
            if p.as_parentheses_node().is_some() {
                return Ok(None);
            }
            let i = (0..pi).rev().find(|&k| std::ptr::eq(&anc[k], p)).unwrap();
            parent = helpers::parser_parent::parser_parent(&anc[..i]);
        }
        if !parent.is_none_or(|p| matches!(p, Node::ClassNode { .. } | Node::ModuleNode { .. })) {
            return Ok(None);
        }
        if block.parameters().is_some() {
            return Ok(None);
        }
        let Some(body) = block.body() else { return Ok(None) };
        if !autocorrectable(cx, r, Some(&anc[pi]), true) {
            return Ok(None);
        }
        let name = String::from_utf8_lossy(assignment.name().as_slice()).into_owned();
        let name = CAMEL.replace_all(&ACRONYM.replace_all(&name, "${1}_${2}"), "${1}_${2}").to_lowercase();
        let ty = rbs_of(node_text(&body))?;
        let declaration = format!("type {name} = {ty}");
        let src = cx.src;
        let aloc = assignment.as_node().location();
        let (mut rs, mut re) = (aloc.start_offset(), aloc.end_offset());
        let (first, last) = (src.line_of(rs), src.line_of(re));
        let comments: Vec<&str> = src
            .comments
            .iter()
            .filter(|c| (first..=last).contains(&src.line_of(c.start)) && c.start >= rs)
            .map(|c| src.slice(c.start, c.end))
            .collect();
        if comments.iter().any(|t| SKIP.is_match(t)) {
            return Ok(None);
        }
        let line_start = src.line_start(first);
        let indentation: String = src.text[line_start..].chars().take_while(|c| *c == ' ' || *c == '\t').collect();
        let mut parts: Vec<String> = comments.iter().map(|t| t.to_string()).collect();
        parts.push(format!("#: {declaration}"));
        let replacement = parts.join(&format!("\n{indentation}"));
        if let Some(lc) = src.comments.iter().filter(|c| (first..=last).contains(&src.line_of(c.start)) && c.start >= rs).last()
            && lc.end > re
        {
            rs = rs.min(lc.start);
            re = lc.end;
        }
        Ok(Some((rs, re, replacement)))
    }
}
