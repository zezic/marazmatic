use super::*;

struct EventLayout {
    y: usize,
    lines: Vec<TextLine>,
}

pub(super) fn render(sequence: &Sequence, options: &RenderOptions) -> Result<Canvas, RenderError> {
    let count = sequence.participants.len();
    if count == 0 || count > 256 || sequence.events.len() > 4096 {
        return Err(RenderError(
            "invalid sequence diagram or diagram limits exceeded".into(),
        ));
    }
    for event in &sequence.events {
        let valid = match event {
            SequenceEvent::Message { from, to, .. } | SequenceEvent::Note { from, to, .. } => {
                *from < count && *to < count
            }
            SequenceEvent::Activate(i) | SequenceEvent::Deactivate(i) => *i < count,
            _ => true,
        };
        if !valid {
            return Err(RenderError(
                "sequence event references an unknown participant".into(),
            ));
        }
    }
    let mut boxes: Vec<_> = sequence
        .participants
        .iter()
        .map(|p| {
            let label = if p.actor {
                format!(
                    "{} {}",
                    if options.border == Border::Ascii {
                        "@"
                    } else {
                        "♙"
                    },
                    p.label
                )
            } else {
                p.label.clone()
            };
            box_node(&label, Shape::Rectangle, options)
        })
        .collect();
    let header_height = boxes.iter().map(|n| n.height).max().unwrap();
    let mut y = header_height + 3;
    let mut number = 0;
    let events: Vec<_> = sequence
        .events
        .iter()
        .map(|event| {
            let (text, extra) = match event {
                SequenceEvent::Message { text, from, to, .. } => {
                    number += 1;
                    let text = if !options.edge_labels {
                        String::new()
                    } else if sequence.autonumber {
                        format!("{number}. {text}")
                    } else {
                        text.clone()
                    };
                    (
                        text,
                        if from == to { 3 } else { 1 } + options.vertical_gap(),
                    )
                }
                SequenceEvent::Note { text, .. } => (text.clone(), 4),
                SequenceEvent::BlockStart { kind, text } => (format!("{kind}  {text}"), 2),
                SequenceEvent::BlockElse(text) => (text.clone(), 2),
                SequenceEvent::BlockEnd => (String::new(), 1),
                SequenceEvent::Activate(_) | SequenceEvent::Deactivate(_) => {
                    return EventLayout {
                        y,
                        lines: Vec::new(),
                    };
                }
            };
            let lines = if text.is_empty() {
                Vec::new()
            } else {
                wrap(&text, options.node_width, Emphasis::ITALIC)
            };
            let height = lines.len() + extra;
            let result = EventLayout { y, lines };
            y += height;
            result
        })
        .collect();
    let note_width =
        |event: &EventLayout| event.lines.iter().map(label_width).max().unwrap_or(0) + 4;
    let mut left_margin = 4;
    for (event, position) in sequence.events.iter().zip(&events) {
        if matches!(
            event,
            SequenceEvent::Note {
                from: 0,
                side: NoteSide::Left,
                ..
            }
        ) {
            left_margin = left_margin.max(note_width(position) + 4);
        }
        if matches!(
            event,
            SequenceEvent::Note {
                from: 0,
                side: NoteSide::Over,
                ..
            }
        ) {
            left_margin = left_margin.max(note_width(position) / 2 + 2);
        }
    }
    let mut centers = vec![left_margin + boxes[0].width / 2];
    for i in 1..count {
        centers.push(
            centers[i - 1]
                + boxes[i - 1].width.div_ceil(2)
                + boxes[i].width / 2
                + options.horizontal_gap()
                + 4,
        );
    }
    for (event, position) in sequence.events.iter().zip(&events) {
        let (from, to, needed) = match event {
            SequenceEvent::Message { from, to, .. } if from != to => {
                (*from.min(to), *from.max(to), note_width(position) + 2)
            }
            _ => continue,
        };
        let distance = centers[to] - centers[from];
        if distance < needed {
            for center in &mut centers[to..] {
                *center += needed - distance;
            }
        }
    }
    let mut shift = 0;
    for (event, position) in sequence.events.iter().zip(&events) {
        if let SequenceEvent::Note { from, to, side, .. } = event {
            let left = centers[*from].min(centers[*to]);
            let needed = match side {
                NoteSide::Left => note_width(position) + 4,
                NoteSide::Over => note_width(position) / 2 + 2,
                NoteSide::Right => 0,
            };
            shift = shift.max(needed.saturating_sub(left));
        }
    }
    for center in &mut centers {
        *center += shift;
    }
    let mut width = centers[count - 1] + boxes[count - 1].width.div_ceil(2) + 4;
    let mut block_depth: usize = 0;
    for (event, position) in sequence.events.iter().zip(&events) {
        match event {
            SequenceEvent::Message { from, to, .. } if from == to => {
                width = width.max(centers[*from] + note_width(position) + 6)
            }
            SequenceEvent::Note { from, to, side, .. } => {
                let right = centers[*from].max(centers[*to]);
                width = width.max(match side {
                    NoteSide::Right => right + note_width(position) + 5,
                    NoteSide::Over => right + note_width(position) / 2 + 4,
                    NoteSide::Left => right + 4,
                });
            }
            SequenceEvent::BlockStart { .. } => {
                width = width.max(note_width(position) + 12 + block_depth * 2);
                block_depth += 1;
            }
            SequenceEvent::BlockElse(_) => {
                width = width.max(note_width(position) + 12 + block_depth * 2)
            }
            SequenceEvent::BlockEnd => block_depth = block_depth.saturating_sub(1),
            _ => {}
        }
    }
    let end_y = y + 1;
    let mut canvas = checked_canvas(width, end_y + header_height + 2, options.border)?;
    for center in &centers {
        canvas.line(
            (*center, header_height + 1),
            (*center, end_y),
            Role::Muted,
            Stroke::Dashed,
        );
    }
    // Draw nested interaction frames behind all event text and messages.
    let mut frames: Vec<(usize, usize)> = Vec::new();
    for (i, (event, position)) in sequence.events.iter().zip(&events).enumerate() {
        match event {
            SequenceEvent::BlockStart { .. } => frames.push((i, frames.len())),
            SequenceEvent::BlockElse(_) => {
                let depth = frames.last().map_or(0, |(_, depth)| *depth);
                canvas.line(
                    (1 + depth, position.y),
                    (width - 2 - depth, position.y),
                    Role::Muted,
                    Stroke::Dashed,
                );
            }
            SequenceEvent::BlockEnd => {
                if let Some((start, depth)) = frames.pop() {
                    canvas.rect(
                        1 + depth,
                        events[start].y,
                        width - 2 - depth * 2,
                        position.y - events[start].y + 1,
                        Role::Muted,
                        options.border,
                    );
                }
            }
            _ => {}
        }
    }
    // Activations are ranges rather than glyph substitutions, so nested calls
    // remain active until all outstanding activations have been closed.
    let mut active = vec![Vec::new(); count];
    for (event, position) in sequence.events.iter().zip(&events) {
        let mut begin = None;
        let mut finish = None;
        match event {
            SequenceEvent::Activate(i) => begin = Some((*i, position.y)),
            SequenceEvent::Deactivate(i) => finish = Some((*i, position.y)),
            SequenceEvent::Message {
                from,
                to,
                activate,
                deactivate,
                ..
            } => {
                let row = position.y + position.lines.len() / 2;
                if *activate {
                    begin = Some((*to, row));
                }
                if *deactivate {
                    finish = Some((*from, row));
                }
            }
            _ => {}
        }
        if let Some((i, row)) = begin {
            active[i].push(row);
        }
        if let Some((i, row)) = finish
            && let Some(start) = active[i].pop()
        {
            canvas.line(
                (centers[i], start),
                (centers[i], row),
                Role::Node(i),
                Stroke::Thick,
            );
        }
    }
    for (i, starts) in active.iter().enumerate() {
        for start in starts {
            canvas.line(
                (centers[i], *start),
                (centers[i], end_y),
                Role::Node(i),
                Stroke::Thick,
            );
        }
    }
    let mut depth: usize = 0;
    for (event, position) in sequence.events.iter().zip(&events) {
        match event {
            SequenceEvent::Message {
                from,
                to,
                stroke,
                start,
                end,
                ..
            } => {
                let x1 = centers[*from];
                let x2 = centers[*to];
                let row = position.y + position.lines.len() / 2;
                if from == to {
                    let right = x1 + note_width(position) + 1;
                    let return_row = position.y + position.lines.len().max(1) + 1;
                    canvas.line((x1, row), (right, row), Role::Edge, *stroke);
                    canvas.line((right, row), (right, return_row), Role::Edge, *stroke);
                    canvas.line(
                        (x1 + 1, return_row),
                        (right, return_row),
                        Role::Edge,
                        *stroke,
                    );
                    arrow(&mut canvas, (x1 + 1, return_row), (-1, 0), *end, options);
                    arrow(&mut canvas, (x1 + 1, row), (-1, 0), *start, options);
                    for (i, line) in position.lines.iter().enumerate() {
                        draw_label(
                            &mut canvas,
                            x1 + (right - x1 - label_width(line)) / 2,
                            position.y + i,
                            line,
                        );
                    }
                } else {
                    let direction = if x2 > x1 { 1 } else { -1 };
                    let finish = (x2 as isize - direction) as usize;
                    let begin = (x1 as isize + direction) as usize;
                    canvas.line((begin, row), (finish, row), Role::Edge, *stroke);
                    arrow(&mut canvas, (finish, row), (direction, 0), *end, options);
                    arrow(&mut canvas, (begin, row), (-direction, 0), *start, options);
                    for (i, line) in position.lines.iter().enumerate() {
                        draw_label(
                            &mut canvas,
                            x1.min(x2) + (x1.abs_diff(x2) - label_width(line)) / 2,
                            position.y + i,
                            line,
                        );
                    }
                }
            }
            SequenceEvent::Note { from, to, side, .. } => {
                let left = centers[*from].min(centers[*to]);
                let right = centers[*from].max(centers[*to]);
                let note_width = note_width(position);
                let (x, width) = match side {
                    NoteSide::Left => (left.saturating_sub(note_width + 2), note_width),
                    NoteSide::Right => (right + 2, note_width),
                    NoteSide::Over => {
                        let width = note_width.max(right - left + 3);
                        ((left + right).saturating_sub(width) / 2, width)
                    }
                };
                canvas.blank_rect(x, position.y, width, position.lines.len() + 2);
                canvas.rect(
                    x,
                    position.y,
                    width,
                    position.lines.len() + 2,
                    Role::LabelDecoration,
                    options.border,
                );
                for (i, line) in position.lines.iter().enumerate() {
                    draw_label(&mut canvas, x + 2, position.y + i + 1, line);
                }
            }
            SequenceEvent::BlockStart { .. } => {
                for (i, line) in position.lines.iter().enumerate() {
                    canvas.blank_rect(3 + depth, position.y + i, line.width() + 2, 1);
                    canvas.text_line(4 + depth, position.y + i, line, Role::Accent);
                }
                depth += 1;
            }
            SequenceEvent::BlockElse(_) => {
                for (i, line) in position.lines.iter().enumerate() {
                    canvas.blank_rect(3 + depth, position.y + i, line.width() + 2, 1);
                    canvas.text_line(4 + depth, position.y + i, line, Role::Accent);
                }
            }
            SequenceEvent::BlockEnd => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    for (i, node) in boxes.iter_mut().enumerate() {
        node.x = centers[i] - node.width / 2;
        node.y = header_height + 1 - node.height;
        draw_node(&mut canvas, node, i, options);
        canvas.attach(
            (node.cx(), node.bottom()),
            (node.cx(), node.bottom() + 1),
            false,
        );
        node.y = end_y;
        draw_node(&mut canvas, node, i, options);
        canvas.attach((node.cx(), node.y), (node.cx(), node.y - 1), false);
    }
    Ok(canvas)
}
