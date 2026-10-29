mod graph;
mod sequence;

use crate::{
    canvas::{Canvas, Emphasis, Role, TextLine, wrap},
    model::*,
};
use clap::ValueEnum;
use std::{error::Error, fmt};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum Border {
    #[default]
    Rounded,
    Square,
    Double,
    Heavy,
    Ascii,
}

impl Border {
    pub fn name(self) -> &'static str {
        match self {
            Self::Rounded => "rounded",
            Self::Square => "square",
            Self::Double => "double",
            Self::Heavy => "heavy",
            Self::Ascii => "ascii",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Rounded => Self::Square,
            Self::Square => Self::Double,
            Self::Double => Self::Heavy,
            Self::Heavy => Self::Ascii,
            Self::Ascii => Self::Rounded,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum Theme {
    Ocean,
    Ember,
    Forest,
    #[default]
    Mono,
}

impl Theme {
    pub fn name(self) -> &'static str {
        match self {
            Self::Ocean => "ocean",
            Self::Ember => "ember",
            Self::Forest => "forest",
            Self::Mono => "mono",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Ocean => Self::Ember,
            Self::Ember => Self::Forest,
            Self::Forest => Self::Mono,
            Self::Mono => Self::Ocean,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderOptions {
    pub border: Border,
    pub theme: Theme,
    pub direction: Option<Direction>,
    pub gap: usize,
    pub node_width: usize,
    pub padding: usize,
    pub edge_labels: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            border: Border::Rounded,
            theme: Theme::Mono,
            direction: None,
            gap: 2,
            node_width: 26,
            padding: 2,
            edge_labels: true,
        }
    }
}

#[derive(Debug)]
pub struct RenderError(pub String);

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl Error for RenderError {}

pub(super) fn checked_canvas(
    width: usize,
    height: usize,
    border: Border,
) -> Result<Canvas, RenderError> {
    if width > 4096 || height > 4096 || width.saturating_mul(height) > 1_000_000 {
        return Err(RenderError("diagram is too large for a terminal canvas (4096 columns/rows, 1 million cells); reduce spacing or split the diagram".into()));
    }
    Ok(Canvas::new(width.max(1), height.max(1), border))
}

/// Render deterministically, without terminal IO or ANSI escape sequences.
pub fn render(diagram: &Diagram, options: &RenderOptions) -> Result<Canvas, RenderError> {
    if !(2..=12).contains(&options.gap)
        || !(8..=80).contains(&options.node_width)
        || options.padding > 3
    {
        return Err(RenderError(
            "options require gap 2..12, node width 8..80, and padding 0..3".into(),
        ));
    }
    let canvas = match &diagram.kind {
        DiagramKind::Graph(graph) => graph::render(graph, options)?,
        DiagramKind::Sequence(sequence) => sequence::render(sequence, options)?,
    }
    .trimmed();
    if let Some(title) = &diagram.title {
        let lines = wrap(title, canvas.width().max(20), Emphasis::BOLD);
        let width = canvas
            .width()
            .max(lines.iter().map(TextLine::width).max().unwrap_or(0));
        let mut titled = checked_canvas(width, canvas.height() + lines.len() + 1, options.border)?;
        for (i, line) in lines.iter().enumerate() {
            titled.text_line(0, i, line, Role::Accent);
        }
        titled.blit(0, lines.len() + 1, &canvas);
        Ok(titled)
    } else {
        Ok(canvas)
    }
}

#[derive(Clone, Debug)]
pub(super) struct BoxNode {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
    pub lines: Vec<TextLine>,
    pub shape: Shape,
}

impl BoxNode {
    pub fn right(&self) -> usize {
        self.x + self.width - 1
    }
    pub fn bottom(&self) -> usize {
        self.y + self.height - 1
    }
    pub fn cx(&self) -> usize {
        self.x + self.width / 2
    }
    pub fn cy(&self) -> usize {
        self.y + self.height / 2
    }
}

fn label_width(line: &TextLine) -> usize {
    line.width() + 4
}

fn draw_label(canvas: &mut Canvas, x: usize, y: usize, line: &TextLine) {
    let (left, right) = if canvas.border == Border::Ascii {
        ("[ ", " ]")
    } else {
        ("🭮█", "█🭬")
    };
    canvas.text(x, y, left, Role::LabelDecoration, Emphasis::default());
    canvas.text_line(x + 2, y, line, Role::Label);
    canvas.text(
        x + 2 + line.width(),
        y,
        right,
        Role::LabelDecoration,
        Emphasis::default(),
    );
}

pub(super) fn box_node(label: &str, shape: Shape, options: &RenderOptions) -> BoxNode {
    let lines = wrap(label, options.node_width, Emphasis::BOLD);
    let width = lines.iter().map(TextLine::width).max().unwrap_or(0) + 2 + 2 * options.padding;
    let extra = match shape {
        Shape::Subroutine => 2,
        Shape::Circle | Shape::DoubleCircle | Shape::Decision | Shape::Hexagon => 4,
        _ => 0,
    };
    let height = lines.len() + if shape == Shape::Cylinder { 3 } else { 2 };
    let (width, height) = if matches!(shape, Shape::Start | Shape::End) {
        (1, 1)
    } else {
        ((width + extra).max(7), height)
    };
    BoxNode {
        x: 0,
        y: 0,
        width,
        height,
        lines,
        shape,
    }
}

pub(super) fn draw_node(
    canvas: &mut Canvas,
    node: &BoxNode,
    index: usize,
    options: &RenderOptions,
) {
    let role = Role::Node(index);
    let ascii = options.border == Border::Ascii;
    if matches!(node.shape, Shape::Start | Shape::End) {
        let glyph = match (node.shape, ascii) {
            (Shape::Start, false) => "●",
            (Shape::End, false) => "◉",
            (Shape::Start, true) => "*",
            _ => "@",
        };
        canvas.put(node.x, node.y, glyph, role, Emphasis::BOLD);
        return;
    }
    canvas.rect(
        node.x,
        node.y,
        node.width,
        node.height,
        role,
        options.border,
    );
    let mut label_y = node.y + (node.height - node.lines.len()) / 2;
    match node.shape {
        Shape::Cylinder => {
            canvas.line(
                (node.x, node.y + 1),
                (node.right(), node.y + 1),
                role,
                Stroke::Solid,
            );
            label_y += 1;
        }
        Shape::Subroutine => {
            canvas.line(
                (node.x + 2, node.y),
                (node.x + 2, node.bottom()),
                role,
                Stroke::Solid,
            );
            canvas.line(
                (node.right() - 2, node.y),
                (node.right() - 2, node.bottom()),
                role,
                Stroke::Solid,
            );
        }
        Shape::Decision | Shape::Hexagon => {
            let corners = if ascii {
                ["/", "\\", "\\", "/"]
            } else {
                ["╱", "╲", "╲", "╱"]
            };
            for ((x, y), glyph) in [
                (node.x, node.y),
                (node.right(), node.y),
                (node.x, node.bottom()),
                (node.right(), node.bottom()),
            ]
            .into_iter()
            .zip(corners)
            {
                canvas.put(x, y, glyph, role, Emphasis::default());
            }
            if node.shape == Shape::Decision {
                canvas.put(node.x, node.cy(), "<", role, Emphasis::BOLD);
                canvas.put(node.right(), node.cy(), ">", role, Emphasis::BOLD);
            }
        }
        Shape::Circle | Shape::DoubleCircle => {
            canvas.put(node.x, node.cy(), "(", role, Emphasis::default());
            canvas.put(node.right(), node.cy(), ")", role, Emphasis::default());
            if node.shape == Shape::DoubleCircle {
                canvas.put(node.x + 1, node.cy(), "(", role, Emphasis::default());
                canvas.put(node.right() - 1, node.cy(), ")", role, Emphasis::default());
            }
        }
        _ => {}
    }
    for (row, line) in node.lines.iter().enumerate() {
        canvas.text_line(
            node.x + (node.width - line.width()) / 2,
            label_y + row,
            line,
            role,
        );
    }
}

pub(super) fn arrow(
    canvas: &mut Canvas,
    point: (usize, usize),
    direction: (isize, isize),
    marker: Arrow,
    options: &RenderOptions,
) {
    let ascii = options.border == Border::Ascii;
    let glyph = match marker {
        Arrow::None => return,
        Arrow::Circle => {
            if ascii {
                "o"
            } else {
                "○"
            }
        }
        Arrow::Cross => {
            if ascii {
                "x"
            } else {
                "×"
            }
        }
        Arrow::Point => match direction {
            (1, 0) => {
                if ascii {
                    ">"
                } else {
                    "╼"
                }
            }
            (-1, 0) => {
                if ascii {
                    "<"
                } else {
                    "╾"
                }
            }
            (0, -1) => {
                if ascii {
                    "^"
                } else {
                    "╿"
                }
            }
            _ => {
                if ascii {
                    "v"
                } else {
                    "╽"
                }
            }
        },
    };
    canvas.put(point.0, point.1, glyph, Role::Edge, Emphasis::default());
}
