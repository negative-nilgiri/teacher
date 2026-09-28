//! Shared implementation for the `learnc` compiler, `learn` runtime, and
//! optional `learnverify` semantic checker.
//!
//! The binaries deliberately expose separate trust boundaries: source and
//! repository access belongs to [`compiler`] and [`repository`], while
//! artifact loading and learner state belong to [`runtime`]. The serialized
//! contract between them belongs to [`artifact`]. The checker remains a leaf:
//! none of those core modules, nor [`lint`], depends on [`learnverify`].

pub mod artifact;
pub mod cli;
pub mod compiler;
pub mod diagnostics;
pub mod language;
pub mod learnverify;
pub mod lint;
pub mod repository;
pub mod runtime;
pub mod source;
