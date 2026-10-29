use marazmatic::{Border, Direction, RenderOptions, Theme, parse, render};
use unicode_width::UnicodeWidthStr;

fn plain(source: &str, options: &RenderOptions) -> String {
    render(&parse(source).unwrap(), options)
        .unwrap()
        .to_text(false, false, Theme::Ansi)
}

fn point_junctions(text: &str) -> usize {
    text.chars()
        .filter(|ch| matches!(ch, '┸' | '┰' | '┥' | '┝'))
        .count()
}

#[test]
fn graph_point_tips_do_not_take_an_extra_row() {
    let single = parse("flowchart TB\nA[Alpha]").unwrap();
    let connected = parse("flowchart TB\nA[Alpha] --> B[Bravo]").unwrap();
    for direction in [Direction::Tb, Direction::Bt] {
        for border in [Border::Rounded, Border::Ascii] {
            let options = RenderOptions {
                direction: Some(direction),
                border,
                gap: 0,
                padding: 0,
                ..RenderOptions::default()
            };
            let node = render(&single, &options).unwrap();
            let canvas = render(&connected, &options).unwrap();
            assert_eq!(
                canvas.height(),
                node.height() * 2,
                "{direction:?} {border:?}: connector tips must not reserve a row"
            );
        }
    }
}

#[test]
fn graph_circle_and_cross_endings_keep_their_distinct_markers() {
    let source = "flowchart TB\nA[ALPHA] o--o B[BRAVO]\nB x--x C[CHARLIE]";
    for (border, circle, cross) in [(Border::Rounded, '○', '×'), (Border::Ascii, 'o', 'x')] {
        let text = plain(
            source,
            &RenderOptions {
                border,
                gap: 0,
                padding: 0,
                ..RenderOptions::default()
            },
        );
        assert_eq!(
            text.chars().filter(|&ch| ch == circle).count(),
            2,
            "{border:?}: {text}"
        );
        assert_eq!(
            text.chars().filter(|&ch| ch == cross).count(),
            2,
            "{border:?}: {text}"
        );
    }
}

#[test]
fn graph_directions_match_border_junctions_and_node_order() {
    for (direction, junction) in [
        (Direction::Tb, '┸'),
        (Direction::Bt, '┰'),
        (Direction::Lr, '┥'),
        (Direction::Rl, '┝'),
    ] {
        let options = RenderOptions {
            direction: Some(direction),
            ..RenderOptions::default()
        };
        let text = plain("flowchart TB\nA[Alpha] --> B[Bravo]", &options);
        assert!(text.contains(junction), "{direction:?}: {text}");
        assert!(
            !text.contains(['╽', '╿', '╼', '╾']),
            "{direction:?}: extra connector tips"
        );
        let alpha = text.find("Alpha").unwrap();
        let bravo = text.find("Bravo").unwrap();
        assert_eq!(
            alpha < bravo,
            matches!(direction, Direction::Tb | Direction::Lr)
        );
    }
}

#[test]
fn connected_nodes_respect_gap_and_port_clearance() {
    let node = parse("flowchart TB\nA[Alpha]").unwrap();
    let connected = parse("flowchart TB\nA[Alpha] --> B[Bravo]").unwrap();
    for direction in [Direction::Tb, Direction::Bt, Direction::Lr, Direction::Rl] {
        for gap in [0, 1, 2, 3, 4, 12] {
            let options = RenderOptions {
                direction: Some(direction),
                horizontal_gap: Some(gap),
                vertical_gap: Some(gap),
                ..RenderOptions::default()
            };
            let node = render(&node, &options).unwrap();
            let canvas = render(&connected, &options).unwrap();
            let actual = if direction.horizontal() {
                canvas.width() - 2 * node.width()
            } else {
                canvas.height() - 2 * node.height()
            };
            let minimum = if !direction.horizontal() && gap == 0 {
                0
            } else {
                2
            };
            assert_eq!(
                actual,
                gap.max(minimum),
                "{direction:?}: extra spacing between connected nodes"
            );
        }
    }
}

#[test]
fn parallel_nodes_use_the_configured_border_gap() {
    let node = parse("flowchart TB\nA[Alpha]").unwrap();
    let parallel = parse("flowchart TB\nA[Alpha]\nB[Bravo]").unwrap();
    for direction in [Direction::Tb, Direction::Bt, Direction::Lr, Direction::Rl] {
        for gap in [0, 1, 2, 3, 4, 12] {
            let options = RenderOptions {
                direction: Some(direction),
                horizontal_gap: Some(gap),
                vertical_gap: Some(gap),
                ..RenderOptions::default()
            };
            let node = render(&node, &options).unwrap();
            let canvas = render(&parallel, &options).unwrap();
            let actual = if direction.horizontal() {
                canvas.height() - 2 * node.height()
            } else {
                canvas.width() - 2 * node.width()
            };
            assert_eq!(
                actual, gap,
                "{direction:?}: extra spacing between parallel nodes"
            );
        }
    }
}

