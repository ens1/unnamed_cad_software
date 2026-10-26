//! Composition metadata for the bundled geometry implementation.
//!
//! The desktop and worker compile the same identity without linking the
//! geometry kernel into the desktop. Contracts still carry an opaque backend
//! identity; changing implementations means selecting another profile here.
//! Bump the adapter or solver identity when its evaluation semantics change.
#![no_std]

macro_rules! native_solver_identity {
    () => {
        "tcad-native-lm/18"
    };
}

pub const NATIVE_SOLVER: &str = native_solver_identity!();
pub const BUNDLED_BACKEND: &str = concat!(
    "monstertruck/0.4.1+6dba41312ccc;adapter/47;solver/",
    native_solver_identity!()
);
