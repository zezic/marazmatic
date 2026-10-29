use super::*;

pub(super) struct Layout {
    pub nodes: Vec<BoxNode>,
    pub width: usize,
    pub height: usize,
    pub ports: Vec<(Point, Point, Point, Point)>,
    pub routes: Vec<Vec<Point>>,
    pub label_anchors: Vec<Option<Point>>,
}

fn ranks(graph: &Graph) -> Vec<usize> {
    // A DFS removes feedback edges only for ranking. They are still routed and
    // rendered, so cycles and self loops preserve their original semantics.
    fn visit(
        node: usize,
        graph: &Graph,
        color: &mut [u8],
        feedback: &mut [bool],
        order: &mut Vec<usize>,
    ) {
        color[node] = 1;
        for (i, edge) in graph
            .edges
            .iter()
            .enumerate()
            .filter(|(_, e)| e.from == node)
        {
            if color[edge.to] == 1 {
                feedback[i] = true;
            } else if color[edge.to] == 0 {
                visit(edge.to, graph, color, feedback, order);
            }
        }
        color[node] = 2;
        order.push(node);
    }
    let mut order = Vec::new();
    let mut color = vec![0; graph.nodes.len()];
    let mut feedback = vec![false; graph.edges.len()];
    for node in 0..graph.nodes.len() {
        if color[node] == 0 {
            visit(node, graph, &mut color, &mut feedback, &mut order);
        }
    }
    let mut rank = vec![0; graph.nodes.len()];
    for from in order.into_iter().rev() {
        for (i, edge) in graph
            .edges
            .iter()
            .enumerate()
            .filter(|(_, e)| e.from == from)
        {
            if !feedback[i] {
                rank[edge.to] = rank[edge.to].max(rank[from] + 1);
            }
        }
    }
    rank
}

// Long edges participate in ordering at every intervening rank. Virtual
// vertices reserve space; whole-route refinement can span several ranks.
struct Layers {
    order: Vec<Vec<usize>>,
    before: Vec<Vec<usize>>,
    after: Vec<Vec<usize>>,
    chains: Vec<Vec<usize>>,
}

impl Layers {
    fn new(graph: &Graph, rank: &[usize]) -> Self {
        let count = graph.nodes.len();
        let mut result = Self {
            order: vec![Vec::new(); rank.iter().max().copied().unwrap_or(0) + 1],
            before: vec![Vec::new(); count],
            after: vec![Vec::new(); count],
            chains: vec![Vec::new(); graph.edges.len()],
        };
        for (node, &r) in rank.iter().enumerate() {
            result.order[r].push(node);
        }
        for (i, edge) in graph.edges.iter().enumerate() {
            if rank[edge.from] >= rank[edge.to] {
                continue;
            }
            let mut previous = edge.from;
            for r in rank[edge.from] + 1..rank[edge.to] {
                let virtual_node = result.before.len();
                result.before.push(Vec::new());
                result.after.push(Vec::new());
                result.order[r].push(virtual_node);
                result.chains[i].push(virtual_node);
                result.connect(previous, virtual_node);
                previous = virtual_node;
            }
            result.connect(previous, edge.to);
        }
        result
    }

    fn connect(&mut self, from: usize, to: usize) {
        self.after[from].push(to);
        self.before[to].push(from);
    }

    fn positions(&self) -> Vec<usize> {
        let mut positions = vec![0; self.before.len()];
        for row in &self.order {
            for (position, &node) in row.iter().enumerate() {
                positions[node] = position;
            }
        }
        positions
    }

    fn crossings(&self) -> usize {
        let positions = self.positions();
        let mut crossings = 0;
        for pair in self.order.windows(2) {
            let mut tree = vec![0usize; pair[1].len() + 1];
            let mut seen = 0;
            for &node in &pair[0] {
                for &target in &self.after[node] {
                    let mut i = positions[target] + 1;
                    let mut preceding = 0;
                    while i > 0 {
                        preceding += tree[i];
                        i &= i - 1;
                    }
                    crossings += seen - preceding;
                }
                // Edges sharing a source or target do not cross one another.
                for &target in &self.after[node] {
                    let mut i = positions[target] + 1;
                    while i < tree.len() {
                        tree[i] += 1;
                        i += i.isolate_lowest_one();
                    }
                    seen += 1;
                }
            }
        }
        crossings
    }

