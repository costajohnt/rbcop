use crate::prelude::*;

pub const NAME: &str = "Bundler/GemFilename";
pub static COP: GemFilename = GemFilename;
pub struct GemFilename;

impl Cop for GemFilename {
    fn on_file(&self, cx: &mut Cx) {
        let path = cx.src.path;
        let basename = std::path::Path::new(path).file_name().and_then(|n| n.to_str()).unwrap_or("");
        let msg = match (cx.cfg.str("EnforcedStyle"), basename) {
            (Some("Gemfile"), "gems.rb") => "`gems.rb` file was found but `Gemfile` is required",
            (Some("Gemfile"), "gems.locked") => "Expected a `Gemfile.lock` with `Gemfile` but found `gems.locked` file",
            (Some("gems.rb"), "Gemfile") => "`Gemfile` was found but `gems.rb` file is required",
            (Some("gems.rb"), "Gemfile.lock") => "Expected a `gems.locked` file with `gems.rb` but found `Gemfile.lock`",
            _ => return,
        };
        // processed_source.file_path: RuboCop resolves the real absolute path.
        let full = std::fs::canonicalize(path).map_or_else(|_| path.to_string(), |p| p.display().to_string());
        // add_global_offense: RuboCop reports it at line 1, column 1 with no source range.
        cx.add_global(format!("{msg} (file path: {full})."));
    }
}
