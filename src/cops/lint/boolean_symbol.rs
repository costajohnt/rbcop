use crate::prelude::*;

pub const NAME: &str = "Lint/BooleanSymbol";
pub static COP: BooleanSymbol = BooleanSymbol;
pub struct BooleanSymbol;

impl Cop for BooleanSymbol {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::SYMBOL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let sym = node.as_symbol_node().unwrap();
        let value = sym.unescaped();
        if value != b"true" && value != b"false" {
            return;
        }
        let parent = cx.parent();
        if let Some(arr) = parent.and_then(|p| p.as_array_node())
            && arr.opening_loc().is_some_and(|o| o.as_slice().starts_with(b"%i") || o.as_slice().starts_with(b"%I"))
        {
            return;
        }
        let loc = node.location();
        let (start, mut end) = (loc.start_offset(), loc.end_offset());
        // A label `true:` / `"true":` is a sym without the trailing colon in parser.
        let label = sym.closing_loc().is_some_and(|c| c.as_slice().ends_with(b":"));
        if label {
            end -= 1;
        }
        // pair.colon? and the sym is the key: the colon is the pair's operator.
        let key_of_colon_pair =
            label && parent.and_then(|p| p.as_assoc_node()).is_some_and(|a| a.key().location().start_offset() == start);
        let source = cx.src.slice(start, end);
        let msg = format!("Symbol with a boolean name - you probably meant to use `{}`.", String::from_utf8_lossy(value));
        let replacement = if key_of_colon_pair { format!("{source} =>") } else { source.replace(':', "") };
        cx.add_fix(start, end, msg, |c| {
            if key_of_colon_pair {
                c.remove(end, end + 1);
            }
            c.replace(start, end, &replacement);
        });
    }
}
