use std::{
    cmp::Reverse,
    collections::{BinaryHeap, HashSet},
};

use super::*;

type Point = (usize, usize);

mod layout;
mod pack;
mod simplify;

fn ports(from: &BoxNode, to: &BoxNode, direction: Direction) -> (Point, Point, Point, Point) {
    if std::ptr::eq(from, to) {
        return (
            (from.right(), from.cy()),
            (from.right() + 1, from.cy()),
            (from.cx(), from.y - 1),
            (from.cx(), from.y),
        );
    }
    if direction == Direction::Lr && to.x > from.right() {
        (
            (from.right(), from.cy()),
            (from.right() + 1, from.cy()),
            (to.x - 1, to.cy()),
            (to.x, to.cy()),
        )
    } else if direction == Direction::Rl && from.x > to.right() {
        (
            (from.x, from.cy()),
            (from.x - 1, from.cy()),
            (to.right() + 1, to.cy()),
            (to.right(), to.cy()),
        )
    } else if direction.horizontal() && (from.x > to.right() || to.x > from.right()) {
        // Feedback edges use the outside of each layer instead of doubling back
        // through the main forward port.
        (
            (from.cx(), from.bottom()),
            (from.cx(), from.bottom() + 1),
            (to.cx(), to.bottom() + 1),
            (to.cx(), to.bottom()),
        )
    } else if direction == Direction::Tb && to.y > from.bottom() {
        (
            (from.cx(), from.bottom()),
            (from.cx(), from.bottom() + 1),
            (to.cx(), to.y - 1),
            (to.cx(), to.y),
        )
    } else if direction == Direction::Bt && from.y > to.bottom() {
        (
            (from.cx(), from.y),
            (from.cx(), from.y - 1),
            (to.cx(), to.bottom() + 1),
            (to.cx(), to.bottom()),
        )
    } else if !direction.horizontal() && (from.y > to.bottom() || to.y > from.bottom()) {
        (
            (from.right(), from.cy()),
            (from.right() + 1, from.cy()),
            (to.right() + 1, to.cy()),
            (to.right(), to.cy()),
        )
    } else if from.x < to.x {
        (
            (from.right(), from.cy()),
            (from.right() + 1, from.cy()),
            (to.x - 1, to.cy()),
            (to.x, to.cy()),
        )
    } else {
        (
            (from.x, from.cy()),
            (from.x - 1, from.cy()),
            (to.right() + 1, to.cy()),
            (to.right(), to.cy()),
        )
    }
}

struct Router {
    width: usize,
    height: usize,
    blocked: Vec<bool>,
    label_space: Vec<bool>,
    used: Vec<u16>,
    tracks: Vec<u8>,
    ports: Vec<bool>,
}

impl Router {
    fn index(&self, point: Point) -> usize {
        point.1 * self.width + point.0
    }

    fn detour_anchor(&self, preferred: Point, width: usize, height: usize, reach: usize) -> Point {
        if width + 2 >= self.width || height + 2 >= self.height {
            return preferred;
        }
        for distance in 0..=reach.min(self.width + self.height) {
            for dx in -(distance as isize)..=distance as isize {
                let dy = (distance - dx.unsigned_abs()) as isize;
                for dy in [dy, -dy] {
                    let point = (
                        preferred.0.saturating_add_signed(dx),
                        preferred.1.saturating_add_signed(dy),
                    );
                    let (Some(left), Some(top)) = (
                        point.0.checked_sub(width / 2 + 1),
                        point.1.checked_sub(height / 2 + 1),
                    ) else {
                        continue;
                    };
                    let (right, bottom) = (left + width + 2, top + height + 2);
                    if left == 0 || top == 0 || right >= self.width || bottom >= self.height {
                        continue;
                    }
                    if (top..bottom).all(|y| {
                        (left..right).all(|x| {
                            let i = y * self.width + x;
                            !self.blocked[i]
                                && !self.label_space[i]
                                && !self.ports[i]
                                && self.used[i] == 0
                        })
                    }) {
                        return point;
                    }
                }
            }
        }
        preferred
    }