    fn order(&mut self) {
        let mut best = self.order.clone();
        let mut best_crossings = self.crossings();
        for pass in 0..12 {
            let downward = pass % 2 == 0;
            let mut positions = self.positions();
            let ranks: Vec<_> = if downward {
                (1..self.order.len()).collect()
            } else {
                (0..self.order.len().saturating_sub(1)).rev().collect()
            };
            for r in ranks {
                let neighbors = if downward { &self.before } else { &self.after };
                self.order[r].sort_by_cached_key(|&node| {
                    let adjacent = &neighbors[node];
                    if adjacent.is_empty() {
                        positions[node] * 1024
                    } else {
                        adjacent.iter().map(|&n| positions[n]).sum::<usize>() * 1024
                            / adjacent.len()
                    }
                });
                for (position, &node) in self.order[r].iter().enumerate() {
                    positions[node] = position;
                }
            }
            // Adjacent swaps improve both sides of a rank, including places
            // where equal barycenters otherwise keep a crossing in place.
            for _ in 0..2 {
                let mut changed = false;
                for row in &mut self.order {
                    for i in 1..row.len() {
                        let (a, b) = (row[i - 1], row[i]);
                        let mut delta = 0isize;
                        for neighbors in [&self.before, &self.after] {
                            for &left in &neighbors[a] {
                                for &right in &neighbors[b] {
                                    delta += match positions[left].cmp(&positions[right]) {
                                        std::cmp::Ordering::Less => 1,
                                        std::cmp::Ordering::Equal => 0,
                                        std::cmp::Ordering::Greater => -1,
                                    };
                                }
                            }
                        }
                        if delta < 0 {
                            row.swap(i - 1, i);
                            positions[a] = i;
                            positions[b] = i - 1;
                            changed = true;
                        }
                    }
                }
                if !changed {
                    break;
                }
            }
            let crossings = self.crossings();
            if crossings < best_crossings {
                best_crossings = crossings;
                best.clone_from(&self.order);
            }
        }
        self.order = best;
    }
}

fn cross_size(node: &BoxNode, horizontal: bool) -> usize {
    if horizontal { node.height } else { node.width }
}

fn offsets(row: &[usize], sizes: &[usize], real_count: usize, gap: usize) -> Vec<i64> {
    let mut offsets = vec![0; row.len()];
    for i in 1..row.len() {
        let (a, b) = (row[i - 1], row[i]);
        let gap = if a >= real_count && b >= real_count {
            2
        } else {
            gap
        };
        offsets[i] = offsets[i - 1] + (sizes[a].div_ceil(2) + sizes[b] / 2 + gap) as i64;
    }
    offsets
}

// Project desired neighbor centers onto non-overlapping ordered positions.
// Pooling adjacent conflicts avoids pushing every later node to one side.
fn align(row: &[usize], offsets: &[i64], desired: &[i64], centers: &mut [i64]) {
    let mut groups: Vec<(usize, usize, i64)> = Vec::new();
    for (i, &node) in row.iter().enumerate() {
        groups.push((i, 1, desired[node] - offsets[i]));
        while groups.len() >= 2 {
            let b = groups[groups.len() - 1];
            let a = groups[groups.len() - 2];
            if a.2 * b.1 as i64 <= b.2 * a.1 as i64 {
                break;
            }
            groups.pop();
            groups.pop();
            groups.push((a.0, a.1 + b.1, a.2 + b.2));
        }
    }
    for (start, count, total) in groups {
        let mean = total.div_euclid(count as i64);
        for i in start..start + count {
            centers[row[i]] = mean + offsets[i];
        }
    }
}

struct PortOffsets {
    before: Vec<Vec<i64>>,
    after: Vec<Vec<i64>>,
}

