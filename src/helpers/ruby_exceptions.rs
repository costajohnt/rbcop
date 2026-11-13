//! Lint/ShadowedException's table of exception classes. Errno codes and aliases depend on the
//! platform (Errno::EOPNOTSUPP is Errno::ENOTSUP on Linux, a class of its own on macOS), so RuboCop's
//! answer does too: each platform gets the table scripts/dump-ruby-exceptions.rb writes there.
//! Other targets use the Linux table.
//! darwin.rs is the 1.91.0 arm64-darwin25 output minus the Net:: rows 1.92.2 no longer loads (the
//! only 1.91.0 to 1.92.2 change on Linux); regenerate it on a Mac.

#[cfg(target_os = "macos")]
#[path = "ruby_exceptions/darwin.rs"]
mod table;
#[cfg(not(target_os = "macos"))]
#[path = "ruby_exceptions/linux.rs"]
mod table;

pub use table::EXCEPTIONS;