    fn route(
        &self,
        start: Point,
        end: Point,
        departure: Option<(isize, isize)>,
        arrival: Option<(isize, isize)>,
    ) -> Result<Vec<Point>, RenderError> {
        let area = self.width * self.height;
        let mut distance = vec![u32::MAX; area * 2];
        let mut previous = vec![usize::MAX; area * 2];
        let mut queue = BinaryHeap::new();
        let start_i = self.index(start);
        let end_i = self.index(end);
        for axis in 0..2 {
            distance[start_i * 2 + axis] = 0;
            queue.push(Reverse((0u32, 0u32, start_i * 2 + axis)));
        }
        let mut iterations = 0;
        while let Some(Reverse((_, cost, state))) = queue.pop() {
            if cost != distance[state] {
                continue;
            }
            let i = state / 2;
            if i == end_i {
                let mut path = vec![(i % self.width, i / self.width)];
                let mut current = state;
                while previous[current] != usize::MAX {
                    current = previous[current];
                    path.push(((current / 2) % self.width, (current / 2) / self.width));
                }
                path.reverse();
                return Ok(path);
            }
            iterations += 1;
            if iterations > 300_000 {
                break;
            }
            let x = i % self.width;
            let y = i / self.width;
            for (dx, dy, axis) in [(1isize, 0isize, 0), (0, 1, 1), (-1, 0, 0), (0, -1, 1)] {
                if i == start_i && departure.is_some_and(|d| d != (dx, dy)) {
                    continue;
                }
                let nx = x as isize + dx;
                let ny = y as isize + dy;
                if nx < 1
                    || ny < 1
                    || nx >= self.width as isize - 1
                    || ny >= self.height as isize - 1
                {
                    continue;
                }
                let next = ny as usize * self.width + nx as usize;
                if ((self.blocked[next] || self.label_space[next] || self.ports[next])
                    && next != end_i)
                    || (next == end_i && arrival.is_some_and(|d| d != (dx, dy)))
                {
                    continue;
                }
                let bend = if state % 2 == axis { 0 } else { 20 };
                let occupied = if self.tracks[next] & (1 << axis) != 0 {
                    4000
                } else if self.tracks[next] != 0 {
                    2000
                } else {
                    0
                };
                let candidate = cost + 10 + bend + occupied;
                let next_state = next * 2 + axis;
                if candidate < distance[next_state] {
                    distance[next_state] = candidate;
                    previous[next_state] = state;
                    let heuristic =
                        (nx.abs_diff(end.0 as isize) + ny.abs_diff(end.1 as isize)) as u32 * 10;
                    queue.push(Reverse((candidate + heuristic, candidate, next_state)));
                }
            }
        }
        Err(RenderError(
            "could not route an edge; increase --gap or reduce diagram size".into(),
        ))
    }

    fn edge(
        &mut self,
        attachments: (Point, Point, Point, Point),
        planned: &[Point],
        detour: Option<(Point, (isize, isize))>,
    ) -> Result<Vec<Point>, RenderError> {
        let (source, start, end, target) = attachments;
        let departure = (
            start.0 as isize - source.0 as isize,
            start.1 as isize - source.1 as isize,
        );
        let arrival = (
            target.0 as isize - end.0 as isize,
            target.1 as isize - end.1 as isize,
        );
        let mut path = Vec::new();
        if !planned.is_empty() {
            path.push(planned[0]);
            for &target in &planned[1..] {
                let mut point = *path.last().unwrap();
                while point != target {
                    if point.0 != target.0 {
                        point.0 = point
                            .0
                            .saturating_add_signed((target.0 as isize - point.0 as isize).signum());
                    } else {
                        point.1 = point
                            .1
                            .saturating_add_signed((target.1 as isize - point.1 as isize).signum());
                    }
                    path.push(point);
                }
            }
        } else if let Some((anchor, flow)) = detour {
            path = self.route(start, anchor, Some(departure), Some(flow))?;
            path.extend(
                self.route(anchor, end, Some(flow), Some(arrival))?
                    .into_iter()
                    .skip(1),
            );
        } else {
            path = self.route(start, end, Some(departure), Some(arrival))?;
        }
        let mut visited = HashSet::new();
        for &point in &path {
            if visited.insert(point) {
                let i = self.index(point);
                self.used[i] = self.used[i].saturating_add(1);
            }
        }
        for pair in path.windows(2) {
            let axis = u8::from(pair[0].0 == pair[1].0);
            for &point in pair {
                let i = self.index(point);
                self.tracks[i] |= 1 << axis;
            }
        }
        Ok(path)
    }

