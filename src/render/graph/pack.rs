//! Compact routed features vertically while preserving their order in each column.

use super::{layout::Layout, *};

// A group moves by a common offset: boxes retain their shape, ports stay on
// their boxes, and every horizontal segment stays horizontal.
#[derive(Clone, Copy, Default)]
struct Feature {
    group: usize,
    left: usize,
    right: usize,
    top: usize,
    bottom: usize,
    edge: Option<usize>,
    label: bool,
    port: bool,
    node: bool,
}

impl Feature {
    fn new(group: usize, a: Point, b: Point) -> Self {
        Self {
            group,
            left: a.0.min(b.0),
            right: a.0.max(b.0),
            top: a.1.min(b.1),
            bottom: a.1.max(b.1),
            ..Self::default()
        }
    }
}

fn root(parents: &mut [usize], mut group: usize) -> usize {
    while parents[group] != group {
        parents[group] = parents[parents[group]];
        group = parents[group];
    }
    group
}

fn join(parents: &mut [usize], a: usize, b: usize) {
    let a = root(parents, a);
    let b = root(parents, b);
    parents[b] = a;
}

pub(super) fn vertical(
    layout: &mut Layout,
    graph: &Graph,
    options: &RenderOptions,
    ranks: &[usize],
) -> bool {
    if layout.width > 4096
        || layout.height > 4096
        || layout.width.saturating_mul(layout.height) > 1_000_000
        || layout.routes.iter().map(Vec::len).sum::<usize>() > 16_384
    {
        return false;
    }
    let mut parents: Vec<_> = (0..layout.nodes.len()).collect();
    let horizontal = options.direction.unwrap_or(graph.direction).horizontal();
    if !horizontal {
        // Preserve the common center of boxes in a layer, including boxes of
        // different heights. Only the space between layers should shrink.
        for node in 0..ranks.len() {
            if let Some(previous) = (0..node).find(|&i| ranks[i] == ranks[node]) {
                join(&mut parents, previous, node);
            }
        }
    }
    let mut features = Vec::new();
    for (i, node) in layout.nodes.iter().enumerate() {
        features.push(Feature {
            node: true,
            ..Feature::new(i, (node.x, node.y), (node.right(), node.bottom()))
        });
    }
    for (edge, &(_, start, end, _)) in graph.edges.iter().zip(&layout.ports) {
        for (group, point) in [(edge.from, start), (edge.to, end)] {
            features.push(Feature {
                port: true,
                ..Feature::new(group, point, point)
            });
        }
    }
    let mut paths = layout.routes.clone();
    let mut point_groups = Vec::new();
    let mut label_groups = vec![None; graph.edges.len()];
    for (i, path) in paths.iter_mut().enumerate() {
        if path.is_empty() {
            point_groups.push(Vec::new());
            continue;
        }
        let caption = layout.label_anchors[i]
            .filter(|_| options.edge_labels && !graph.edges[i].label.is_empty());
        if let Some(anchor) = caption {
            let Some(leg) = path.windows(2).position(|pair| {
                pair[0].0.min(pair[1].0) <= anchor.0
                    && anchor.0 <= pair[0].0.max(pair[1].0)
                    && pair[0].1.min(pair[1].1) <= anchor.1
                    && anchor.1 <= pair[0].1.max(pair[1].1)
            }) else {
                return false;
            };
            if path[leg] != anchor && path[leg + 1] != anchor {
                path.insert(leg + 1, anchor);
            }
        }
        let mut groups = vec![graph.edges[i].from];
        for _ in 1..path.len() - 1 {
            groups.push(parents.len());
            parents.push(parents.len());
        }
        groups.push(graph.edges[i].to);
        for (leg, pair) in path.windows(2).enumerate() {
            if pair[0].1 == pair[1].1 {
                join(&mut parents, groups[leg], groups[leg + 1]);
                features.push(Feature {
                    edge: Some(i),
                    ..Feature::new(groups[leg], pair[0], pair[1])
                });
            }
        }
        if let Some(anchor) = caption {
            let group = groups[path.iter().position(|&point| point == anchor).unwrap()];
            label_groups[i] = Some(group);
            let lines = wrap(&graph.edges[i].label, options.node_width, Emphasis::ITALIC);
            let width = lines.iter().map(label_width).max().unwrap_or(0);
            let (Some(left), Some(top)) = (
                anchor.0.checked_sub(width / 2 + 1),
                anchor.1.checked_sub(lines.len() / 2),
            ) else {
                return false;
            };
            features.push(Feature {
                edge: Some(i),
                label: true,
                ..Feature::new(
                    group,
                    (left, top),
                    (left + width + 1, top + lines.len() - 1),
                )
            });
        }
        point_groups.push(groups);
    }
    let mut columns = vec![Vec::new(); layout.width];
    let mut shifts = vec![usize::MAX; parents.len()];
    let mut work = 0;
    for (i, feature) in features.iter_mut().enumerate() {
        feature.group = root(&mut parents, feature.group);
        shifts[feature.group] = shifts[feature.group].min(feature.top.saturating_sub(1));
        work += feature.right - feature.left + 1;
        if feature.right >= columns.len() || work > 4_000_000 {
            return false;
        }
        for column in &mut columns[feature.left..=feature.right] {
            column.push(i);
        }
    }
    let mut constraints = vec![Vec::new(); parents.len()];
    if !horizontal {
        let mut bands: Vec<Option<Feature>> = vec![None; parents.len()];
        for feature in features.iter().filter(|feature| feature.node) {
            if let Some(band) = &mut bands[feature.group] {
                band.top = band.top.min(feature.top);
                band.bottom = band.bottom.max(feature.bottom);
            } else {
                bands[feature.group] = Some(*feature);
            }
        }
        let mut bands: Vec<_> = bands.into_iter().flatten().collect();
        bands.sort_unstable_by_key(|band| band.top);
        for pair in bands.windows(2) {
            let (upper, lower) = (pair[0], pair[1]);
            let Some(slack) = lower
                .top
                .checked_sub(upper.bottom + options.vertical_gap() + 1)
            else {
                return false;
            };
            constraints[upper.group].push((lower.group, slack));
        }
    }
    // Vertical stems connect features in their endpoint columns. Preserving
    // the order of all features in each column also preserves those crossings.
    for column in &mut columns {
        column.sort_unstable_by_key(|&i| (features[i].top, features[i].bottom, features[i].group));
        let mut previous: Option<Feature> = None;
        let mut previous_node: Option<Feature> = None;
        for &i in column.iter() {
            let current = features[i];
            // Routing features can sit between two boxes. Constrain the boxes
            // directly as well so packing never consumes their requested gap.
            if horizontal && current.node {
                if let Some(upper) = previous_node
                    && upper.group != current.group
                {
                    let Some(slack) = current
                        .top
                        .checked_sub(upper.bottom + options.vertical_gap() + 1)
                    else {
                        return false;
                    };
                    constraints[upper.group].push((current.group, slack));
                }
                previous_node = Some(current);
            }
            if let Some(upper) = &mut previous {
                if upper.group == current.group {
                    upper.bottom = upper.bottom.max(current.bottom);
                    upper.label |= current.label;
                    upper.port |= current.port;
                    upper.node |= current.node;
                    if upper.edge != current.edge {
                        upper.edge = None;
                    }
                    continue;
                }
                let label = upper.label || current.label;
                let separation = if label && (upper.port || current.port) {
                    2
                } else if label
                    && upper.edge.is_some()
                    && upper.edge == current.edge
                    && !upper.node
                    && !current.node
                {
                    // A label may cover a bend on its own connection.
                    0
                } else {
                    1
                };
                let Some(slack) = current.top.checked_sub(upper.bottom + separation) else {
                    // Existing overlaps cannot be separated by compaction.
                    return false;
                };
                constraints[upper.group].push((current.group, slack));
            }
            previous = Some(current);
        }
    }
    // Each constraint bounds how much farther the lower feature can move up
    // than the upper one. Nonnegative slack gives a shortest-path problem.
    let mut queue: BinaryHeap<_> = shifts
        .iter()
        .enumerate()
        .filter(|&(_, &shift)| shift != usize::MAX)
        .map(|(group, &shift)| Reverse((shift, group)))
        .collect();
    while let Some(Reverse((shift, group))) = queue.pop() {
        if shift != shifts[group] {
            continue;
        }
        for &(next, slack) in &constraints[group] {
            let candidate = shift + slack;
            if candidate < shifts[next] {
                shifts[next] = candidate;
                queue.push(Reverse((candidate, next)));
            }
        }
    }
    for (i, node) in layout.nodes.iter_mut().enumerate() {
        node.y -= shifts[root(&mut parents, i)];
    }
    for (edge, (source, start, end, target)) in graph.edges.iter().zip(&mut layout.ports) {
        let from = shifts[root(&mut parents, edge.from)];
        let to = shifts[root(&mut parents, edge.to)];
        source.1 -= from;
        start.1 -= from;
        end.1 -= to;
        target.1 -= to;
    }
    for (path, groups) in paths.iter_mut().zip(point_groups) {
        for (point, group) in path.iter_mut().zip(groups) {
            point.1 -= shifts[root(&mut parents, group)];
        }
    }
    for (anchor, group) in layout.label_anchors.iter_mut().zip(label_groups) {
        if let (Some(anchor), Some(group)) = (anchor, group) {
            anchor.1 -= shifts[root(&mut parents, group)];
        }
    }
    let old_bottom = features.iter().map(|feature| feature.bottom).max().unwrap();
    let new_bottom = features
        .iter()
        .map(|feature| feature.bottom - shifts[feature.group])
        .max()
        .unwrap();
    layout.height -= old_bottom - new_bottom;
    layout.routes = paths;
    true
}
