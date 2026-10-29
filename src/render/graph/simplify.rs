//! Refine whole routes after layer ordering has established their topology.
//! Virtual vertices guide placement; they do not require a bend at every rank.

use super::{layout::Layout, *};

pub(super) fn compact(points: &[Point]) -> Vec<Point> {
    let mut result: Vec<Point> = Vec::new();
    for &point in points {
        if result.last() == Some(&point) {
            continue;
        }
        while result.len() > 1 {
            let a = result[result.len() - 2];
            let b = result[result.len() - 1];
            if !((a.0 == b.0 && b.0 == point.0) || (a.1 == b.1 && b.1 == point.1)) {
                break;
            }
            result.pop();
        }
        result.push(point);
    }
    result
}

fn cells(a: Point, b: Point) -> impl Iterator<Item = Point> {
    let distance = a.0.abs_diff(b.0) + a.1.abs_diff(b.1);
    let dx = (b.0 as isize - a.0 as isize).signum();
    let dy = (b.1 as isize - a.1 as isize).signum();
    (0..=distance).map(move |i| {
        (
            a.0.saturating_add_signed(dx * i as isize),
            a.1.saturating_add_signed(dy * i as isize),
        )
    })
}

struct Projection {
    direction: Direction,
    width: usize,
    height: usize,
}

impl Projection {
    fn forward(&self, (x, y): Point) -> Point {
        match self.direction {
            Direction::Tb => (x, y),
            Direction::Bt => (x, self.height - y - 1),
            Direction::Lr => (y, x),
            Direction::Rl => (y, self.width - x - 1),
        }
    }

    fn backward(&self, (x, y): Point) -> Point {
        match self.direction {
            Direction::Tb => (x, y),
            Direction::Bt => (x, self.height - y - 1),
            Direction::Lr => (y, x),
            Direction::Rl => (self.width - y - 1, x),
        }
    }
}

#[derive(Clone, Copy)]
struct Caption {
    anchor: Point,
    width: usize,
    height: usize,
    top: usize,
}

impl Caption {
    fn area(&self, at: Point) -> Option<(usize, usize, usize, usize)> {
        let left = at.0.checked_sub(self.width / 2 + 1)?;
        let top = at.1.checked_sub(self.top + 1)?;
        Some((left, top, left + self.width + 2, top + self.height + 2))
    }
}

struct Scene {
    width: usize,
    height: usize,
    boxes: Vec<(Point, Point)>,
    ends: Vec<(Point, Point)>,
    captions: Vec<Option<Caption>>,
    paths: Vec<Vec<Point>>,
}

impl Scene {
    fn tracks(&self, except: Option<usize>) -> Vec<[usize; 2]> {
        let mut tracks = vec![[0, 0]; self.width * self.height];
        for (i, path) in self.paths.iter().enumerate() {
            if except == Some(i) {
                continue;
            }
            for pair in path.windows(2) {
                let axis = usize::from(pair[0].0 == pair[1].0);
                for (x, y) in cells(pair[0], pair[1]) {
                    tracks[y * self.width + x][axis] += 1;
                }
            }
        }
        tracks
    }

    fn crossings(&self) -> usize {
        self.tracks(None)
            .iter()
            .map(|axes| axes[0] * axes[1])
            .sum::<usize>()
            .saturating_sub(
                self.paths
                    .iter()
                    .map(|path| path.len().saturating_sub(2))
                    .sum(),
            )
    }
}

// Prefix sums make segment clearance and crossing costs independent of length.
// A crossing stays possible; overlapping another route or turning on it does not.
struct Grid {
    width: usize,
    height: usize,
    horizontal_blocked: Vec<usize>,
    horizontal_crossings: Vec<usize>,
    vertical_blocked: Vec<usize>,
    vertical_crossings: Vec<usize>,
    occupied: Vec<usize>,
}

