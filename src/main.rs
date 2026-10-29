mod tui;

use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use marazmatic::{
    Border, Direction, RenderOptions, Theme, parse, parser::MAX_SOURCE_BYTES, render,
};
use std::{
    env, fs,
    io::{self, IsTerminal, Read, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
enum OutputMode {
    #[default]
    Auto,
    Always,
    Never,
}

#[derive(Parser, Debug)]
#[command(
    version,
    about = "Mermaid diagrams, beautifully rendered in your terminal",
    long_about = "Render Mermaid flowcharts, sequence diagrams, and state diagrams using terminal-native box drawing. Use `live FILE` to watch a file and tune rendering with hotkeys.",
    after_help = "Examples:\n  marazmatic diagram.mmd\n  marazmatic live diagram.mmd\n  marazmatic diagram.mmd --plain --border double\n  cat diagram.mmd | marazmatic --ascii\n  marazmatic diagram.mmd -o diagram.txt\n\nLive hotkeys: b borders, t theme, c colors, s styles, d direction, m center,\n              +/- spacing, x/X horizontal gap, y/Y vertical gap, [/] label width,\n              p padding, v labels, arrows pan, ? help, q quit."
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Mermaid file, or - for stdin. Reads stdin when omitted in a pipe.
    file: Option<PathBuf>,

    /// Open the interactive viewer (equivalent to `live FILE`).
    #[arg(short, long, global = true)]
    live: bool,

    /// Start the live viewer aligned to the top-left instead of centered.
    #[arg(long, global = true)]
    no_center: bool,

    /// Write the full diagram to a file.
    #[arg(short, long, global = true, value_name = "FILE")]
    output: Option<PathBuf>,

    /// Disable all ANSI colors and font styles; keep Unicode box drawing.
    #[arg(long, global = true)]
    plain: bool,

    /// Use only ASCII characters for borders, connectors, and arrows.
    #[arg(long, global = true)]
    ascii: bool,

    /// Terminal border style.
    #[arg(long, value_enum, default_value = "rounded", global = true)]
    border: Border,

    /// Color assignment using the terminal's ANSI palette.
    #[arg(long, value_enum, default_value = "ansi", global = true)]
    theme: Theme,

    /// ANSI color policy. Auto respects NO_COLOR, TERM=dumb, and redirection.
    #[arg(long, value_enum, default_value = "auto", global = true)]
    color: OutputMode,

    /// ANSI font style policy (bold, italic, underline).
    #[arg(long, value_enum, default_value = "auto", global = true)]
    style: OutputMode,

    /// Disable font styles while preserving the color policy.
    #[arg(long, global = true)]
    no_style: bool,

    /// Override graph direction; sequence diagrams preserve time order.
    #[arg(long, value_enum, global = true)]
    direction: Option<Direction>,

    /// Space between nodes and ranks.
    #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(u8).range(0..=12), global = true)]
    gap: u8,

    /// Horizontal space between nodes (defaults to --gap).
    #[arg(long, value_parser = clap::value_parser!(u8).range(0..=12), global = true)]
    horizontal_gap: Option<u8>,

    /// Vertical space between nodes (defaults to --gap).
    #[arg(long, value_parser = clap::value_parser!(u8).range(0..=12), global = true)]
    vertical_gap: Option<u8>,

    /// Wrap label text at this many terminal columns.
    #[arg(long, default_value_t = 26, value_parser = clap::value_parser!(u8).range(8..=80), global = true)]
    node_width: u8,

    /// Horizontal padding inside node boxes.
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u8).range(0..=3), global = true)]
    padding: u8,

    /// Hide edge and message labels.
    #[arg(long, global = true)]
    no_labels: bool,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Watch a Mermaid file and adjust rendering with live hotkeys.
    Live {
        /// Mermaid file to watch.
        file: PathBuf,
    },
}

