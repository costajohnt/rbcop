//! Cop implementations, one file per cop under `src/cops/<department>/`, registered by build.rs.

include!(concat!(env!("OUT_DIR"), "/cops.rs"));
