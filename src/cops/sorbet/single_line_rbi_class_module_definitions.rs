use crate::prelude::*;

pub const NAME: &str = "Sorbet/SingleLineRbiClassModuleDefinitions";
pub static COP: SingleLineRbiClassModuleDefinitions = SingleLineRbiClassModuleDefinitions;
pub struct SingleLineRbiClassModuleDefinitions;

static NEWLINES: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\r\n]+[ \t\r\n\x0b\x0c]*[\r\n]*").unwrap());

impl Cop for SingleLineRbiClassModuleDefinitions {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CLASS_NODE, kind::MODULE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let empty = match node.as_class_node() {
            Some(c) => c.body().is_none(),
            None => node.as_module_node().unwrap().body().is_none(),
        };
        let (s, e) = helpers::node_types::range(node);
        if !empty || cx.src.line_of(s) == cx.src.line_of(e) {
            return;
        }
        let fixed = NEWLINES.replacen(cx.src.slice(s, e), 1, "; ").into_owned();
        cx.add_fix(s, e, "Empty class/module definitions in RBI files should be on a single line.", |c| {
            c.replace(s, e, &fixed)
        });
    }
}