impl Grid {
    fn new(scene: &Scene, own: usize) -> Self {
        let (width, height) = (scene.width, scene.height);
        let mut blocked = vec![false; width * height];
        for &(a, b) in &scene.boxes {
            for y in a.1..=b.1 {
                for x in a.0..=b.0 {
                    blocked[y * width + x] = true;
                }
            }
        }
        for (i, caption) in scene.captions.iter().enumerate() {
            if i == own {
                continue;
            }
            if let Some((left, top, right, bottom)) = caption.and_then(|c| c.area(c.anchor)) {
                for y in top..bottom.min(height) {
                    for x in left..right.min(width) {
                        blocked[y * width + x] = true;
                    }
                }
            }
        }
        for (i, &(a, b)) in scene.ends.iter().enumerate() {
            if i != own {
                blocked[a.1 * width + a.0] = true;
                blocked[b.1 * width + b.0] = true;
            }
        }
        let tracks = scene.tracks(Some(own));
        let mut result = Self {
            width,
            height,
            horizontal_blocked: vec![0; height * (width + 1)],
            horizontal_crossings: vec![0; height * (width + 1)],
            vertical_blocked: vec![0; (height + 1) * width],
            vertical_crossings: vec![0; (height + 1) * width],
            occupied: vec![0; (height + 1) * (width + 1)],
        };
        for y in 0..height {
            let mut occupied_row = 0;
            for x in 0..width {
                let i = y * width + x;
                let h = y * (width + 1) + x;
                let [horizontal, vertical] = tracks[i];
                result.horizontal_blocked[h + 1] =
                    result.horizontal_blocked[h] + usize::from(blocked[i] || horizontal != 0);
                result.horizontal_crossings[h + 1] = result.horizontal_crossings[h] + vertical;
                result.vertical_blocked[i + width] =
                    result.vertical_blocked[i] + usize::from(blocked[i] || vertical != 0);
                result.vertical_crossings[i + width] = result.vertical_crossings[i] + horizontal;
                occupied_row += usize::from(
                    blocked[i]
                        || horizontal != 0
                        || vertical != 0
                        || scene.ends[own].0 == (x, y)
                        || scene.ends[own].1 == (x, y),
                );
                result.occupied[h + width + 2] = result.occupied[h + 1] + occupied_row;
            }
        }
        result
    }

    fn horizontal(&self, y: usize, a: usize, b: usize) -> Option<usize> {
        let offset = y * (self.width + 1);
        let (lo, hi) = (offset + a.min(b), offset + a.max(b) + 1);
        (self.horizontal_blocked[hi] == self.horizontal_blocked[lo])
            .then_some(self.horizontal_crossings[hi] - self.horizontal_crossings[lo])
    }

    fn vertical(&self, x: usize, a: usize, b: usize) -> Option<usize> {
        let (lo, hi) = (a.min(b) * self.width + x, (a.max(b) + 1) * self.width + x);
        (self.vertical_blocked[hi] == self.vertical_blocked[lo])
            .then_some(self.vertical_crossings[hi] - self.vertical_crossings[lo])
    }

    fn score(&self, path: &[Point]) -> (usize, usize, usize) {
        let mut crossings = 0;
        let mut length = 0;
        for pair in path.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            if a.1 == b.1 {
                let offset = a.1 * (self.width + 1);
                crossings += self.horizontal_crossings[offset + a.0.max(b.0) + 1]
                    - self.horizontal_crossings[offset + a.0.min(b.0)];
            } else {
                crossings += self.vertical_crossings[(a.1.max(b.1) + 1) * self.width + a.0]
                    - self.vertical_crossings[a.1.min(b.1) * self.width + a.0];
            }
            length += a.0.abs_diff(b.0) + a.1.abs_diff(b.1);
        }
        (crossings, path.len().saturating_sub(2), length)
    }

    fn caption(&self, path: &[Point], caption: Caption) -> Option<Point> {
        let mut best = None;
        for pair in path.windows(2) {
            for point in cells(pair[0], pair[1]) {
                let Some((left, top, right, bottom)) = caption.area(point) else {
                    continue;
                };
                if left < 1 || top < 1 || right >= self.width || bottom >= self.height {
                    continue;
                }
                let stride = self.width + 1;
                let count = self.occupied[bottom * stride + right]
                    + self.occupied[top * stride + left]
                    - self.occupied[top * stride + right]
                    - self.occupied[bottom * stride + left];
                if count == 0 {
                    let score = (
                        point.0.abs_diff(caption.anchor.0) + point.1.abs_diff(caption.anchor.1),
                        point,
                    );
                    if best.is_none_or(|previous| score < previous) {
                        best = Some(score);
                    }
                }
            }
        }
        best.map(|(_, point)| point)
    }

    fn refine(
        &self,
        original: &[Point],
        caption: Option<Caption>,
        extra_crossings: usize,
    ) -> (Vec<Point>, Option<Point>) {
        let (start, end) = (original[0], original[original.len() - 1]);
        let old_score = self.score(original);
        let mut best = original.to_vec();
        let mut best_caption = None;
        let mut best_score = if extra_crossings > 0 {
            (usize::MAX, old_score.1, old_score.2)
        } else {
            old_score
        };
        for x in 1..self.width - 1 {
            let mut prefix: Option<(i64, usize)> = None;
            for y in start.1 + 1..end.1 {
                if self.vertical(x, y, y).is_none() {
                    prefix = None;
                    continue;
                }
                let cumulative = self.vertical_crossings[(y + 1) * self.width + x] as i64;
                if let (Some(a), Some(b)) = (
                    self.vertical(start.0, start.1, y),
                    self.horizontal(y, start.0, x),
                ) {
                    let cost = (a + b) as i64 - cumulative;
                    if prefix.is_none_or(|(previous, _)| cost < previous) {
                        prefix = Some((cost, y));
                    }
                }
                let (Some((cost, entry)), Some(a), Some(b)) = (
                    prefix,
                    self.horizontal(y, x, end.0),
                    self.vertical(end.0, y, end.1),
                ) else {
                    continue;
                };
                let crossings = (cost + cumulative) as usize + a + b;
                if crossings > old_score.0 + extra_crossings {
                    continue;
                }
                let candidate =
                    compact(&[start, (start.0, entry), (x, entry), (x, y), (end.0, y), end]);
                let score = (
                    crossings,
                    candidate.len().saturating_sub(2),
                    end.1 - start.1 + start.0.abs_diff(x) + x.abs_diff(end.0),
                );
                if score >= best_score || score.1 > old_score.1 {
                    continue;
                }
                let position = caption.and_then(|c| self.caption(&candidate, c));
                if caption.is_some() && position.is_none() {
                    continue;
                }
                best = candidate;
                best_caption = position;
                best_score = score;
            }
        }
        (best, best_caption)
    }
}

