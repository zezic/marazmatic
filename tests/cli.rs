use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
};

fn run(args: &[&str], source: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_marazmatic"))
        .args(args)
        .env("TERM", "xterm-256color")
        .env_remove("NO_COLOR")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(source.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

const SOURCE: &str = "flowchart LR\nA[Hello] -->|world| B[Terminal]\n";

fn has_native_foreground(text: &str) -> bool {
    text.split("\x1b[").skip(1).any(|escape| {
        escape.split_once('m').is_some_and(|(codes, _)| {
            codes.split(';').any(|code| {
                code.parse::<u8>()
                    .is_ok_and(|code| matches!(code, 30..=37 | 39 | 90..=97))
            })
        })
    })
}

#[test]
fn zero_padding_removes_space_even_from_short_node_labels() {
    let output = run(&["--padding", "0", "--gap", "2"], "flowchart TB\nA[A]");
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("│A│"), "{text}");
}

#[test]
fn zero_padding_and_low_spacing_render_every_diagram_family() {
    for source in [
        "flowchart TB\nA[Alpha] --> B[Bravo]",
        "sequenceDiagram\nparticipant A as Alpha\nparticipant B as Bravo\nA->>B: request\nB->>B: first<br>last\nB-->>A: response",
        "stateDiagram-v2\nAlpha --> Bravo",
    ] {
        for args in [
            vec!["--padding", "0"],
            vec!["--padding", "0", "--gap", "0"],
            vec!["--padding", "0", "--gap", "1"],
            vec![
                "--padding",
                "0",
                "--horizontal-gap",
                "0",
                "--vertical-gap",
                "0",
            ],
        ] {
            let output = run(&args, source);
            assert!(
                output.status.success(),
                "{args:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let text = String::from_utf8(output.stdout).unwrap();
            assert!(
                text.contains("│Alpha│") && text.contains("│Bravo│"),
                "{args:?}: {text}"
            );
        }
    }
}

#[test]
fn piped_output_is_plain_by_default_and_stdin_can_be_explicit() {
    for args in [vec![], vec!["-"]] {
        let output = run(&args, SOURCE);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains("Hello"));
        assert!(text.contains('╭'));
        assert!(!text.contains('\x1b'));
    }
}

#[test]
fn plain_overrides_forced_color_and_accents_and_ascii_is_structural() {
    let output = run(
        &[
            "--plain", "--color", "always", "--style", "always", "--ascii",
        ],
        SOURCE,
    );
    assert!(output.status.success());
    assert!(output.stdout.is_ascii());
    assert!(!output.stdout.contains(&0x1b));
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("[ world ]"));
    assert!(text.contains("--+"));
}

#[test]
fn color_and_style_can_be_controlled_independently() {
    for theme in ["ansi", "mono"] {
        let output = run(
            &["--color", "always", "--no-style", "--theme", theme],
            SOURCE,
        );
        assert!(output.status.success());
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(
            has_native_foreground(&text),
            "{theme}: colored output must use the terminal's ANSI palette"
        );
        for extended in ["38;5;", "38;2;", "48;5;", "48;2;"] {
            assert!(
                !text.contains(extended),
                "{theme}: custom palette color {extended}"
            );
        }
        assert!(!text.contains(";1m"));
    }
    let output = run(&["--color", "never", "--style", "always"], SOURCE);
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(!has_native_foreground(&text));
    assert!(text.contains(";1m"));
}

#[test]
fn malformed_input_fails_with_line_number_without_partial_stdout() {
    let output = run(&[], "flowchart TB\nA -->\n");
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8(output.stderr).unwrap().contains("line 2"));
}

#[test]
fn live_requires_a_terminal_and_a_real_file() {
    for args in [
        vec!["live", "assets/flowchart.mmd"],
        vec!["--live", "assets/flowchart.mmd"],
        vec!["live", "-"],
    ] {
        let output = run(&args, "");
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        let text = String::from_utf8(output.stderr).unwrap();
        assert!(
            text.contains("interactive terminal") || text.contains("file to watch"),
            "{text}"
        );
    }
}

struct TempDir(PathBuf);
impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "marazmatic-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn exports_are_complete_and_never_overwrite_the_input() {
    let dir = TempDir::new();
    let source = dir.0.join("source.mmd");
    let output = dir.0.join("output.txt");
    fs::write(&source, SOURCE).unwrap();
    let result = run(
        &[source.to_str().unwrap(), "-o", output.to_str().unwrap()],
        "",
    );
    assert!(result.status.success());
    assert!(result.stdout.is_empty());
    let text = fs::read_to_string(&output).unwrap();
    assert!(text.contains("Hello"));
    assert!(!text.contains('\x1b'));
    let result = run(
        &[source.to_str().unwrap(), "-o", source.to_str().unwrap()],
        "",
    );
    assert!(!result.status.success());
    assert_eq!(fs::read_to_string(source).unwrap(), SOURCE);
}

#[test]
fn help_version_and_option_bounds_work() {
    assert!(run(&["--help"], "").status.success());
    assert!(
        String::from_utf8(run(&["--version"], "").stdout)
            .unwrap()
            .starts_with("marazmatic 0.1.0")
    );
    for args in [
        vec!["--gap", "13"],
        vec!["--horizontal-gap", "13"],
        vec!["--vertical-gap", "13"],
        vec!["--node-width", "255"],
        vec!["--padding", "4"],
        vec!["--theme", "terminal"],
        vec!["--theme", "ocean"],
        vec!["--theme", "ember"],
        vec!["--theme", "forest"],
    ] {
        assert!(!run(&args, SOURCE).status.success());
    }
}

#[test]
fn axis_spacing_flags_override_the_shared_gap() {
    let source = "flowchart TB\nA[Alpha] --> B[Bravo]\nA --> C[Charlie]\n";
    let base = run(&["--gap", "2"], source);
    let wide = run(&["--gap", "2", "--horizontal-gap", "12"], source);
    let tall = run(&["--gap", "2", "--vertical-gap", "12"], source);
    for output in [&base, &wide, &tall] {
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let text = |output: Output| String::from_utf8(output.stdout).unwrap();
    let base = text(base);
    let wide = text(wide);
    let tall = text(tall);
    assert!(
        wide.lines().map(|line| line.chars().count()).max()
            > base.lines().map(|line| line.chars().count()).max()
    );
    assert_eq!(wide.lines().count(), base.lines().count());
    assert!(tall.lines().count() > base.lines().count());
}
