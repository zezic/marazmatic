use marazmatic::{Border, Direction, RenderOptions, Theme, parse, render};
use unicode_width::UnicodeWidthStr;

fn plain(source: &str, options: &RenderOptions) -> String {
    render(&parse(source).unwrap(), options)
        .unwrap()
        .to_text(false, false, Theme::Ocean)
}

#[test]
fn graph_directions_match_arrowheads_and_node_order() {
    for (direction, arrow) in [
        (Direction::Tb, '╽'),
        (Direction::Bt, '╿'),
        (Direction::Lr, '╼'),
        (Direction::Rl, '╾'),
    ] {
        let options = RenderOptions {
            direction: Some(direction),
            ..RenderOptions::default()
        };
        let text = plain("flowchart TB\nA[Alpha] --> B[Bravo]", &options);
        assert!(text.contains(arrow), "{direction:?}: {text}");
        let alpha = text.find("Alpha").unwrap();
        let bravo = text.find("Bravo").unwrap();
        assert_eq!(
            alpha < bravo,
            matches!(direction, Direction::Tb | Direction::Lr)
        );
    }
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
        let text = canvas.to_text(false, false, Theme::Ocean);
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
        let arrows = text
            .chars()
            .filter(|ch| matches!(ch, '╿' | '╽' | '╾' | '╼'))
            .count();
        assert_eq!(
            arrows,
            diagram.counts().1,
            "{direction:?}: a connection lost its arrowhead\n{text}"
        );
        assert!(text.lines().all(|line| line.width() <= canvas.width()));
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
        assert_eq!(
            text.chars()
                .filter(|ch| matches!(ch, '╿' | '╽' | '╾' | '╼'))
                .count(),
            24,
            "{direction:?}: {text}"
        );
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
            assert_eq!(
                text.chars()
                    .filter(|ch| matches!(ch, '╿' | '╽' | '╾' | '╼'))
                    .count(),
                3,
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
    let plain = canvas.to_text(false, false, Theme::Ocean);
    let styled = canvas.to_text(true, true, Theme::Ocean);
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
    let text = canvas.to_text(false, false, Theme::Ocean);
    assert!(text.contains("innermost message"));
    assert!(text.contains("a very long note on the"));
    assert!(text.contains("left"));
    assert!(text.lines().all(|line| line.width() <= canvas.width()));
}

#[test]
fn hiding_labels_preserves_nodes_and_arrows() {
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
    assert!(text.contains('╼'));
    let text = plain("sequenceDiagram\nA->>B: secret message", &options);
    assert!(!text.contains("secret"));
    assert!(text.contains('╼'));
}

#[test]
fn invalid_library_inputs_and_extreme_canvases_return_errors() {
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
