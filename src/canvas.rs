//! A cell canvas shared by the renderer, ANSI exporter, and live viewport.
//! Grapheme clusters occupy terminal columns, not UTF-8 bytes or scalar values.

use std::{
    cmp::Reverse,
    collections::{BinaryHeap, HashSet},
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::{
    model::Stroke,
    render::{Border, Theme},
};

pub const UP: u8 = 1;
pub const RIGHT: u8 = 2;
pub const DOWN: u8 = 4;
pub const LEFT: u8 = 8;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Role {
    #[default]
    Text,
    Node(usize),
    Edge,
    Label,
    LabelDecoration,
    Muted,
    Accent,
    Error,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Emphasis(pub u8);

impl Emphasis {
    pub const BOLD: Self = Self(1);
    pub const ITALIC: Self = Self(2);
    pub const UNDERLINE: Self = Self(4);
}

#[derive(Clone, Debug, Default)]
struct Cell {
    glyph: String,
    connections: u8,
    heavy_connections: u8,
    continuation: bool,
    role: Role,
    emphasis: Emphasis,
    stroke: Stroke,
    border: Border,
}

#[derive(Clone, Debug)]
pub struct Canvas {
    width: usize,
    height: usize,
    cells: Vec<Cell>,
    pub border: Border,
}

#[derive(Clone, Debug)]
pub struct Glyph {
    pub text: String,
    pub width: usize,
    pub emphasis: Emphasis,
}

#[derive(Clone, Debug, Default)]
pub struct TextLine(pub Vec<Glyph>);

impl TextLine {
    pub fn width(&self) -> usize {
        self.0.iter().map(|g| g.width).sum()
    }
    pub fn text(&self) -> String {
        self.0.iter().map(|g| g.text.as_str()).collect()
    }
}

impl Canvas {
    pub fn new(width: usize, height: usize, border: Border) -> Self {
        Self {
            width,
            height,
            cells: vec![Cell::default(); width * height],
            border,
        }
    }

    pub fn width(&self) -> usize {
        self.width
    }
    pub fn height(&self) -> usize {
        self.height
    }

    pub fn blit(&mut self, x: usize, y: usize, other: &Self) {
        for row in 0..other.height.min(self.height.saturating_sub(y)) {
            let width = other.width.min(self.width.saturating_sub(x));
            let from = row * other.width;
            let to = (y + row) * self.width + x;
            self.cells[to..to + width].clone_from_slice(&other.cells[from..from + width]);
        }
    }

    pub fn viewport(
        &mut self,
        origin: (usize, usize),
        other: &Self,
        offset: (usize, usize),
        size: (usize, usize),
    ) {
        for row in 0..size.1 {
            for col in 0..size.0 {
                let (sx, sy) = (offset.0 + col, offset.1 + row);
                let (x, y) = (origin.0 + col, origin.1 + row);
                if x >= self.width || y >= self.height || sx >= other.width || sy >= other.height {
                    continue;
                }
                let cell = &other.cells[sy * other.width + sx];
                self.cells[y * self.width + x] =
                    if (col == 0 && cell.continuation) || col + cell.glyph.width() > size.0 {
                        Cell::default()
                    } else {
                        cell.clone()
                    };
            }
        }
    }

    pub fn put(&mut self, x: usize, y: usize, glyph: &str, role: Role, emphasis: Emphasis) {
        let width = glyph.width();
        if x >= self.width || y >= self.height || width == 0 || x + width > self.width {
            return;
        }
        for offset in 0..width {
            self.cells[y * self.width + x + offset] = Cell {
                glyph: if offset == 0 {
                    glyph.to_owned()
                } else {
                    String::new()
                },
                continuation: offset != 0,
                role,
                emphasis,
                ..Cell::default()
            };
        }
    }

    pub fn text(&mut self, x: usize, y: usize, text: &str, role: Role, emphasis: Emphasis) {
        let mut x = x;
        for glyph in text.graphemes(true) {
            if glyph.chars().any(char::is_control) {
                continue;
            }
            self.put(x, y, glyph, role, emphasis);
            x += glyph.width();
        }
    }

    pub fn text_line(&mut self, x: usize, y: usize, line: &TextLine, role: Role) {
        let mut x = x;
        for glyph in &line.0 {
            self.put(x, y, &glyph.text, role, glyph.emphasis);
            x += glyph.width;
        }
    }

    pub fn line(&mut self, from: (usize, usize), to: (usize, usize), role: Role, stroke: Stroke) {
        self.line_with_border(from, to, role, stroke, self.border);
    }

    pub fn line_with_border(
        &mut self,
        from: (usize, usize),
        to: (usize, usize),
        role: Role,
        stroke: Stroke,
        border: Border,
    ) {
        let (x1, y1) = from;
        let (x2, y2) = to;
        if x1 != x2 && y1 != y2 {
            return;
        }
        for y in y1.min(y2)..=y1.max(y2) {
            for x in x1.min(x2)..=x1.max(x2) {
                if x >= self.width || y >= self.height {
                    continue;
                }
                let mut mask = 0;
                if x > x1.min(x2) {
                    mask |= LEFT;
                }
                if x < x1.max(x2) {
                    mask |= RIGHT;
                }
                if y > y1.min(y2) {
                    mask |= UP;
                }
                if y < y1.max(y2) {
                    mask |= DOWN;
                }
                let cell = &mut self.cells[y * self.width + x];
                cell.connections |= mask;
                cell.glyph.clear();
                cell.continuation = false;
                cell.role = role;
                cell.stroke = stroke;
                cell.border = border;
            }
        }
    }

    pub(crate) fn attach(&mut self, at: (usize, usize), outside: (usize, usize), heavy: bool) {
        if at.0 >= self.width || at.1 >= self.height {
            return;
        }
        let arm = match (
            outside.0 as isize - at.0 as isize,
            outside.1 as isize - at.1 as isize,
        ) {
            (1, 0) => RIGHT,
            (-1, 0) => LEFT,
            (0, 1) => DOWN,
            (0, -1) => UP,
            _ => return,
        };
        let cell = &mut self.cells[at.1 * self.width + at.0];
        // Shape glyphs such as decision brackets and state dots stay intact.
        if cell.connections == 0 {
            return;
        }
        cell.connections |= arm;
        if heavy {
            cell.heavy_connections |= arm;
        }
    }

    pub fn rect(
        &mut self,
        x: usize,
        y: usize,
        width: usize,
        height: usize,
        role: Role,
        border: Border,
    ) {
        if width < 2 || height < 2 {
            return;
        }
        self.line_with_border((x, y), (x + width - 1, y), role, Stroke::Solid, border);
        self.line_with_border(
            (x, y + height - 1),
            (x + width - 1, y + height - 1),
            role,
            Stroke::Solid,
            border,
        );
        self.line_with_border((x, y), (x, y + height - 1), role, Stroke::Solid, border);
        self.line_with_border(
            (x + width - 1, y),
            (x + width - 1, y + height - 1),
            role,
            Stroke::Solid,
            border,
        );
    }

    pub fn blank_rect(&mut self, x: usize, y: usize, width: usize, height: usize) {
        for row in y..(y + height).min(self.height) {
            for col in x..(x + width).min(self.width) {
                self.cells[row * self.width + col] = Cell::default();
            }
        }
    }

    pub fn is_blank(&self, x: usize, y: usize) -> bool {
        if x >= self.width || y >= self.height {
            return true;
        }
        self.cells
            .get(y * self.width + x)
            .is_none_or(|c| c.connections == 0 && c.glyph.trim().is_empty() && !c.continuation)
    }

    pub fn area_blank(&self, x: usize, y: usize, width: usize, height: usize) -> bool {
        x + width <= self.width
            && y + height <= self.height
            && (y..y + height).all(|row| (x..x + width).all(|col| self.is_blank(col, row)))
    }

    // Each interval bounds how many rows can be removed between consecutive
    // boxes or layers. Repeated column constraints share the same interval.
    fn vertical_gap_constraints(&self, gap: usize) -> Vec<Vec<(usize, usize)>> {
        let mut starts = vec![Vec::new(); self.height];
        if gap == 0 {
            return starts;
        }
        let mut above: Vec<Option<(usize, usize)>> = vec![None; self.width];
        let mut intervals = HashSet::new();
        let mut previous_node_row: Option<usize> = None;
        for (y, row) in self.cells.chunks(self.width).enumerate() {
            // Consecutive layers may have no boxes in a common column.
            if row.iter().any(|cell| matches!(cell.role, Role::Node(_))) {
                if let Some(upper) = previous_node_row
                    && y > upper + 1
                    && intervals.insert((upper, y))
                {
                    starts[upper + 1].push((y, (y - upper - 1).saturating_sub(gap)));
                }
                previous_node_row = Some(y);
            }
            for (x, cell) in row.iter().enumerate() {
                if let Role::Node(node) = cell.role {
                    if let Some((previous, upper)) = above[x]
                        && previous != node
                        && intervals.insert((upper, y))
                    {
                        starts[upper + 1].push((y, (y - upper - 1).saturating_sub(gap)));
                    }
                    above[x] = Some((node, y));
                }
            }
        }
        starts
    }

    /// Close unused vertical space after graph routing, preserving the minimum
    /// gap between node borders. Disjoint horizontal runs can share a row when
    /// their connecting vertical stems stay separate.
    pub(crate) fn compact_vertical(mut self, gap: usize) -> Self {
        let starts = self.vertical_gap_constraints(gap);
        let mut limits = BinaryHeap::new();
        let mut removed = 0;
        let mut packed: Vec<Cell> = Vec::with_capacity(self.cells.len());
        let mut previous_edges = false;
        for (y, row) in self.cells.chunks(self.width).enumerate() {
            for &(end, slack) in &starts[y] {
                limits.push(Reverse((removed + slack, end)));
            }
            while limits.peek().is_some_and(|&Reverse((_, end))| end <= y) {
                limits.pop();
            }
            let can_remove = limits
                .peek()
                .is_none_or(|&Reverse((limit, _))| removed < limit);
            let edges = row.iter().all(|cell| {
                cell.glyph.trim().is_empty()
                    && !cell.continuation
                    && (cell.connections == 0 || cell.role == Role::Edge)
            });
            if can_remove
                && edges
                && row
                    .iter()
                    .all(|cell| cell.connections & (LEFT | RIGHT) == 0)
            {
                removed += 1;
                continue;
            }
            if can_remove && edges && previous_edges {
                let start = packed.len() - self.width;
                let previous = &mut packed[start..];
                let compatible = previous.iter().zip(row).all(|(upper, lower)| {
                    if upper.connections == 0 || lower.connections == 0 {
                        return true;
                    }
                    // Contract only an existing vertical connection. Otherwise
                    // merging the cells would join separate edges or add a crossing.
                    upper.connections & DOWN != 0
                        && lower.connections & UP != 0
                        && (upper.connections & (LEFT | RIGHT) == 0
                            || lower.connections & (LEFT | RIGHT) == 0)
                        && upper.stroke == lower.stroke
                        && upper.border == lower.border
                });
                if compatible {
                    for (upper, lower) in previous.iter_mut().zip(row) {
                        let connections = (upper.connections & !DOWN) | (lower.connections & !UP);
                        let heavy =
                            (upper.heavy_connections & !DOWN) | (lower.heavy_connections & !UP);
                        if upper.connections & (LEFT | RIGHT) == 0 {
                            upper.clone_from(lower);
                        }
                        upper.connections = connections;
                        upper.heavy_connections = heavy;
                    }
                    removed += 1;
                    continue;
                }
            }
            packed.extend_from_slice(row);
            previous_edges = edges;
        }
        self.cells = packed;
        self.height = self.cells.len() / self.width;
        if self.height == 0 {
            return Self::new(self.width, 1, self.border);
        }
        self
    }

    /// Trim unused outer columns and rows, retaining all grapheme continuations.
    pub fn trimmed(self) -> Self {
        let mut min_x = self.width;
        let mut min_y = self.height;
        let mut max_x = 0;
        let mut max_y = 0;
        for y in 0..self.height {
            for x in 0..self.width {
                if !self.is_blank(x, y) {
                    min_x = min_x.min(x);
                    min_y = min_y.min(y);
                    max_x = max_x.max(x);
                    max_y = max_y.max(y);
                }
            }
        }
        if min_x == self.width {
            return Self::new(1, 1, self.border);
        }
        let mut result = Self::new(max_x - min_x + 1, max_y - min_y + 1, self.border);
        for y in min_y..=max_y {
            let from = y * self.width + min_x;
            let to = (y - min_y) * result.width;
            result.cells[to..to + result.width]
                .clone_from_slice(&self.cells[from..from + result.width]);
        }
        result
    }

    /// Serialize a viewport without cutting an ANSI sequence or wide grapheme.
    pub fn row(
        &self,
        y: usize,
        x: usize,
        width: usize,
        color: bool,
        accents: bool,
        theme: Theme,
    ) -> String {
        if y >= self.height {
            return String::new();
        }
        let end = (x + width).min(self.width);
        let mut out = String::new();
        let mut previous = String::new();
        let mut col = x;
        while col < end {
            let cell = &self.cells[y * self.width + col];
            let glyph = if cell.connections != 0 {
                connection_glyph(cell).to_string()
            } else if cell.continuation || cell.glyph.is_empty() {
                " ".to_owned()
            } else {
                cell.glyph.clone()
            };
            let glyph_width = glyph.width().max(1);
            let clipped = col + glyph_width > end;
            let style = ansi_style(cell.role, cell.emphasis, color, accents, theme);
            if style != previous {
                out.push_str(if style.is_empty() { "\x1b[0m" } else { &style });
                previous = style;
            }
            if clipped {
                out.push_str(&" ".repeat(end - col));
            } else {
                out.push_str(&glyph);
            }
            col += glyph_width;
        }
        if !previous.is_empty() {
            out.push_str("\x1b[0m");
        }
        out
    }

    pub fn to_text(&self, color: bool, accents: bool, theme: Theme) -> String {
        let mut out = String::new();
        for y in 0..self.height {
            let end = (0..self.width)
                .rev()
                .find(|x| !self.is_blank(*x, y))
                .map_or(0, |x| x + 1);
            out.push_str(&self.row(y, 0, end, color, accents, theme));
            out.push('\n');
        }
        out
    }
}

pub fn ansi_style(
    role: Role,
    emphasis: Emphasis,
    color: bool,
    accents: bool,
    theme: Theme,
) -> String {
    let mut codes = Vec::new();
    if color {
        let palette = match theme {
            Theme::Ansi => [36, 32, 33, 34, 35, 31],
            Theme::Mono => [39; 6],
        };
        let number = match role {
            Role::Node(i) => palette[i % palette.len()],
            Role::Edge | Role::Label | Role::LabelDecoration | Role::Text => 39,
            Role::Muted => 90,
            Role::Accent => palette[0],
            Role::Error => 31,
        };
        codes.push(number.to_string());
        if role == Role::Label {
            codes.push("7".into());
        }
    }
    if accents {
        if emphasis.0 & 1 != 0 {
            codes.push("1".into());
        }
        if emphasis.0 & 2 != 0 {
            codes.push("3".into());
        }
        if emphasis.0 & 4 != 0 {
            codes.push("4".into());
        }
    }
    if codes.is_empty() {
        String::new()
    } else {
        format!("\x1b[0;{}m", codes.join(";"))
    }
}

fn connection_glyph(cell: &Cell) -> char {
    if matches!(cell.border, Border::Rounded | Border::Square) && cell.stroke != Stroke::Thick {
        match cell.connections {
            7 if cell.heavy_connections & RIGHT != 0 => return '┝',
            13 if cell.heavy_connections & LEFT != 0 => return '┥',
            14 if cell.heavy_connections & DOWN != 0 => return '┰',
            11 if cell.heavy_connections & UP != 0 => return '┸',
            _ => {}
        }
    }
    line_glyph(cell.connections, cell.border, cell.stroke)
}

pub fn line_glyph(mask: u8, border: Border, stroke: Stroke) -> char {
    let horizontal = mask & (LEFT | RIGHT) != 0;
    let vertical = mask & (UP | DOWN) != 0;
    if border == Border::Ascii {
        return match (horizontal, vertical) {
            (true, true) => '+',
            (true, false) => {
                if stroke == Stroke::Thick {
                    '='
                } else if stroke == Stroke::Dashed {
                    '.'
                } else {
                    '-'
                }
            }
            _ => {
                if stroke == Stroke::Dashed {
                    ':'
                } else {
                    '|'
                }
            }
        };
    }
    let index = mask as usize;
    if border == Border::Double {
        return [
            ' ', '║', '═', '╚', '║', '║', '╔', '╠', '═', '╝', '═', '╩', '╗', '╣', '╦', '╬',
        ][index];
    }
    if border == Border::Heavy || stroke == Stroke::Thick {
        return [
            ' ', '┃', '━', '┗', '┃', '┃', '┏', '┣', '━', '┛', '━', '┻', '┓', '┫', '┳', '╋',
        ][index];
    }
    if stroke == Stroke::Dashed && !(horizontal && vertical) {
        return if horizontal { '┄' } else { '┆' };
    }
    let corners = if border == Border::Rounded {
        ['╰', '╭', '╯', '╮']
    } else {
        ['└', '┌', '┘', '┐']
    };
    match mask {
        3 => corners[0],
        6 => corners[1],
        9 => corners[2],
        12 => corners[3],
        _ => [
            ' ', '│', '─', '└', '│', '│', '┌', '├', '─', '┘', '─', '┴', '┐', '┤', '┬', '┼',
        ][index],
    }
}

/// Wrap Markdown-style emphasis and HTML emphasis into styled grapheme lines.
/// The same glyphs and geometry are used with and without ANSI styling.
pub fn wrap(text: &str, width: usize, base: Emphasis) -> Vec<TextLine> {
    let width = width.max(2);
    let mut glyphs = Vec::new();
    let mut emphasis = Emphasis::default();
    let mut rest = text;
    while !rest.is_empty() {
        let tokens = [
            ("**", 1),
            ("__", 1),
            ("*", 2),
            ("`", 4),
            ("<b>", 1),
            ("</b>", 1),
            ("<strong>", 1),
            ("</strong>", 1),
            ("<i>", 2),
            ("</i>", 2),
            ("<em>", 2),
            ("</em>", 2),
            ("<u>", 4),
            ("</u>", 4),
        ];
        let token = tokens.iter().find(|(token, bit)| {
            if !rest.starts_with(token) {
                return false;
            }
            let closing = match *token {
                "<b>" => "</b>",
                "<strong>" => "</strong>",
                "<i>" => "</i>",
                "<em>" => "</em>",
                "<u>" => "</u>",
                _ => token,
            };
            emphasis.0 & bit != 0 || rest[token.len()..].contains(closing)
        });
        if let Some((token, bit)) = token {
            emphasis.0 ^= bit;
            rest = &rest[token.len()..];
            continue;
        }
        let g = rest.graphemes(true).next().unwrap();
        rest = &rest[g.len()..];
        if g.chars().any(|c| c.is_control() && c != '\n') {
            continue;
        }
        glyphs.push(Glyph {
            text: g.to_owned(),
            width: g.width(),
            emphasis: Emphasis(base.0 | emphasis.0),
        });
    }
    let mut lines = Vec::new();
    let mut current: Vec<Glyph> = Vec::new();
    let mut columns = 0;
    for glyph in glyphs {
        if glyph.text == "\n" {
            lines.push(TextLine(std::mem::take(&mut current)));
            columns = 0;
            continue;
        }
        if columns + glyph.width > width && !current.is_empty() {
            if let Some(space) = current.iter().rposition(|g| g.text == " ") {
                let tail = current.split_off(space + 1);
                current.pop();
                lines.push(TextLine(std::mem::replace(&mut current, tail)));
                columns = current.iter().map(|g| g.width).sum();
            }
            if columns + glyph.width > width {
                lines.push(TextLine(std::mem::take(&mut current)));
                columns = 0;
            }
        }
        if current.is_empty() && glyph.text == " " {
            continue;
        }
        columns += glyph.width;
        current.push(glyph);
    }
    lines.push(TextLine(current));
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vertical_compaction_preserves_gaps_between_staggered_nodes() {
        let mut canvas = Canvas::new(10, 16, Border::Rounded);
        for (i, (x, y)) in [(0, 0), (0, 7), (6, 4), (6, 12)].into_iter().enumerate() {
            canvas.rect(x, y, 3, 3, Role::Node(i), Border::Rounded);
        }
        canvas.line((1, 3), (1, 6), Role::Edge, Stroke::Solid);
        canvas.line((7, 7), (7, 11), Role::Edge, Stroke::Solid);
        for gap in 0..=4 {
            let packed = canvas.clone().compact_vertical(gap);
            let bounds: Vec<_> = (0..4)
                .map(|node| {
                    let rows: Vec<_> = packed
                        .cells
                        .chunks(packed.width)
                        .enumerate()
                        .filter(|(_, row)| row.iter().any(|cell| cell.role == Role::Node(node)))
                        .map(|(y, _)| y)
                        .collect();
                    assert_eq!(rows.len(), 3, "gap {gap}: box height changed");
                    (rows[0], rows[2])
                })
                .collect();
            for (upper, lower) in [(0, 1), (2, 3)] {
                let actual_gap = bounds[lower].0 - bounds[upper].1 - 1;
                assert!(
                    actual_gap >= gap,
                    "gap {gap}: only {actual_gap} rows remain"
                );
            }
            assert!(
                packed.height() < canvas.height(),
                "unused outer rows remain"
            );
        }
    }

    #[test]
    fn vertical_compaction_preserves_edge_styles_and_existing_crossings() {
        for (first, second) in [
            (Stroke::Solid, Stroke::Solid),
            (Stroke::Dashed, Stroke::Thick),
        ] {
            let mut canvas = Canvas::new(16, 4, Border::Rounded);
            canvas.text(10, 0, "top", Role::Text, Emphasis::default());
            canvas.text(10, 3, "end", Role::Text, Emphasis::default());
            for (points, stroke) in [
                ([(1, 0), (1, 1), (3, 1), (3, 3)], first),
                ([(6, 0), (6, 2), (8, 2), (8, 3)], second),
            ] {
                for pair in points.windows(2) {
                    canvas.line(pair[0], pair[1], Role::Edge, stroke);
                }
            }
            if first == second {
                canvas.line((7, 0), (7, 3), Role::Edge, first);
            }
            let canvas = canvas.compact_vertical(0);
            let text = canvas.to_text(false, false, Theme::Mono);
            assert_eq!(canvas.height(), 3, "{text}");
            if first == second {
                assert_eq!(text.matches('┼').count(), 1, "{text}");
            } else {
                assert!(text.contains('┄') && text.contains('━'), "{text}");
                assert!(!text.contains(['┼', '╋']), "{text}");
            }
        }
    }

    #[test]
    fn vertical_compaction_keeps_overlapping_horizontal_runs_separate() {
        let mut canvas = Canvas::new(16, 4, Border::Rounded);
        canvas.text(10, 0, "top", Role::Text, Emphasis::default());
        canvas.text(10, 3, "end", Role::Text, Emphasis::default());
        for points in [
            [(1, 0), (1, 1), (5, 1), (5, 3)],
            [(3, 0), (3, 2), (7, 2), (7, 3)],
        ] {
            for pair in points.windows(2) {
                canvas.line(pair[0], pair[1], Role::Edge, Stroke::Solid);
            }
        }
        let original = canvas.to_text(false, false, Theme::Mono);
        assert_eq!(
            canvas
                .compact_vertical(0)
                .to_text(false, false, Theme::Mono),
            original
        );
    }

    #[test]
    fn vertical_compaction_keeps_labels_and_explicit_markers_between_bends() {
        for (glyph, role) in [("○", Role::Edge), ("×", Role::Edge), ("你好", Role::Label)] {
            let mut canvas = Canvas::new(16, 3, Border::Rounded);
            canvas.line((1, 0), (3, 0), Role::Edge, Stroke::Solid);
            canvas.line((6, 2), (8, 2), Role::Edge, Stroke::Solid);
            canvas.text(10, 1, glyph, role, Emphasis::ITALIC);
            let original = canvas.to_text(false, false, Theme::Mono);
            assert_eq!(
                canvas
                    .compact_vertical(0)
                    .to_text(false, false, Theme::Mono),
                original
            );
        }
    }

    #[test]
    fn cjk_emoji_and_combining_marks_align_and_clip() {
        let mut canvas = Canvas::new(12, 1, Border::Rounded);
        canvas.text(0, 0, "你👩‍💻e\u{301} OK", Role::Text, Emphasis::BOLD);
        assert_eq!(canvas.row(0, 0, 4, false, false, Theme::Ansi), "你👩‍💻");
        assert_eq!(canvas.row(0, 1, 2, false, false, Theme::Ansi), "  ");
        assert_eq!(
            canvas.row(0, 4, 4, false, false, Theme::Ansi),
            "e\u{301} OK"
        );
        assert!(!canvas.to_text(false, false, Theme::Ansi).contains('\x1b'));
        assert!(canvas.to_text(false, true, Theme::Ansi).contains(";1m"));
    }

    #[test]
    fn wrapping_keeps_clusters_and_formatting() {
        let lines = wrap("**你好** world\n*e\u{301}👩‍💻*", 5, Emphasis::default());
        assert!(lines.iter().all(|l| l.width() <= 5));
        assert_eq!(lines[0].text(), "你好");
        assert_eq!(lines[0].0[0].emphasis, Emphasis::BOLD);
        assert_eq!(lines.last().unwrap().text(), "e\u{301}👩‍💻");
    }

    #[test]
    fn literal_operators_and_unmatched_markup_are_preserved() {
        let line = wrap("2 * 3 and unmatched `code", 60, Emphasis::default());
        assert_eq!(line[0].text(), "2 * 3 and unmatched `code");
        let line = wrap("<b>bold</b> and **also bold**", 60, Emphasis::default());
        assert_eq!(line[0].text(), "bold and also bold");
        assert_eq!(line[0].0[0].emphasis, Emphasis::BOLD);
    }
}