#[test]
fn simple_labeled_connections_only_expand_to_fit_the_label_and_its_ports() {
    let node = parse("flowchart TB\nA[Alpha]").unwrap();
    let connected = parse("flowchart TB\nA[Alpha] -->|label| B[Bravo]").unwrap();
    for direction in [Direction::Tb, Direction::Bt, Direction::Lr, Direction::Rl] {
        for gap in [0, 1, 2, 4, 6, 12] {
            let options = RenderOptions {
                direction: Some(direction),
                gap,
                ..RenderOptions::default()
            };
            let node = render(&node, &options).unwrap();
            let canvas = render(&connected, &options).unwrap();
            let (actual, label_size) = if direction.horizontal() {
                (canvas.width() - 2 * node.width(), "🭮█label█🭬".width())
            } else {
                (canvas.height() - 2 * node.height(), 1)
            };
            let clearance = if !direction.horizontal() && gap == 0 {
                0
            } else {
                4
            };
            assert_eq!(
                actual,
                gap.max(label_size + clearance),
                "{direction:?}: unnecessary label clearance"
            );
        }
    }
}

#[test]
fn spacing_adjusts_screen_axes_independently_in_every_direction() {
    let diagram =
        parse("flowchart TB\nA[Alpha] --> B[Bravo]\nA --> C[Charlie]\nB --> D[Delta]\nC --> D")
            .unwrap();
    for direction in [Direction::Tb, Direction::Bt, Direction::Lr, Direction::Rl] {
        let options = RenderOptions {
            direction: Some(direction),
            gap: 4,
            ..RenderOptions::default()
        };
        let base = render(&diagram, &options).unwrap();
        let wide = render(
            &diagram,
            &RenderOptions {
                horizontal_gap: Some(5),
                ..options.clone()
            },
        )
        .unwrap();
        let tall = render(
            &diagram,
            &RenderOptions {
                vertical_gap: Some(5),
                ..options
            },
        )
        .unwrap();
        assert!(
            wide.width() > base.width(),
            "{direction:?}: horizontal spacing had no effect"
        );
        assert_eq!(
            wide.height(),
            base.height(),
            "{direction:?}: horizontal spacing changed height"
        );
        assert!(
            tall.height() > base.height(),
            "{direction:?}: vertical spacing had no effect"
        );
        assert_eq!(
            tall.width(),
            base.width(),
            "{direction:?}: vertical spacing changed width"
        );
    }
}

#[test]
fn sequence_spacing_separates_participants_and_messages_independently() {
    let diagram = parse(
        "sequenceDiagram\nparticipant A as Alpha\nparticipant B as Bravo\nA->>B: ping\nB->>A: pong",
    )
    .unwrap();
    let options = RenderOptions {
        gap: 4,
        ..RenderOptions::default()
    };
    let base = render(&diagram, &options).unwrap();
    let wide = render(
        &diagram,
        &RenderOptions {
            horizontal_gap: Some(5),
            ..options.clone()
        },
    )
    .unwrap();
    let tall = render(
        &diagram,
        &RenderOptions {
            vertical_gap: Some(5),
            ..options
        },
    )
    .unwrap();
    assert!(wide.width() > base.width());
    assert_eq!(wide.height(), base.height());
    assert!(tall.height() > base.height());
    assert_eq!(tall.width(), base.width());
}

#[test]
fn graph_labels_share_the_connector_center_in_every_direction() {
    for direction in [Direction::Tb, Direction::Bt, Direction::Lr, Direction::Rl] {
        let options = RenderOptions {
            direction: Some(direction),
            ..RenderOptions::default()
        };
        let text = plain("flowchart TB\nA[Alpha] -->|label| B[Bravo]", &options);
        let rows: Vec<_> = text.lines().collect();
        let y = rows
            .iter()
            .position(|row| row.contains("🭮█label█🭬"))
            .unwrap();
        if direction.horizontal() {
            assert!(
                rows[y].contains("Alpha") && rows[y].contains("Bravo"),
                "{text}"
            );
            let (left, right) = rows[y].split_once("🭮█label█🭬").unwrap();
            let left_run = left.chars().rev().take_while(|ch| *ch == '─').count();
            let right_run = right.chars().take_while(|ch| *ch == '─').count();
            assert!(left_run > 0 && right_run > 0, "{text}");
            assert!(left_run.abs_diff(right_run) <= 2, "{text}");
        } else {
            let alpha = rows.iter().find(|row| row.contains("Alpha")).unwrap();
            let connector_x = alpha[..alpha.find("Alpha").unwrap()].width() + 2;
            let label_x = rows[y][..rows[y].find("🭮█").unwrap()].width();
            assert_eq!(label_x + "🭮█label█🭬".width() / 2, connector_x, "{text}");
            for row in [rows[y - 1], rows[y + 1]] {
                assert_eq!(row.chars().nth(connector_x), Some('│'), "{text}");
            }
        }
    }
}