fn align_centers(
    layers: &Layers,
    offsets: &[Vec<i64>],
    centers: &mut [i64],
    deltas: Option<&PortOffsets>,
) {
    for pass in 0..12 {
        let downward = pass % 2 == 0;
        let indices: Vec<_> = if downward {
            (1..layers.order.len()).collect()
        } else {
            (0..layers.order.len().saturating_sub(1)).rev().collect()
        };
        for r in indices {
            let neighbors = if downward {
                &layers.before
            } else {
                &layers.after
            };
            let mut desired = centers.to_vec();
            for &node in &layers.order[r] {
                let mut adjacent: Vec<_> = neighbors[node]
                    .iter()
                    .enumerate()
                    .map(|(i, &n)| {
                        centers[n]
                            + deltas.map_or(0, |deltas| {
                                if downward {
                                    deltas.before[node][i]
                                } else {
                                    deltas.after[node][i]
                                }
                            })
                    })
                    .collect();
                if !adjacent.is_empty() {
                    adjacent.sort_unstable();
                    desired[node] = (adjacent[(adjacent.len() - 1) / 2]
                        + adjacent[adjacent.len() / 2])
                        .div_euclid(2);
                }
            }
            align(&layers.order[r], &offsets[r], &desired, centers);
        }
    }
}

