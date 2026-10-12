use crate::prelude::*;

pub const NAME: &str = "Rails/MigrationClassName";
pub static COP: MigrationClassName = MigrationClassName;
pub struct MigrationClassName;

impl Cop for MigrationClassName {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CLASS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let class = node.as_class_node().unwrap();
        if !helpers::rails::is_migration_class(&class) {
            return;
        }
        let path = std::path::Path::new(&cx.src.path);
        let base = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let base = base.strip_suffix(".rb").unwrap_or(&base).to_string();
        // remove_gem_suffix: sub(/\..+\z/, '')
        let base = match base.find('.') {
            Some(i) if i + 1 < base.len() => base[..i].to_string(),
            _ => base,
        };
        // sub(/\A\d+_/, '')
        let digits = base.bytes().take_while(u8::is_ascii_digit).count();
        let base = if digits > 0 && base.as_bytes().get(digits) == Some(&b'_') { base[digits + 1..].to_string() } else { base };
        let camelized: String = base.split('_').map(helpers::rails::capitalize).collect();
        let cp = class.constant_path();
        let (s, e) = match cp.as_constant_path_node() {
            Some(p) => (p.name_loc().start_offset(), p.name_loc().end_offset()),
            None => (cp.location().start_offset(), cp.location().end_offset()),
        };
        if cx.src.slice(s, e).to_ascii_lowercase() == camelized.to_ascii_lowercase()
            && cx.src.slice(s, e).len() == camelized.len()
        {
            return;
        }
        let msg = format!("Replace with `{camelized}` that matches the file name.");
        // Deliberate difference: a file name that does not camelize to a constant name is not spliced in.
        static CONST: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A[A-Z][A-Za-z0-9_]*\z").unwrap());
        if !CONST.is_match(&camelized) {
            return cx.add_withheld_fix(s, e, msg);
        }
        cx.add_fix(s, e, msg, |c| c.replace(s, e, &camelized));
    }
}
