use crate::prelude::*;
use helpers::parser_parent::is_top_const;

pub const NAME: &str = "Rails/RootPathnameMethods";
pub static COP: RootPathnameMethods = RootPathnameMethods;
pub struct RootPathnameMethods;

const DIR_GLOB_METHODS: &[&str] = &["[]", "glob"];
#[rustfmt::skip]
const DIR_NON_GLOB_METHODS: &[&str] = &[
    "children", "delete", "each_child", "empty?", "entries", "exist?", "mkdir", "open", "rmdir", "unlink",
];
#[rustfmt::skip]
const FILE_METHODS: &[&str] = &[
    "atime", "basename", "binread", "binwrite", "birthtime", "blockdev?", "chardev?", "chmod", "chown", "ctime",
    "delete", "directory?", "dirname", "empty?", "executable?", "executable_real?", "exist?", "expand_path",
    "extname", "file?", "fnmatch", "fnmatch?", "ftype", "grpowned?", "join", "lchmod", "lchown", "lstat", "mtime",
    "open", "owned?", "pipe?", "read", "readable?", "readable_real?", "readlines", "readlink", "realdirpath",
    "realpath", "rename", "setgid?", "setuid?", "size", "size?", "socket?", "split", "stat", "sticky?", "symlink?",
    "sysopen", "truncate", "unlink", "utime", "world_readable?", "world_writable?", "writable?", "writable_real?",
    "write", "zero?",
];
#[rustfmt::skip]
const FILE_TEST_METHODS: &[&str] = &[
    "blockdev?", "chardev?", "directory?", "empty?", "executable?", "executable_real?", "exist?", "file?",
    "grpowned?", "owned?", "pipe?", "readable?", "readable_real?", "setgid?", "setuid?", "size", "size?", "socket?",
    "sticky?", "symlink?", "world_readable?", "world_writable?", "writable?", "writable_real?", "zero?",
];
const FILE_UTILS_METHODS: &[&str] = &["chmod", "chown", "mkdir", "mkpath", "rmdir", "rmtree"];

/// (send (const {nil? cbase} :Rails) {:root :public_path})
fn rails_root(n: &Node) -> bool {
    helpers::rails::send_node(n).is_some_and(|c| {
        matches!(method_name(&c), "root" | "public_path")
            && send_args(&c).is_empty()
            && c.receiver().is_some_and(|r| is_top_const(&r, "Rails"))
    })
}

impl Cop for RootPathnameMethods {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = helpers::rails::send(node) else { return };
        let method = method_name(&call);
        // `open` whose parent is a send (not a block around it) is skipped.
        if method == "open" {
            let anc = cx.ancestors;
            if helpers::rails::parent_index(anc).is_some_and(|p| helpers::rails::send(&anc[p]).is_some()) {
                return;
            }
        }
        let Some(recv) = call.receiver() else { return };
        let dir = is_top_const(&recv, "Dir");
        let glob = dir && DIR_GLOB_METHODS.contains(&method);
        let known = (dir && (DIR_NON_GLOB_METHODS.contains(&method) || glob && cx.config.target_ruby >= 2.5))
            || ((is_top_const(&recv, "IO") || is_top_const(&recv, "File")) && FILE_METHODS.contains(&method))
            || (is_top_const(&recv, "FileTest") && FILE_TEST_METHODS.contains(&method))
            || (is_top_const(&recv, "FileUtils") && FILE_UTILS_METHODS.contains(&method));
        if !known {
            return;
        }
        let args = send_args(&call);
        let Some((path, rest)) = args.split_first() else { return };
        // rails_root_pathname?: Rails.root itself or `Rails.root.join(...)`.
        let join = helpers::rails::send_node(path).filter(|j| method_name(j) == "join" && j.receiver().is_some_and(|r| rails_root(&r)));
        let root_src = if rails_root(path) {
            node_text(path).to_string()
        } else if let Some(j) = &join {
            node_text(&j.receiver().unwrap()).to_string()
        } else {
            return;
        };
        let mut safe = true;
        let replacement = if glob {
            // build_path_glob_replacement: `path.children.first.loc.selector` raises for a bare
            // Rails.root (a const has no selector), so RuboCop reports nothing.
            let Some(j) = &join else { return };
            let root = j.receiver().unwrap();
            let sel_end = root.as_call_node().and_then(|c| c.message_loc()).map_or(0, |l| l.end_offset());
            let receiver = cx.src.slice(path.location().start_offset(), sel_end).to_string();
            let jargs = send_args(j);
            let argument = if jargs.len() == 1 {
                node_text(&jargs[0]).to_string()
            } else {
                // join_arguments
                let mut interpolate = false;
                let parts: Vec<String> = jargs
                    .iter()
                    .map(|a| match helpers::rails::node_value(a) {
                        Some(v) => {
                            // Deliberate difference (docs/known-gaps.md): literal text goes into
                            // the new quotes only when inert there.
                            safe &= match a.as_interpolated_string_node() {
                                Some(d) => d.parts().iter().all(|p| p.as_embedded_statements_node().is_some() || helpers::rails::node_value(&p).is_some_and(|v| helpers::rails::inert_text(&v))),
                                None => helpers::rails::inert_text(&v),
                            };
                            v
                        }
                        None => {
                            interpolate = true;
                            format!("#{{{}}}", node_text(a))
                        }
                    })
                    .collect();
                let has_interp = jargs.iter().any(|a| {
                    a.as_interpolated_string_node().is_some_and(|d| d.parts().iter().any(|p| p.as_embedded_statements_node().is_some()))
                });
                let double = cx.config.for_cop("Style/StringLiterals").str("EnforcedStyle") == Some("double_quotes");
                let q = if double || has_interp || interpolate { '"' } else { '\'' };
                format!("{q}{}{q}", parts.join("/"))
            };
            format!("{receiver}.glob({argument})")
        } else {
            // build_path_replacement
            let mut p = node_text(path).to_string();
            if let Some(j) = &join
                && !send_args(j).is_empty()
                && j.opening_loc().is_none()
                && let Some(i) = p.find(' ')
            {
                p.replace_range(i..i + 1, "(");
                p.push(')');
            }
            let mut r = format!("{p}.{method}");
            if !rest.is_empty() {
                let formatted: Vec<String> = rest
                    .iter()
                    .map(|a| if a.as_array_node().is_some() { format!("*{}", node_text(a)) } else { node_text(a).to_string() })
                    .collect();
                r.push_str(&format!("({})", formatted.join(", ")));
            }
            r
        };
        let (s, e) = send_range(&call);
        let msg = format!("`{root_src}` is a `Pathname`, so you can use `{replacement}`.");
        if !safe {
            return cx.add_withheld_fix(s, e, msg);
        }
        cx.add_fix(s, e, msg, |c| c.replace(s, e, &replacement));
    }
}