pub(super) fn layout(graph: &Graph, options: &RenderOptions) -> Layout {
    let direction = options.direction.unwrap_or(graph.direction);
    let horizontal = direction.horizontal();
    let mut nodes: Vec<_> = graph
        .nodes
        .iter()
        .map(|n| box_node(&n.label, n.shape, options))
        .collect();
    // Interleave routing ranks with node ranks. Even a short labeled edge gets
    // an anchor with enough room for its text between its two nodes.
    let rank: Vec<_> = ranks(graph).into_iter().map(|r| r * 2).collect();
    let mut outgoing = vec![0usize; nodes.len()];
    let mut incoming = outgoing.clone();
    for edge in &graph.edges {
        outgoing[edge.from] += 1;
        incoming[edge.to] += 1;
    }
    for (i, node) in nodes.iter_mut().enumerate() {
        if matches!(node.shape, Shape::Start | Shape::End) {
            continue;
        }
        let ports = outgoing[i].max(incoming[i]);
        if horizontal {
            node.height = node.height.max(ports * 2 + 1);
        } else {
            node.width = node.width.max(ports * 2 + 1);
        }
    }
    let mut layers = Layers::new(graph, &rank);
    layers.order();
    let mut sizes = vec![1usize; layers.before.len()];
    let mut main_sizes = vec![1usize; sizes.len()];
    for (i, node) in nodes.iter().enumerate() {
        sizes[i] = cross_size(node, horizontal);
        main_sizes[i] = if horizontal { node.width } else { node.height };
    }
    if options.edge_labels {
        for (edge, chain) in graph.edges.iter().zip(&layers.chains) {
            if edge.label.is_empty() {
                continue;
            }
            let lines = wrap(&edge.label, options.node_width, Emphasis::ITALIC);
            let cross_size = if horizontal {
                lines.len()
            } else {
                lines.iter().map(label_width).max().unwrap_or(1)
            };
            let main_size = if horizontal {
                lines.iter().map(label_width).max().unwrap_or(1)
            } else {
                lines.len()
            };
            if let Some(&node) = chain.get(chain.len() / 2) {
                sizes[node] = cross_size;
                main_sizes[node] = main_size;
            }
        }
    }
    let cross_gap = options.gap + if horizontal { 2 } else { 4 };
    let offsets: Vec<_> = layers
        .order
        .iter()
        .map(|row| offsets(row, &sizes, nodes.len(), cross_gap))
        .collect();
    let mut centers = vec![0i64; sizes.len()];
    for (row, offsets) in layers.order.iter().zip(&offsets) {
        let middle = offsets.last().copied().unwrap_or(0) / 2;
        for (i, &node) in row.iter().enumerate() {
            centers[node] = offsets[i] - middle;
        }
    }
    align_centers(&layers, &offsets, &mut centers, None);
    let label_lines: Vec<_> = graph
        .edges
        .iter()
        .filter(|e| options.edge_labels && !e.label.is_empty())
        .map(|e| wrap(&e.label, options.node_width, Emphasis::ITALIC))
        .collect();
    let max_label_width = label_lines
        .iter()
        .flatten()
        .map(label_width)
        .max()
        .unwrap_or(0);
    let margin = max_label_width.max(8) + options.gap + 3;
    let place = |nodes: &mut [BoxNode], gaps: &[usize], centers: &[i64]| {
        let min_cross = centers
            .iter()
            .enumerate()
            .map(|(i, &c)| c - (sizes[i] / 2) as i64)
            .min()
            .unwrap_or(0);
        let max_cross = centers
            .iter()
            .enumerate()
            .map(|(i, &c)| c + sizes[i].div_ceil(2) as i64)
            .max()
            .unwrap_or(0);
        let cross_extent = (max_cross - min_cross) as usize + 2 * margin;
        let mut main = margin;
        let mut positions = vec![(0, 0); sizes.len()];
        let mut bands = Vec::new();
        for (r, row) in layers.order.iter().enumerate() {
            let main_size = row.iter().map(|&i| main_sizes[i]).max().unwrap_or(1);
            for &i in row {
                let cross = (centers[i] - min_cross) as usize + margin;
                positions[i] = if horizontal {
                    (main + main_size / 2, cross)
                } else {
                    (cross, main + main_size / 2)
                };
                if let Some(node) = nodes.get_mut(i) {
                    node.x = positions[i].0 - node.width / 2;
                    node.y = positions[i].1 - node.height / 2;
                }
            }
            bands.push(main + main_size + 1);
            main += main_size + gaps[r];
        }
        let extent = nodes
            .iter()
            .map(|n| if horizontal { n.right() } else { n.bottom() })
            .max()
            .unwrap()
            + margin
            + 1;
        (positions, bands, extent, cross_extent)
    };
    let canonical = if horizontal {
        Direction::Lr
    } else {
        Direction::Tb
    };
    let initial_gaps = vec![options.gap + 3; layers.order.len()];
    let (mut initial_positions, _, _, _) = place(&mut nodes, &initial_gaps, &centers);
    let mut attachments: Vec<_> = graph
        .edges
        .iter()
        .map(|e| super::ports(&nodes[e.from], &nodes[e.to], canonical))
        .collect();
    distribute_ports(
        graph,
        &nodes,
        &layers.chains,
        &initial_positions,
        &mut attachments,
    );
    // Real port positions add offsets to the alignment constraints. This lets
    // boxes move into a straight connection without squeezing their neighbors.
    let cross = |point: Point| {
        if horizontal {
            point.1 as i64
        } else {
            point.0 as i64
        }
    };
    let mut deltas = PortOffsets {
        before: vec![Vec::new(); layers.before.len()],
        after: vec![Vec::new(); layers.after.len()],
    };
    for (i, edge) in graph.edges.iter().enumerate() {
        if layers.chains[i].is_empty() {
            continue;
        }
        let source = cross(attachments[i].0) - cross(initial_positions[edge.from]);
        let target = cross(attachments[i].3) - cross(initial_positions[edge.to]);
        let mut previous = edge.from;
        for node in layers.chains[i]
            .iter()
            .copied()
            .chain(std::iter::once(edge.to))
        {
            let delta = if previous == edge.from { source } else { 0 }
                - if node == edge.to { target } else { 0 };
            deltas.before[node].push(delta);
            deltas.after[previous].push(-delta);
            previous = node;
        }
    }
    align_centers(&layers, &offsets, &mut centers, Some(&deltas));
    (initial_positions, _, _, _) = place(&mut nodes, &initial_gaps, &centers);
    attachments = graph
        .edges
        .iter()
        .map(|e| super::ports(&nodes[e.from], &nodes[e.to], canonical))
        .collect();
    distribute_ports(
        graph,
        &nodes,
        &layers.chains,
        &initial_positions,
        &mut attachments,
    );
    let (lanes, counts) = channels(
        graph,
        &rank,
        &layers.chains,
        &initial_positions,
        &attachments,
        horizontal,
    );
    let gaps: Vec<_> = counts
        .iter()
        .map(|&count| (options.gap.div_ceil(2) + 1).max(count + 3))
        .collect();
    let (mut positions, bands, main_extent, cross_extent) = place(&mut nodes, &gaps, &centers);
    attachments = graph
        .edges
        .iter()
        .map(|e| super::ports(&nodes[e.from], &nodes[e.to], canonical))
        .collect();
    distribute_ports(graph, &nodes, &layers.chains, &positions, &mut attachments);
    let mut routes = vec![Vec::new(); graph.edges.len()];
    for (i, edge) in graph.edges.iter().enumerate() {
        if rank[edge.from] >= rank[edge.to] {
            continue;
        }
        let mut stops = vec![attachments[i].1];
        stops.extend(layers.chains[i].iter().map(|&n| positions[n]));
        stops.push(attachments[i].2);
        routes[i].push(stops[0]);
        for (leg, pair) in stops.windows(2).enumerate() {
            let lane = bands[rank[edge.from] + leg] + lanes[i][leg];
            if horizontal {
                routes[i].extend([(lane, pair[0].1), (lane, pair[1].1), pair[1]]);
            } else {
                routes[i].extend([(pair[0].0, lane), (pair[1].0, lane), pair[1]]);
            }
        }
    }
    let (width, height) = if horizontal {
        (main_extent, cross_extent)
    } else {
        (cross_extent, main_extent)
    };
    let mirror = |point: &mut Point| {
        if direction == Direction::Bt {
            point.1 = height - point.1 - 1;
        } else if direction == Direction::Rl {
            point.0 = width - point.0 - 1;
        }
    };
    if direction == Direction::Bt {
        for node in &mut nodes {
            node.y = height - node.y - node.height;
        }
    } else if direction == Direction::Rl {
        for node in &mut nodes {
            node.x = width - node.x - node.width;
        }
    }
    for point in &mut positions {
        mirror(point);
    }
    for points in &mut routes {
        for point in points {
            mirror(point);
        }
    }
    for (source, start, end, target) in &mut attachments {
        mirror(source);
        mirror(start);
        mirror(end);
        mirror(target);
    }
    let mut result = Layout {
        nodes,
        width,
        height,
        ports: attachments,
        routes,
        label_anchors: layers
            .chains
            .iter()
            .map(|chain| chain.get(chain.len() / 2).map(|&i| positions[i]))
            .collect(),
    };
    super::simplify::routes(&mut result, graph, options);
    result
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Top,
    Right,
    Bottom,
    Left,
}