    fn label(
        &mut self,
        canvas: &mut Canvas,
        path: &[Point],
        lines: &[TextLine],
        anchor: Option<Point>,
    ) -> Result<Point, RenderError> {
        let width = lines.iter().map(label_width).max().unwrap_or(0);
        let height = lines.len();
        if width == 0 {
            return Ok(anchor.unwrap_or_default());
        }
        let path_cells: HashSet<_> = path.iter().copied().collect();
        let mut points: Vec<_> = path.iter().enumerate().collect();
        points.sort_by_key(|&(i, &(x, y))| {
            anchor.map_or_else(
                || i.abs_diff(path.len() / 2),
                |(ax, ay)| x.abs_diff(ax) + y.abs_diff(ay),
            )
        });
        for &(_, &(px, py)) in &points {
            let (Some(x), Some(y)) = (px.checked_sub(width / 2), py.checked_sub(height / 2)) else {
                continue;
            };
            if x == 0 || y == 0 || x + width >= self.width || y + height >= self.height {
                continue;
            }
            // Cover only this edge, leaving room beside other labels, nodes,
            // and arrowheads.
            let left = x - 1;
            let right = x + width + 1;
            if (y..y + height).any(|row| {
                (left..right).any(|col| {
                    self.blocked[row * self.width + col] || self.ports[row * self.width + col]
                })
            }) || (y - 1..(y + height + 1).min(self.height))
                .any(|row| (left..right).any(|col| self.ports[row * self.width + col]))
                || (y..y + height).any(|row| {
                    (x..x + width).any(|col| {
                        !canvas.is_blank(col, row)
                            && (!path_cells.contains(&(col, row))
                                || self.used[row * self.width + col] > 1)
                    })
                })
            {
                continue;
            }
            for row in y..y + height {
                for col in left..right {
                    self.blocked[row * self.width + col] = true;
                }
            }
            canvas.blank_rect(x, y, width, height);
            for (row, line) in lines.iter().enumerate() {
                draw_label(canvas, px - label_width(line) / 2, y + row, line);
            }
            return Ok((px, py));
        }
        Err(RenderError(
            "not enough room for an edge label; increase --gap, shorten labels, or use --no-labels"
                .into(),
        ))
    }
}

