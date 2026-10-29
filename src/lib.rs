//! A terminal-native renderer for a practical subset of Mermaid.
//!
//! Parse once with [`parse`], then render with different [`RenderOptions`] to
//! explore layouts without reparsing the source. Rendering never writes to a
//! terminal; ANSI output is an explicit choice when serializing the canvas.

pub mod canvas;
pub mod model;
pub mod parser;
pub mod render;

pub use model::{Diagram, Direction};
pub use parser::{ParseError, parse};
pub use render::{Border, RenderError, RenderOptions, Theme, render};