// Schedule bends together within each gap. A branch that passes another
// branch's departing stem must turn later; a branch that passes its arriving
// stem must turn earlier. Disjoint spans can share a lane.
fn channels(
    graph: &Graph,
    rank: &[usize],
    chains: &[Vec<usize>],
    positions: &[Point],
    ports: &[(Point, Point, Point, Point)],
    horizontal: bool,
) -> (Vec<Vec<usize>>, Vec<usize>) {
    let mut groups = vec![Vec::new(); rank.iter().max().copied().unwrap_or(0) + 1];
    let mut slots = vec![Vec::new(); graph.edges.len()];
    for (i, edge) in graph.edges.iter().enumerate() {
        if rank[edge.from] >= rank[edge.to] {
            continue;
        }
        let mut stops = vec![ports[i].1];
        stops.extend(chains[i].iter().map(|&n| positions[n]));
        stops.push(ports[i].2);
        slots[i] = vec![0; stops.len() - 1];
        for (leg, pair) in stops.windows(2).enumerate() {
            let (a, b) = if horizontal {
                (pair[0].1, pair[1].1)
            } else {
                (pair[0].0, pair[1].0)
            };
            if a != b {
                groups[rank[edge.from] + leg].push((i, leg, a, b));
            }
        }
    }
    let mut counts = vec![0; groups.len()];
    for (r, group) in groups.iter().enumerate() {
        let n = group.len();
        let contains =
            |a: usize, x: usize| group[a].2.min(group[a].3) <= x && x <= group[a].2.max(group[a].3);
        let mut before = vec![vec![false; n]; n];
        let mut indegree = vec![0usize; n];
        for a in 0..n {
            for b in a + 1..n {
                let ab =
                    usize::from(contains(a, group[b].3)) + usize::from(contains(b, group[a].2));
                let ba =
                    usize::from(contains(a, group[b].2)) + usize::from(contains(b, group[a].3));
                let ordering = if ab > ba {
                    Some((a, b))
                } else if ba > ab {
                    Some((b, a))
                } else {
                    None
                };
                if let Some((first, last)) = ordering {
                    before[first][last] = true;
                    indegree[last] += 1;
                }
            }
        }
        let mut assigned = vec![None; n];
        let mut occupied: Vec<Vec<(usize, usize)>> = Vec::new();
        for _ in 0..n {
            // Cyclic constraints reflect crossings in the layer ordering.
            // Break the smallest conflict deterministically when necessary.
            let next = (0..n)
                .filter(|&i| assigned[i].is_none())
                .min_by_key(|&i| (indegree[i], i))
                .unwrap();
            let mut lane = (0..n)
                .filter(|&p| before[p][next])
                .filter_map(|p| assigned[p].map(|lane| lane + 1))
                .max()
                .unwrap_or(0);
            let (_, _, a, b) = group[next];
            let span = (a.min(b), a.max(b));
            while lane < occupied.len()
                && occupied[lane]
                    .iter()
                    .any(|&(lo, hi)| span.0 <= hi && lo <= span.1)
            {
                lane += 1;
            }
            occupied.resize_with(occupied.len().max(lane + 1), Vec::new);
            occupied[lane].push(span);
            assigned[next] = Some(lane);
            slots[group[next].0][group[next].1] = lane;
            for (i, degree) in indegree.iter_mut().enumerate() {
                if before[next][i] {
                    *degree = degree.saturating_sub(1);
                }
            }
        }
        counts[r] = occupied.len();
    }
    (slots, counts)
}