fn policy(mode: OutputMode, automatic: bool) -> bool {
    match mode {
        OutputMode::Auto => automatic,
        OutputMode::Always => true,
        OutputMode::Never => false,
    }
}

fn read_source(path: &Path) -> io::Result<String> {
    let reader: Box<dyn Read> = if path == Path::new("-") {
        Box::new(io::stdin())
    } else {
        Box::new(fs::File::open(path)?)
    };
    let mut source = String::new();
    reader
        .take(MAX_SOURCE_BYTES as u64 + 1)
        .read_to_string(&mut source)?;
    if source.len() > MAX_SOURCE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "diagram exceeds the 1 MiB input limit",
        ));
    }
    Ok(source)
}

fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    let interactive = io::stdout().is_terminal();
    let capable = interactive && env::var("TERM").is_ok_and(|term| term != "dumb");
    let options = RenderOptions {
        border: if cli.ascii { Border::Ascii } else { cli.border },
        theme: cli.theme,
        direction: cli.direction,
        gap: cli.gap.into(),
        horizontal_gap: cli.horizontal_gap.map(usize::from),
        vertical_gap: cli.vertical_gap.map(usize::from),
        node_width: cli.node_width.into(),
        padding: cli.padding.into(),
        edge_labels: !cli.no_labels,
    };
    let color = !cli.plain
        && policy(
            cli.color,
            capable && cli.output.is_none() && env::var_os("NO_COLOR").is_none_or(|v| v.is_empty()),
        );
    let accents = !cli.plain && !cli.no_style && policy(cli.style, capable && cli.output.is_none());
    let live = cli.live || cli.command.is_some();
    if cli.command.is_some() && cli.file.is_some() {
        return Err("specify one input file, after `live`".into());
    }
    let file = match cli.command {
        Some(Command::Live { file }) => file,
        None => match cli.file {
            Some(file) => file,
            None if !io::stdin().is_terminal() && !live => PathBuf::from("-"),
            None if !live => {
                Cli::command().print_help()?;
                println!();
                return Ok(());
            }
            None => {
                return Err(
                    "live mode requires a Mermaid file: marazmatic live diagram.mmd".into(),
                );
            }
        },
    };
    if live {
        if file == Path::new("-") {
            return Err(
                "live mode needs a file to watch; stdin is supported for one-shot rendering".into(),
            );
        }
        if cli.output.is_some() {
            return Err(
                "--output is for one-shot rendering; press e in live mode to export".into(),
            );
        }
        if !interactive || !io::stdin().is_terminal() {
            return Err("live mode requires an interactive terminal on stdin and stdout".into());
        }
        if env::var("TERM").is_ok_and(|term| term == "dumb") {
            return Err(
                "live mode requires a cursor-addressable terminal (TERM cannot be dumb)".into(),
            );
        }
        tui::run(file, options, color, accents, !cli.no_center)?;
        return Ok(());
    }
    let source = read_source(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    let diagram = parse(&source).map_err(|e| format!("{}:{e}", file.display()))?;
    let canvas = render(&diagram, &options)?;
    for warning in &diagram.warnings {
        eprintln!("warning: {warning}");
    }
    let text = canvas.to_text(color, accents, options.theme);
    if let Some(output) = cli.output {
        if file != Path::new("-") && same_file(&file, &output) {
            return Err("output path is the input file; choose a different --output path".into());
        }
        fs::write(output, text)?;
    } else {
        io::stdout().lock().write_all(text.as_bytes())?;
    }
    Ok(())
}

fn same_file(a: &Path, b: &Path) -> bool {
    a == b || matches!((a.canonicalize(), b.canonicalize()), (Ok(a), Ok(b)) if a == b)
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            if error
                .downcast_ref::<io::Error>()
                .is_some_and(|e| e.kind() == io::ErrorKind::BrokenPipe)
            {
                return ExitCode::SUCCESS;
            }
            eprintln!("marazmatic: {error}");
            ExitCode::FAILURE
        }
    }
}
