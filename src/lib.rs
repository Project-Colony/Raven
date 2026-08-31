//! Raven runs Windows programs against a real Windows installation mounted as C:.
//!
//! Everything Raven can do lives here as a library. The command-line interface in
//! `main.rs` is a shell over this API and holds no logic of its own, so that a
//! graphical front end is a second caller rather than a rewrite.

pub mod mount;
