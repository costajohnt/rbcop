use crate::helpers::parser_parent::defines_module;
use crate::prelude::*;

pub const NAME: &str = "Lint/ConstantResolution";
pub static COP: ConstantResolution = ConstantResolution;
pub struct ConstantResolution;

impl Cop for ConstantResolution {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CONSTANT_READ_NODE]
    }
    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let name = String::from_utf8_lossy(node.as_constant_read_node().unwrap().name().as_slice()).into_owned();
        // `(allowed_names.empty? || allowed_names.include?(name)) && !ignored_names.include?(name)`
        // on the bare settings: a String is a substring test.
        let only_empty = match cx.cfg.get("Only") {
            None => panic!("undefined method 'empty?' for nil"),
            Some(Value::Str(s)) => s.is_empty(),
            Some(Value::Seq(items)) => items.is_empty(),
            Some(Value::Map(m)) => m.keys().next().is_none(),
            Some(_) => panic!("undefined method 'empty?' for Only"),
        };
        if !(only_empty || cx.cfg.includes("Only", &name)) || cx.cfg.includes_bare("Ignore", &name) {
            return;
        }
        if defines_module(cx.ancestors) {
            return;
        }
        // With the project index, only constants whose lexical resolution differs from the
        // top-level one (`ambiguous_resolution?`).
        if let Some(index) = crate::project_index::get() {
            let nesting = crate::project_index::lexical_nesting(cx.ancestors, node.location().start_offset());
            let relative = index.resolve_constant(&name, &nesting);
            let absolute = index.resolve_constant(&name, &[]);
            let ix = index.read();
            let ambiguous = matches!((relative, absolute), (Some(r), Some(a)) if ix.name(r) != ix.name(a));
            if !ambiguous {
                return;
            }
        }
        let loc = node.location();
        let (start, mut end) = (loc.start_offset(), loc.end_offset());
        // The omitted value of `{ Foo: }` is a parser `const` spanning the bare name; Prism's
        // ImplicitNode and the ConstantReadNode inside it include the colon (#315).
        if cx.parent().is_some_and(|p| p.as_implicit_node().is_some()) && cx.src.text.as_bytes()[end - 1] == b':' {
            end -= 1;
        }
        cx.add(start, end, "Fully qualify this constant to avoid possibly ambiguous resolution.");
    }
}
