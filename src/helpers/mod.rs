//! Shared helpers for cops (ports of RuboCop mixins and utilities), one file per topic.
//! Files in this directory are registered automatically by build.rs: add a file, don't edit this one.

include!(concat!(env!("OUT_DIR"), "/helpers.rs"));
