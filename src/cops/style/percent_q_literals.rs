use crate::prelude::*;

pub const NAME: &str = "Style/PercentQLiterals";
pub static COP: PercentQLiterals = PercentQLiterals;
pub struct PercentQLiterals;

/// The unescaped value of `src` parsed on its own, when it is a plain string literal (parser `str`).
fn str_value(src: &[u8]) -> Option<Vec<u8>> {
    let result = ruby_prism::parse(src);
    if result.errors().next().is_some() {
        return None;
    }
    let program = result.node();
    let stmts = program.as_program_node()?.statements();
    let mut body = stmts.body().iter();
    let only = body.next()?;
    if body.next().is_some() || is_parser_dstr(&only) {
        return None;
    }
    only.as_string_node().map(|s| s.unescaped().to_vec())
}

impl Cop for PercentQLiterals {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::STRING_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let s = node.as_string_node().unwrap();
        let Some(open) = s.opening_loc() else { return };
        let ob = open.as_slice();
        if ob.len() < 2 || ob[0] != b'%' || !matches!(ob[1], b'q' | b'Q') || is_parser_dstr(node) {
            return;
        }
        let lower = cx.cfg.str("EnforcedStyle") != Some("upper_case_q");
        if (ob[1] == b'q') == lower {
            return;
        }
        let mut corrected = node.location().as_slice().to_vec();
        corrected[1] ^= 0x20;
        if str_value(&corrected).as_deref() != Some(s.unescaped()) {
            return;
        }
        let msg = if lower {
            "Do not use `%Q` unless interpolation is needed. Use `%q`."
        } else {
            "Use `%Q` instead of `%q`."
        };
        let loc = node.location();
        let (ns, ne) = (loc.start_offset(), loc.end_offset());
        let Ok(corrected) = String::from_utf8(corrected) else { return };
        cx.add_fix(open.start_offset(), open.end_offset(), msg, |c| c.replace(ns, ne, &corrected));
    }
}