#[test]
fn every_border_supports_cycles_self_loops_and_disconnected_nodes() {
    let source =
        "flowchart LR\nA[Alpha] -->|forward| B[Bravo]\nB -.->|return| A\nB ==> B\nC[Charlie]";
    for border in [
        Border::Rounded,
        Border::Square,
        Border::Double,
        Border::Heavy,
        Border::Ascii,
    ] {
        let options = RenderOptions {
            border,
            ..RenderOptions::default()
        };
        let text = plain(source, &options);
        for label in ["Alpha", "Bravo", "Charlie", "forward", "return"] {
            assert!(text.contains(label), "missing {label}: {text}");
        }
        assert!(!text.contains('\x1b'));
        if border == Border::Ascii {
            assert!(text.is_ascii());
        }
        assert_eq!(text, plain(source, &options));
    }
}

#[test]
fn supplied_diagram_preserves_connections_with_few_crossings_in_every_direction() {
    let diagram = parse(include_str!("../assets/example.mermaid")).unwrap();
    for direction in [Direction::Tb, Direction::Bt, Direction::Lr, Direction::Rl] {
        let options = RenderOptions {
            direction: Some(direction),
            ..RenderOptions::default()
        };
        let canvas =
            render(&diagram, &options).unwrap_or_else(|error| panic!("{direction:?}: {error}"));
        let text = canvas.to_text(false, false, Theme::Ansi);
        for label in [
            "blocks-engine",
            "F: Entity",
            "F: Progress",
            "F: Analytics",
            "F: Uploader",
            "Supplier Gateway",
        ] {
            assert!(text.contains(label), "{direction:?}: missing {label}");
        }
        let crossings = text
            .chars()
            .filter(|ch| matches!(ch, '┼' | '╋' | '╬'))
            .count();
        assert!(
            crossings <= 4,
            "{direction:?}: {crossings} crossings\n{text}"
        );
        assert_eq!(
            point_junctions(&text),
            diagram.counts().1,
            "{direction:?}: a connection lost its border junction\n{text}"
        );
        assert!(text.lines().all(|line| line.width() <= canvas.width()));
    }
}

#[test]
fn zero_vertical_gap_compacts_the_supplied_graph_after_routing() {
    let diagram = parse(include_str!("../assets/example.mermaid")).unwrap();
    let marazmatic::model::DiagramKind::Graph(graph) = &diagram.kind else {
        unreachable!()
    };
    for direction in [Direction::Tb, Direction::Bt, Direction::Lr, Direction::Rl] {
        let options = RenderOptions {
            direction: Some(direction),
            gap: 0,
            padding: 0,
            ..RenderOptions::default()
        };
        let canvas = render(&diagram, &options).unwrap();
        let text = canvas.to_text(false, false, Theme::Mono);
        let redundant = text.lines().position(|line| {
            line.chars()
                .all(|ch| matches!(ch, ' ' | '│' | '┆' | '┃' | '┊' | '║' | '|'))
        });
        assert!(
            redundant.is_none(),
            "{direction:?}: unused connector row {redundant:?} in a {}-row graph",
            canvas.height()
        );
        for label in graph
            .nodes
            .iter()
            .map(|node| &node.label)
            .chain(graph.edges.iter().map(|edge| &edge.label))
        {
            for line in marazmatic::canvas::wrap(
                label,
                options.node_width,
                marazmatic::canvas::Emphasis::default(),
            ) {
                let label = line.text();
                assert!(text.contains(&label), "{direction:?}: missing {label}");
            }
        }
        assert_eq!(
            point_junctions(&text),
            diagram.counts().1,
            "{direction:?}: lost border junctions"
        );
    }
}