pub(super) fn routes(layout: &mut Layout, graph: &Graph, options: &RenderOptions) {
    let area = layout.width.saturating_mul(layout.height);
    if layout.width > 4096 || layout.height > 4096 || area > 1_000_000 {
        return;
    }
    let direction = options.direction.unwrap_or(graph.direction);
    let projection = Projection {
        direction,
        width: layout.width,
        height: layout.height,
    };
    let (width, height) = if direction.horizontal() {
        (layout.height, layout.width)
    } else {
        (layout.width, layout.height)
    };
    let reversed = matches!(direction, Direction::Bt | Direction::Rl);
    let mut scene = Scene {
        width,
        height,
        boxes: layout
            .nodes
            .iter()
            .map(|node| {
                let a = projection.forward((node.x, node.y));
                let b = projection.forward((node.right(), node.bottom()));
                ((a.0.min(b.0), a.1.min(b.1)), (a.0.max(b.0), a.1.max(b.1)))
            })
            .collect(),
        ends: layout
            .ports
            .iter()
            .map(|&(_, a, b, _)| (projection.forward(a), projection.forward(b)))
            .collect(),
        captions: graph
            .edges
            .iter()
            .zip(&layout.label_anchors)
            .map(|(edge, &anchor)| {
                if !options.edge_labels || edge.label.is_empty() {
                    return None;
                }
                let anchor = projection.forward(anchor?);
                let lines = wrap(&edge.label, options.node_width, Emphasis::ITALIC);
                let width = lines.iter().map(label_width).max().unwrap_or(0);
                let (width, height) = if direction.horizontal() {
                    (lines.len(), width)
                } else {
                    (width, lines.len())
                };
                Some(Caption {
                    anchor,
                    width,
                    height,
                    top: if reversed {
                        (height - 1) / 2
                    } else {
                        height / 2
                    },
                })
            })
            .collect(),
        paths: layout
            .routes
            .iter()
            .map(|path| {
                compact(
                    &path
                        .iter()
                        .map(|&p| projection.forward(p))
                        .collect::<Vec<_>>(),
                )
            })
            .collect(),
    };
    let initial_crossings = scene.crossings();
    let mut crossings = initial_crossings;
    // Bound refinement work for diagrams near the canvas limits.
    let mut remaining_cells = 16_000_000usize;
    for pass in 0..5 {
        let mut order: Vec<_> = (0..scene.paths.len()).collect();
        order.sort_by_key(|&i| Reverse(scene.paths[i].len()));
        let mut changed = false;
        for own in order {
            let path = &scene.paths[own];
            if path.len() < 6 || path[0].1 >= path[path.len() - 1].1 {
                continue;
            }
            let Some(remaining) = remaining_cells.checked_sub(area) else {
                break;
            };
            remaining_cells = remaining;
            let extra = if pass == 2 && path.len() > 6 {
                (initial_crossings + 2).saturating_sub(crossings).min(2)
            } else {
                0
            };
            let grid = Grid::new(&scene, own);
            let old_crossings = grid.score(path).0;
            let (candidate, caption) = grid.refine(path, scene.captions[own], extra);
            if candidate != *path {
                crossings = crossings + grid.score(&candidate).0 - old_crossings;
                scene.paths[own] = candidate;
                if let (Some(label), Some(point)) = (&mut scene.captions[own], caption) {
                    label.anchor = point;
                }
                changed = true;
            }
        }
        if !changed && pass > 2 {
            break;
        }
    }
    for (i, path) in scene.paths.iter().enumerate() {
        layout.routes[i] = path.iter().map(|&p| projection.backward(p)).collect();
        if let Some(caption) = scene.captions[i] {
            layout.label_anchors[i] = Some(projection.backward(caption.anchor));
        }
    }
}
