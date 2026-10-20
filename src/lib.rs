//! bendenv: a project and environment manager for Bend 2.
//!
//! A manifest, a lockfile and a pinned package environment, over
//! BendHub. The environment is the shape the loader already reads,
//! ~/.bend/lib, so pointing BEND_LIB at it is the whole of the
//! integration.

pub mod cache;
pub mod env;
pub mod hub;
pub mod lock;
pub mod manifest;
pub mod publish;
pub mod resolve;
pub mod scan;
pub mod version;
