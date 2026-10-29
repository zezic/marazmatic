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
    assert!(text.contains("[ world ]-->"));
}

#[test]
fn color_and_style_can_be_controlled_independently() {
    let output = run(&["--color", "always", "--no-style"], SOURCE);
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("38;5;"));
    assert!(!text.contains(";1m"));
    let output = run(&["--color", "never", "--style", "always"], SOURCE);
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(!text.contains("38;5;"));
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
        vec!["--gap", "0"],
        vec!["--node-width", "255"],
        vec!["--padding", "4"],
    ] {
        assert!(!run(&args, SOURCE).status.success());
    }
}
