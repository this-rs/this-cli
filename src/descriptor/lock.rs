//! `descriptor/descriptor.lock`: the types, the check and the tag assignment live in
//! `this_descriptor::lock` (the server applies the very same lock when it composes a compiled
//! module: one source, no second copy). This module re-exports them for the CLI.

pub use this_descriptor::lock::*;