pub(super) fn render(graph: &Graph, options: &RenderOptions) -> Result<Canvas, RenderError> {
    if graph.nodes.is_empty() {
        return Err(RenderError("graph contains no nodes".into()));
    }
    if graph.nodes.len() > 256
        || graph.edges.len() > 1024
        || graph
            .edges
            .iter()
            .any(|e| e.from >= graph.nodes.len() || e.to >= graph.nodes.len())
    {
        return Err(RenderError(
            "invalid graph or diagram limits exceeded".into(),
        ));
    }
    let mut layout = layout::layout(graph, options);
    let mut canvas = checked_canvas(layout.width, layout.height, options.border)?;
    let area = layout.width * layout.height;
    let mut router = Router {
        width: layout.width,
        height: layout.height,
        blocked: vec![false; area],
        label_space: vec![false; area],
        used: vec![0; area],
        tracks: vec![0; area],
        ports: vec![false; area],
    };
    for (i, node) in layout.nodes.iter().enumerate() {
        draw_node(&mut canvas, node, i, options);
        for y in node.y..=node.bottom() {
            for x in node.x..=node.right() {
                router.blocked[y * layout.width + x] = true;
            }
        }
    }
    let all_ports = &layout.ports;
    for (_, start, end, _) in all_ports {
        router.ports[start.1 * layout.width + start.0] = true;
        router.ports[end.1 * layout.width + end.0] = true;
    }
    if options.edge_labels {
        for (edge, anchor) in graph.edges.iter().zip(&layout.label_anchors) {
            let Some((x, y)) = *anchor else {
                continue;
            };
            if edge.label.is_empty() {
                continue;
            }
            let lines = wrap(&edge.label, options.node_width, Emphasis::ITALIC);
            let width = lines.iter().map(label_width).max().unwrap_or(0);
            let height = lines.len();
            let left = x.saturating_sub(width / 2 + 1);
            let top = y.saturating_sub(height / 2 + 1);
            for row in top..(top + height + 2).min(layout.height) {
                for col in left..(left + width + 2).min(layout.width) {
                    router.label_space[row * layout.width + col] = true;
                }
            }
        }
    }
    // Route feedback and self edges around the planned forward lanes.
    let direction = options.direction.unwrap_or(graph.direction);
    let mut order: Vec<_> = (0..graph.edges.len()).collect();
    order.sort_by_key(|i| {
        let (_, a, b, _) = all_ports[*i];
        let (main, span) = if direction.horizontal() {
            (a.0, a.1.abs_diff(b.1))
        } else {
            (a.1, a.0.abs_diff(b.0))
        };
        let main = match direction {
            Direction::Bt => layout.height - main,
            Direction::Rl => layout.width - main,
            _ => main,
        };
        (
            layout.routes[*i].is_empty(),
            main,
            graph.edges[*i].from,
            Reverse(span),
        )
    });
    let mut markers = Vec::new();
    let mut paths = vec![Vec::new(); graph.edges.len()];
    let mut label_anchors = layout.label_anchors.clone();
    for &i in &order {
        let edge = &graph.edges[i];
        let (source, start, end, target) = all_ports[i];
        let detour = if options.edge_labels && layout.routes[i].is_empty() && !edge.label.is_empty()
        {
            let lines = wrap(&edge.label, options.node_width, Emphasis::ITALIC);
            let width = lines.iter().map(label_width).max().unwrap_or(0);
            let dx = start.0 as isize - source.0 as isize;
            let dy = end.1 as isize - target.1 as isize;
            let (preferred, flow) = if edge.from == edge.to {
                (
                    (
                        start.0.saturating_add_signed(
                            dx * (width / 2 + options.horizontal_gap() + 2) as isize,
                        ),
                        target.1.saturating_add_signed(
                            dy * (lines.len() + options.vertical_gap() + 2) as isize,
                        ),
                    ),
                    (0, dy),
                )
            } else if direction.horizontal() {
                // Feedback labels sit beyond the outside ports, where even
                // narrow boxes leave room for the caption and its approach.
                (
                    (
                        (start.0 + end.0) / 2,
                        start.1.max(end.1) + lines.len() / 2 + 2,
                    ),
                    (if direction == Direction::Lr { -1 } else { 1 }, 0),
                )
            } else {
                (
                    (start.0.max(end.0) + width / 2 + 2, (start.1 + end.1) / 2),
                    (0, if direction == Direction::Tb { -1 } else { 1 }),
                )
            };
            let anchor = router.detour_anchor(
                preferred,
                width,
                lines.len(),
                options.horizontal_gap().max(options.vertical_gap()) + width.max(lines.len()) + 2,
            );
            label_anchors[i] = Some(anchor);
            Some((anchor, flow))
        } else {
            None
        };
        let path = router
            .edge(all_ports[i], &layout.routes[i], detour)
            .map_err(|error| {
                RenderError(format!(
                    "edge {} -> {}: {error}",
                    graph.nodes[edge.from].id, graph.nodes[edge.to].id
                ))
            })?;
        canvas.attach(
            source,
            start,
            edge.stroke == Stroke::Thick || edge.start == Arrow::Point,
        );
        canvas.attach(
            target,
            end,
            edge.stroke == Stroke::Thick || edge.end == Arrow::Point,
        );
        for segment in path.windows(2) {
            canvas.line(segment[0], segment[1], Role::Edge, edge.stroke);
        }
        paths[i] = path;
        // Point connections use the weighted border junction. Explicit circle
        // and cross endings retain their distinct markers beside the border.
        for (point, border, marker) in [(end, target, edge.end), (start, source, edge.start)] {
            if matches!(marker, Arrow::Circle | Arrow::Cross) {
                markers.push((
                    point,
                    (
                        border.0 as isize - point.0 as isize,
                        border.1 as isize - point.1 as isize,
                    ),
                    marker,
                ));
            }
        }
    }
    // Route first, then place labels where they cover only their own edge.
    // Virtual vertices reserve label space without becoming visible junctions.
    if options.edge_labels {
        for (i, (edge, path)) in graph.edges.iter().zip(&paths).enumerate() {
            if !edge.label.is_empty() {
                let anchor = router
                    .label(
                        &mut canvas,
                        path,
                        &wrap(&edge.label, options.node_width, Emphasis::ITALIC),
                        label_anchors[i],
                    )
                    .map_err(|error| {
                        RenderError(format!(
                            "edge {} -> {}: {error}",
                            graph.nodes[edge.from].id, graph.nodes[edge.to].id
                        ))
                    })?;
                label_anchors[i] = Some(anchor);
            }
        }
    }
    for (p, direction, marker) in markers {
        arrow(&mut canvas, p, direction, marker, options);
    }
    Ok(if options.vertical_gap() == 0 {
        layout.routes = paths.iter().map(|path| simplify::compact(path)).collect();
        layout.label_anchors = label_anchors;
        if pack::vertical(&mut layout, graph, options, &layout::ranks(graph)) {
            canvas = paint(&layout, graph, options, &order)?;
        }
        canvas.compact_vertical()
    } else {
        canvas
    })
}

