//! Shared implementation for the `learnc` compiler and `learn` runtime.
//!
//! The binaries deliberately expose separate trust boundaries: source and
//! repository access belongs to [`compiler`] and [`repository`], while
//! artifact loading and learner state belong to [`runtime`]. The serialized
//! contract between them belongs to [`artifact`].

pub mod artifact;
pub mod cli;
pub mod compiler;
pub mod diagnostics;
pub mod language;
pub mod repository;
pub mod runtime;
pub mod source;