#[test]
fn wide_fanouts_and_fanins_keep_every_branch_separate() {
    let mut source = "flowchart TB\n".to_owned();
    for i in 0..12 {
        source.push_str(&format!(
            "Root[Root] -->|path {i}| N{i}[Branch {i}]\nN{i} --> Sink[Sink]\n"
        ));
    }
    for direction in [Direction::Tb, Direction::Bt, Direction::Lr, Direction::Rl] {
        let options = RenderOptions {
            direction: Some(direction),
            gap: 2,
            ..RenderOptions::default()
        };
        let text = plain(&source, &options);
        assert!(
            !text.chars().any(|ch| matches!(ch, '┼' | '╋' | '╬')),
            "{direction:?}: {text}"
        );
        assert_eq!(point_junctions(&text), 24, "{direction:?}: {text}");
        for i in 0..12 {
            assert!(
                text.contains(&format!("🭮█path {i}█🭬")),
                "{direction:?}: branch {i} lost its label\n{text}"
            );
        }
    }
}

#[test]
fn labeled_self_loops_leave_room_for_forward_and_feedback_labels() {
    let source =
        "flowchart TB\nA[Alpha] -->|forward| B[Bravo]\nB -.->|return| A\nB -->|self| B\nC[Charlie]";
    for gap in [2, 4] {
        for direction in [Direction::Tb, Direction::Bt, Direction::Lr, Direction::Rl] {
            let text = plain(
                source,
                &RenderOptions {
                    direction: Some(direction),
                    gap,
                    ..RenderOptions::default()
                },
            );
            for label in ["forward", "return", "self"] {
                assert!(
                    text.contains(&format!("🭮█{label}█🭬")),
                    "{direction:?}: {text}"
                );
            }
            assert_eq!(point_junctions(&text), 3, "{direction:?}: {text}");
        }
    }
}

#[test]
fn zero_padding_keeps_feedback_labels_visible() {
    let diagram = parse(include_str!("../assets/flowchart.mmd")).unwrap();
    for direction in [Direction::Tb, Direction::Bt, Direction::Lr, Direction::Rl] {
        for gap in [0, 1, 2] {
            let options = RenderOptions {
                direction: Some(direction),
                gap,
                padding: 0,
                ..RenderOptions::default()
            };
            let canvas = render(&diagram, &options)
                .unwrap_or_else(|error| panic!("{direction:?}, gap {gap}: {error}"));
            let text = canvas.to_text(false, false, Theme::Mono);
            assert!(text.contains("🭮█try again█🭬"), "{direction:?}: {text}");
            assert!(
                !text.contains(['╽', '╿', '╼', '╾']),
                "{direction:?}: {text}"
            );
            assert!(
                text.contains("Build it")
                    && text.contains("Tests pass?")
                    && text.contains("Ship it!"),
                "{direction:?}: {text}"
            );
        }
    }
}

#[test]
fn ansi_and_plain_share_identical_geometry() {
    let diagram =
        parse("flowchart LR\nA[\"**Bold** and *italic*\"] -->|`code`| B[\"你好 👩‍💻 é\"]").unwrap();
    let canvas = render(&diagram, &RenderOptions::default()).unwrap();
    let plain = canvas.to_text(false, false, Theme::Ansi);
    let styled = canvas.to_text(true, true, Theme::Ansi);
    assert_eq!(strip_ansi(&styled), plain);
    assert!(styled.contains(";1m"));
    assert!(styled.contains(";3m"));
    assert!(styled.contains(";4m"));
    assert!(!plain.contains("**"));
    assert!(plain.contains("你好 👩‍💻 é"));
    for line in plain.lines() {
        assert!(line.width() <= canvas.width());
    }
}

fn strip_ansi(text: &str) -> String {
    let mut chars = text.chars();
    let mut result = String::new();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            assert_eq!(chars.next(), Some('['));
            for ch in chars.by_ref() {
                if ch == 'm' {
                    break;
                }
            }
        } else {
            result.push(ch);
        }
    }
    result
}

#[test]
fn state_start_end_and_terminal_shape_approximations_survive_routing() {
    let text = plain(
        include_str!("../assets/state.mmd"),
        &RenderOptions::default(),
    );
    for label in [
        "●",
        "◉",
        "Draft",
        "In review",
        "Published",
        "submit",
        "revise",
        "approve",
    ] {
        assert!(text.contains(label), "missing {label}: {text}");
    }
    let text = plain(
        "flowchart TB\nA[(Database)] --> B[[Subroutine]]\nB --> C{{Prepare}}\nC --> D(((End)))",
        &RenderOptions::default(),
    );
    for label in ["Database", "Subroutine", "Prepare", "End"] {
        assert!(text.contains(label));
    }
}

