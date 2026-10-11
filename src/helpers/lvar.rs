//! Parser-shaped ranges for local variable reads.

use ruby_prism::Location;

/// parser's `lvar` range for a Prism local variable read. For a hash-shorthand value (`foo(x:)`,
/// `{ x: }`) Prism's `LocalVariableReadNode` / `ItLocalVariableReadNode` spans the label colon
/// too; parser's `lvar` is the bare name.
pub fn range(loc: &Location, name: &[u8]) -> (usize, usize) {
    let start = loc.start_offset();
    (start, start + name.len())
}
