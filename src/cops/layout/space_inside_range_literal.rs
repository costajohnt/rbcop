use crate::prelude::*;

pub const NAME: &str = "Layout/SpaceInsideRangeLiteral";
pub static COP: SpaceInsideRangeLiteral = SpaceInsideRangeLiteral;
pub struct SpaceInsideRangeLiteral;

// Ruby's \s is ASCII whitespace only.
static IRANGE_NL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\.\.\n[ \t\n\x0B\x0C\r]*").unwrap());
static ERANGE_NL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\.\.\.\n[ \t\n\x0B\x0C\r]*").unwrap());
static IRANGE_SPACE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[ \t\n\x0B\x0C\r]\.\.|\.\.[ \t\n\x0B\x0C\r]").unwrap());
static ERANGE_SPACE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[ \t\n\x0B\x0C\r]\.\.\.|\.\.\.[ \t\n\x0B\x0C\r]").unwrap());

impl Cop for SpaceInsideRangeLiteral {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::RANGE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let range = node.as_range_node().unwrap();
        let (nl, space, op) = if range.is_exclude_end() {
            (&*ERANGE_NL, &*ERANGE_SPACE, "...")
        } else {
            (&*IRANGE_NL, &*IRANGE_SPACE, "..")
        };
        let loc = node.location();
        // A line break right after the operator (plus the next line's indentation) is fine.
        let expression = nl.replace(node_text(node), op);
        if space.is_match(&expression) {
            cx.add(loc.start_offset(), loc.end_offset(), "Space inside range literal.");
        }
    }
}
