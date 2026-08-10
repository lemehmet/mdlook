//! A terminal markdown reader built around two properties the existing tools do
//! not offer together: paragraphs that reflow to *your* terminal width rather
//! than the author's editor width, and a search index you can navigate.
//!
//! The pipeline is three stages, each a pure function of the previous one:
//!
//! ```text
//! source ──parse──▶ Document ──layout(width, theme)──▶ RenderedDoc ──▶ screen
//!                   (semantic,                          (styled lines +
//!                    width-free)                         plain mirror + index)
//! ```
//!
//! Keeping `layout` pure is what makes the output reproducible, and building the
//! styled lines and their plain-text mirror in the same pass is what makes
//! "scroll to the match" exact rather than approximate.

pub mod doc;
pub mod layout;
pub mod render;
pub mod ui;

pub use doc::parse;
pub use layout::{layout, RenderedDoc, Theme, ThemeKind};