fn side(node: &BoxNode, point: Point) -> Side {
    if point.1 < node.y {
        Side::Top
    } else if point.1 > node.bottom() {
        Side::Bottom
    } else if point.0 < node.x {
        Side::Left
    } else {
        Side::Right
    }
}

fn distribute_ports(
    graph: &Graph,
    nodes: &[BoxNode],
    chains: &[Vec<usize>],
    positions: &[Point],
    ports: &mut [(Point, Point, Point, Point)],
) {
    for (i, node) in nodes.iter().enumerate() {
        for side_at in [Side::Top, Side::Right, Side::Bottom, Side::Left] {
            let horizontal_side = matches!(side_at, Side::Top | Side::Bottom);
            let mut ends = Vec::new();
            for (edge_i, edge) in graph.edges.iter().enumerate() {
                if edge.from == i && side(node, ports[edge_i].1) == side_at {
                    let neighbor = chains[edge_i].first().copied().unwrap_or(edge.to);
                    ends.push((
                        if horizontal_side {
                            positions[neighbor].0
                        } else {
                            positions[neighbor].1
                        },
                        edge_i,
                        true,
                    ));
                }
                if edge.to == i && side(node, ports[edge_i].2) == side_at {
                    let neighbor = chains[edge_i].last().copied().unwrap_or(edge.from);
                    ends.push((
                        if horizontal_side {
                            positions[neighbor].0
                        } else {
                            positions[neighbor].1
                        },
                        edge_i,
                        false,
                    ));
                }
            }
            ends.sort_unstable();
            let length = if horizontal_side {
                node.width
            } else {
                node.height
            };
            let spacing = if ends.len() > 1 {
                length.saturating_sub(3).min(2 * (ends.len() - 1)) / (ends.len() - 1)
            } else {
                0
            };
            let first = (length - 1).saturating_sub(spacing * ends.len().saturating_sub(1)) / 2;
            for (index, &(_, edge_i, source)) in ends.iter().enumerate() {
                let offset = if ends.len() == 1 {
                    length / 2
                } else {
                    first + spacing * index
                };
                let (border, outside) = match side_at {
                    Side::Top => ((node.x + offset, node.y), (node.x + offset, node.y - 1)),
                    Side::Right => (
                        (node.right(), node.y + offset),
                        (node.right() + 1, node.y + offset),
                    ),
                    Side::Bottom => (
                        (node.x + offset, node.bottom()),
                        (node.x + offset, node.bottom() + 1),
                    ),
                    Side::Left => ((node.x, node.y + offset), (node.x - 1, node.y + offset)),
                };
                let attachment = &mut ports[edge_i];
                if source {
                    attachment.0 = border;
                    attachment.1 = outside;
                } else {
                    attachment.2 = outside;
                    attachment.3 = border;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bends(points: &[Point]) -> usize {
        let directions: Vec<_> = points
            .windows(2)
            .filter_map(|pair| {
                (pair[0] != pair[1]).then_some((
                    (pair[1].0 as isize - pair[0].0 as isize).signum(),
                    (pair[1].1 as isize - pair[0].1 as isize).signum(),
                ))
            })
            .collect();
        directions
            .windows(2)
            .filter(|pair| pair[0] != pair[1])
            .count()
    }

    #[test]
    fn placement_aligns_analytics_with_its_blocks_engine_port() {
        let diagram = crate::parse(include_str!("../../../assets/example.mermaid")).unwrap();
        let DiagramKind::Graph(graph) = &diagram.kind else {
            unreachable!()
        };
        let edge = graph
            .edges
            .iter()
            .position(|edge| graph.nodes[edge.from].id == "B" && graph.nodes[edge.to].id == "AN")
            .unwrap();
        for direction in [Direction::Tb, Direction::Bt, Direction::Lr, Direction::Rl] {
            let result = layout(
                graph,
                &RenderOptions {
                    direction: Some(direction),
                    ..RenderOptions::default()
                },
            );
            let (_, from, to, _) = result.ports[edge];
            let offset = if direction.horizontal() {
                from.1.abs_diff(to.1)
            } else {
                from.0.abs_diff(to.0)
            };
            assert_eq!(
                offset, 0,
                "{direction:?}: Analytics is offset from its connection port"
            );
            assert_eq!(
                bends(&result.routes[edge]),
                0,
                "{direction:?}: Analytics needs an unnecessary turn"
            );
        }
    }

    #[test]
    fn a_shortcut_across_one_node_needs_at_most_four_bends() {
        let diagram =
            crate::parse("flowchart TB\nA[Source] --> B[Middle]\nB --> C[Target]\nA --> C")
                .unwrap();
        let DiagramKind::Graph(graph) = &diagram.kind else {
            unreachable!()
        };
        for direction in [Direction::Tb, Direction::Bt, Direction::Lr, Direction::Rl] {
            let result = layout(
                graph,
                &RenderOptions {
                    direction: Some(direction),
                    ..RenderOptions::default()
                },
            );
            let turns = bends(&result.routes[2]);
            assert!(turns <= 4, "{direction:?}: the shortcut has {turns} bends");
            for pair in result.routes[2].windows(2) {
                let (a, b) = (pair[0], pair[1]);
                for node in &result.nodes {
                    let overlaps = a.0.min(b.0) <= node.right()
                        && a.0.max(b.0) >= node.x
                        && a.1.min(b.1) <= node.bottom()
                        && a.1.max(b.1) >= node.y;
                    assert!(!overlaps, "{direction:?}: the shortcut cuts through a box");
                }
            }
        }
    }

    #[test]
    fn supplied_graph_routes_need_at_most_four_bends() {
        let diagram = crate::parse(include_str!("../../../assets/example.mermaid")).unwrap();
        let DiagramKind::Graph(graph) = &diagram.kind else {
            unreachable!()
        };
        for direction in [Direction::Tb, Direction::Bt, Direction::Lr, Direction::Rl] {
            let options = RenderOptions {
                direction: Some(direction),
                ..RenderOptions::default()
            };
            let result = layout(graph, &options);
            let (edge, turns) = result
                .routes
                .iter()
                .enumerate()
                .map(|(i, route)| (i, bends(route)))
                .max_by_key(|&(_, turns)| turns)
                .unwrap();
            assert!(
                turns <= 4,
                "{direction:?}: {} -> {} has {turns} bends",
                graph.nodes[graph.edges[edge].from].id,
                graph.nodes[graph.edges[edge].to].id
            );
            for (i, edge) in graph.edges.iter().enumerate() {
                if edge.label.is_empty() {
                    continue;
                }
                let (x, y) = result.label_anchors[i].unwrap();
                assert!(
                    result.routes[i].windows(2).any(|pair| {
                        pair[0].0.min(pair[1].0) <= x
                            && x <= pair[0].0.max(pair[1].0)
                            && pair[0].1.min(pair[1].1) <= y
                            && y <= pair[0].1.max(pair[1].1)
                    }),
                    "{direction:?}: a label left its connection"
                );
            }
        }
    }
}
