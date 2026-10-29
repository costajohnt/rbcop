//! RuboCop::LSP.enabled?, which `--editor-mode` turns on for the run: `AutoCorrect: contextual`
//! cops do not correct, Lint/Syntax drops its parser-version line, and the Metrics length and
//! complexity cops report only a definition's head.

use crate::prelude::*;
use std::sync::atomic::{AtomicBool, Ordering};

static ENABLED: AtomicBool = AtomicBool::new(false);
/// `--lsp` is serving: offenses keep their own corrections (the quick fixes it publishes).
static SERVING: AtomicBool = AtomicBool::new(false);

pub fn enable() {
    ENABLED.store(true, Ordering::Relaxed);
}

/// RuboCop::LSP.disable { ... }: `workspace/executeCommand` formats as a command-line run would.
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

/// LSP::Runtime.new: `--lsp` serves, with LSP enabled.
pub fn start_serving() {
    SERVING.store(true, Ordering::Relaxed);
    enable();
}

pub fn serving() -> bool {
    SERVING.load(Ordering::Relaxed)
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// The `editing` option (RuboCop 1.92.2, set by MCP): holds back `AutoCorrect: contextual`
/// without the rest of LSP mode.
pub static EDITING: AtomicBool = AtomicBool::new(false);

/// AutocorrectLogic#contextual_autocorrect_held_back? without the `contextual` check:
/// `LSP.enabled? || @options.fetch(:editing, false)`.
pub fn holds_contextual() -> bool {
    enabled() || EDITING.load(Ordering::Relaxed)
}

/// CodeLength / MethodComplexity `#location` under LSP: from the node's start to the end of its
/// name (`def foo`, `class Foo`, the `self` of `class << self`) or of its block opener (`foo do`, `-> {`); other nodes (and
/// every node outside editor mode) keep `range`.
pub fn location(node: &Node, range: (usize, usize)) -> (usize, usize) {
    if !enabled() {
        return range;
    }
    let end = if let Some(def) = node.as_def_node() {
        Some(def.name_loc().end_offset())
    } else if let Some(class) = node.as_class_node() {
        Some(class.constant_path().location().end_offset())
    } else if let Some(module) = node.as_module_node() {
        Some(module.constant_path().location().end_offset())
    } else if let Some(sclass) = node.as_singleton_class_node() {
        Some(sclass.expression().location().end_offset())
    } else if let Some(lambda) = node.as_lambda_node() {
        Some(lambda.opening_loc().end_offset())
    } else if let Some(call) = node.as_call_node() {
        call.block().and_then(|b| b.as_block_node()).map(|b| b.opening_loc().end_offset())
    } else {
        None
    };
    end.map_or(range, |e| (range.0, e))
}