#[test]
fn sequence_messages_notes_and_interaction_frames_remain_visible() {
    let text = plain(
        include_str!("../assets/sequence.mmd"),
        &RenderOptions::default(),
    );
    for label in [
        "User",
        "API",
        "Database",
        "1. Sign in",
        "Account details",
        "alt",
        "Unknown account",
        "Update session",
        "Ready for the next request",
    ] {
        assert!(text.contains(label), "missing {label}: {text}");
    }
    assert!(text.contains('┄'));
    assert!(text.contains('┃'));
    assert!(text.contains('╾'));
    assert!(text.contains('╼'));
}

#[test]
fn sequence_labels_share_arrow_rows_and_wrapped_self_calls_keep_their_return() {
    let text = plain(
        "sequenceDiagram\nA->>B: forward\nB-->>A: return\nA->>A: first<br>middle<br>last",
        &RenderOptions::default(),
    );
    let rows: Vec<_> = text.lines().collect();
    for (label, arrow) in [("forward", '╼'), ("return", '╾')] {
        let row = rows.iter().find(|row| row.contains(label)).unwrap();
        assert!(row.contains(arrow), "{text}");
    }
    let first = rows.iter().position(|row| row.contains("first")).unwrap();
    let middle = rows.iter().position(|row| row.contains("middle")).unwrap();
    let last = rows.iter().position(|row| row.contains("last")).unwrap();
    assert_eq!(middle, first + 1, "{text}");
    assert_eq!(last, middle + 1, "{text}");
    assert!(
        rows[middle].contains('├') && rows[middle].contains('╮'),
        "{text}"
    );
    assert!(
        rows[last + 2].contains('╾') && rows[last + 2].contains('╯'),
        "{text}"
    );
}

#[test]
fn single_participant_notes_and_long_self_calls_are_not_clipped() {
    let text = plain(
        "sequenceDiagram\nparticipant A as Agent\nNote left of A: leftmost note\nNote right of A: rightmost note\nNote over A: over the lifeline\nA->>A: a rather long self message",
        &RenderOptions::default(),
    );
    for label in [
        "leftmost note",
        "rightmost note",
        "over the lifeline",
        "a rather long self message",
    ] {
        assert!(text.contains(label), "missing {label}: {text}");
    }
}

#[test]
fn deeply_nested_frames_and_notes_to_the_left_of_later_participants_fit() {
    let mut source = "sequenceDiagram\nparticipant A\nparticipant B\nNote left of B: a very long note on the left\n".to_owned();
    for _ in 0..16 {
        source.push_str("loop again\n");
    }
    source.push_str("A->>B: innermost message\n");
    for _ in 0..16 {
        source.push_str("end\n");
    }
    let canvas = render(&parse(&source).unwrap(), &RenderOptions::default()).unwrap();
    let text = canvas.to_text(false, false, Theme::Ansi);
    assert!(text.contains("innermost message"));
    assert!(text.contains("a very long note on the"));
    assert!(text.contains("left"));
    assert!(text.lines().all(|line| line.width() <= canvas.width()));
}

#[test]
fn hiding_labels_preserves_graph_connections_and_sequence_arrows() {
    let options = RenderOptions {
        edge_labels: false,
        ..RenderOptions::default()
    };
    let text = plain(
        "flowchart LR\nA[Alpha] -->|secret label| B[Bravo]",
        &options,
    );
    assert!(!text.contains("secret"));
    assert!(text.contains("Alpha"));
    assert!(text.contains('┥'));
    let text = plain("sequenceDiagram\nA->>B: secret message", &options);
    assert!(!text.contains("secret"));
    assert!(text.contains('╼'));
}

#[test]
fn invalid_library_inputs_and_extreme_canvases_return_errors() {
    for (horizontal_gap, vertical_gap) in [
        (Some(usize::MAX), None),
        (Some(13), None),
        (None, Some(usize::MAX)),
        (None, Some(13)),
    ] {
        assert!(
            render(
                &parse("flowchart TB\nA").unwrap(),
                &RenderOptions {
                    horizontal_gap,
                    vertical_gap,
                    ..RenderOptions::default()
                },
            )
            .is_err()
        );
    }
    assert!(
        render(
            &parse("flowchart TB\nA").unwrap(),
            &RenderOptions {
                node_width: 0,
                ..RenderOptions::default()
            }
        )
        .is_err()
    );
    let mut source = "flowchart LR\n".to_owned();
    for i in 0..255 {
        source.push_str(&format!("N{i}[{}] --> N{}\n", "x".repeat(80), i + 1));
    }
    let diagram = parse(&source).unwrap();
    assert!(
        render(
            &diagram,
            &RenderOptions {
                node_width: 80,
                ..RenderOptions::default()
            }
        )
        .unwrap_err()
        .to_string()
        .contains("too large")
    );
}
