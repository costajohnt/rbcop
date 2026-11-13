//! Lint/ShadowedException's table of exception classes. Errno codes and aliases depend on the
//! platform (Errno::EOPNOTSUPP is Errno::ENOTSUP on Linux, a class of its own on macOS), so RuboCop's
//! answer does too: each platform gets the table scripts/dump-ruby-exceptions.rb writes there.
//! Other targets use the Linux table.
//! darwin.rs was regenerated with RuboCop 1.92.2 on Ruby 4.0.6 arm64-darwin25 (2026-11-13, the
//! ci.yml gem set) and matched byte for byte.

#[cfg(target_os = "macos")]
#[path = "ruby_exceptions/darwin.rs"]
mod table;
#[cfg(not(target_os = "macos"))]
#[path = "ruby_exceptions/linux.rs"]
mod table;

pub use table::EXCEPTIONS;
