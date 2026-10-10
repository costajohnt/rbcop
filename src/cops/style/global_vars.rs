use crate::prelude::*;

pub const NAME: &str = "Style/GlobalVars";
pub static COP: GlobalVars = GlobalVars;
pub struct GlobalVars;

const BUILT_IN_VARS: &[&str] = &[
    "$:", "$LOAD_PATH", "$\"", "$LOADED_FEATURES", "$0", "$PROGRAM_NAME", "$!", "$ERROR_INFO", "$@",
    "$ERROR_POSITION", "$;", "$FS", "$FIELD_SEPARATOR", "$,", "$OFS", "$OUTPUT_FIELD_SEPARATOR", "$/", "$RS",
    "$INPUT_RECORD_SEPARATOR", "$\\", "$ORS", "$OUTPUT_RECORD_SEPARATOR", "$.", "$NR", "$INPUT_LINE_NUMBER", "$_",
    "$LAST_READ_LINE", "$>", "$DEFAULT_OUTPUT", "$<", "$DEFAULT_INPUT", "$$", "$PID", "$PROCESS_ID", "$?",
    "$CHILD_STATUS", "$~", "$LAST_MATCH_INFO", "$=", "$IGNORECASE", "$*", "$ARGV", "$&", "$MATCH", "$`",
    "$PREMATCH", "$'", "$POSTMATCH", "$+", "$LAST_PAREN_MATCH", "$stdin", "$stdout", "$stderr", "$DEBUG",
    "$FILENAME", "$VERBOSE", "$SAFE", "$-0", "$-a", "$-d", "$-F", "$-i", "$-I", "$-l", "$-p", "$-v", "$-w",
    "$CLASSPATH", "$JRUBY_VERSION", "$JRUBY_REVISION", "$ENV_JAVA",
];

impl Cop for GlobalVars {
    fn kinds(&self) -> &'static [usize] {
        &[
            kind::GLOBAL_VARIABLE_READ_NODE,
            kind::GLOBAL_VARIABLE_WRITE_NODE,
            kind::GLOBAL_VARIABLE_OPERATOR_WRITE_NODE,
            kind::GLOBAL_VARIABLE_OR_WRITE_NODE,
            kind::GLOBAL_VARIABLE_AND_WRITE_NODE,
            kind::GLOBAL_VARIABLE_TARGET_NODE,
            kind::STRING_NODE,
        ]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if node.as_string_node().is_some() {
            // `#$-1` below 3.3 (see legacy_gvar_interpolations).
            for (start, end) in helpers::string_help::legacy_gvar_interpolations(cx, node) {
                let name = cx.src.slice(start, end);
                if !BUILT_IN_VARS.contains(&name) && !cx.cfg.strings("AllowedVariables").iter().any(|v| v == name) {
                    cx.add(start, end, "Do not introduce global variables.");
                }
            }
            return;
        }
        // gvar / gvasgn (also inside op_asgn, or_asgn, and_asgn, masgn): `loc.name`.
        let loc = match node {
            Node::GlobalVariableWriteNode { .. } => node.as_global_variable_write_node().unwrap().name_loc(),
            Node::GlobalVariableOperatorWriteNode { .. } => {
                node.as_global_variable_operator_write_node().unwrap().name_loc()
            }
            Node::GlobalVariableOrWriteNode { .. } => node.as_global_variable_or_write_node().unwrap().name_loc(),
            Node::GlobalVariableAndWriteNode { .. } => node.as_global_variable_and_write_node().unwrap().name_loc(),
            _ => node.location(),
        };
        let name = text(&loc);
        if BUILT_IN_VARS.contains(&name) || cx.cfg.strings("AllowedVariables").iter().any(|v| v == name) {
            return;
        }
        cx.add(loc.start_offset(), loc.end_offset(), "Do not introduce global variables.");
    }
}