fn paint(
    layout: &layout::Layout,
    graph: &Graph,
    options: &RenderOptions,
    order: &[usize],
) -> Result<Canvas, RenderError> {
    let mut canvas = checked_canvas(layout.width, layout.height, options.border)?;
    for (i, node) in layout.nodes.iter().enumerate() {
        draw_node(&mut canvas, node, i, options);
    }
    for &i in order {
        let edge = &graph.edges[i];
        let (source, start, end, target) = layout.ports[i];
        canvas.attach(
            source,
            start,
            edge.stroke == Stroke::Thick || edge.start == Arrow::Point,
        );
        canvas.attach(
            target,
            end,
            edge.stroke == Stroke::Thick || edge.end == Arrow::Point,
        );
        for pair in layout.routes[i].windows(2) {
            canvas.line(pair[0], pair[1], Role::Edge, edge.stroke);
        }
    }
    if options.edge_labels {
        for (edge, anchor) in graph.edges.iter().zip(&layout.label_anchors) {
            if edge.label.is_empty() {
                continue;
            }
            let Some((x, y)) = *anchor else {
                continue;
            };
            let lines = wrap(&edge.label, options.node_width, Emphasis::ITALIC);
            let width = lines.iter().map(label_width).max().unwrap_or(0);
            let top = y - lines.len() / 2;
            canvas.blank_rect(x - width / 2, top, width, lines.len());
            for (row, line) in lines.iter().enumerate() {
                draw_label(&mut canvas, x - label_width(line) / 2, top + row, line);
            }
        }
    }
    for &i in order {
        let edge = &graph.edges[i];
        let (source, start, end, target) = layout.ports[i];
        for (point, border, marker) in [(end, target, edge.end), (start, source, edge.start)] {
            if matches!(marker, Arrow::Circle | Arrow::Cross) {
                arrow(
                    &mut canvas,
                    point,
                    (
                        border.0 as isize - point.0 as isize,
                        border.1 as isize - point.1 as isize,
                    ),
                    marker,
                    options,
                );
            }
        }
    }
    Ok(canvas)
}
