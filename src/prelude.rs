//! Everything a cop file usually needs: `use crate::prelude::*;`.

pub use crate::ast::*;
pub use crate::config::value::{Map, Value};
pub use crate::cop::{Cop, Cx};
pub use crate::node_kind as kind;
pub use regex::Regex;
pub use ruby_prism::Node;
pub use std::sync::LazyLock;
