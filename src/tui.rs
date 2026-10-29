use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
        KeyModifiers, MouseEventKind,
    },
    execute, queue,
    terminal::{
        self, Clear, ClearType, DisableLineWrap, EnableLineWrap, EnterAlternateScreen,
        LeaveAlternateScreen,
    },
};
use marazmatic::{
    Border, Diagram, Direction, RenderOptions,
    canvas::{Canvas, Emphasis, Role, wrap},
    parse, render,
};
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::PathBuf,
    time::{Duration, Instant},
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

struct TerminalGuard;

fn restore_terminal() {
    let _ = terminal::disable_raw_mode();
    let _ = execute!(
        io::stdout(),
        DisableMouseCapture,
        EnableLineWrap,
        Show,
        LeaveAlternateScreen
    );
}

impl TerminalGuard {
    fn enter() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        let guard = Self;
        execute!(
            io::stdout(),
            EnterAlternateScreen,
            Hide,
            DisableLineWrap,
            EnableMouseCapture
        )?;
        let original = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            restore_terminal();
            original(info);
        }));
        Ok(guard)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore_terminal();
    }
}

struct App {
    path: PathBuf,
    options: RenderOptions,
    initial: (RenderOptions, bool, bool),
    colors: bool,
    accents: bool,
    diagram: Option<Diagram>,
    canvas: Option<Canvas>,
    source: Option<String>,
    source_error: Option<String>,
    render_error: Option<String>,
    status: String,
    revision: usize,
    pan: (usize, usize),
    initial_view: bool,
    watch: bool,
    sidebar: bool,
    help: bool,
}

impl App {
    fn reload(&mut self, force: bool) -> bool {
        match super::read_source(&self.path) {
            Ok(source) => {
                if !force && self.source.as_ref() == Some(&source) && self.source_error.is_none() {
                    return false;
                }
                // A parse error is cached too: partial writes must not cause a
                // busy retry loop, and the last good diagram stays on screen.
                if !force
                    && self.source.as_ref() == Some(&source)
                    && self
                        .source_error
                        .as_ref()
                        .is_some_and(|e| e.starts_with("line "))
                {
                    return false;
                }
                self.source = Some(source.clone());
                match parse(&source) {
                    Ok(diagram) => {
                        self.source_error = None;
                        self.diagram = Some(diagram);
                        self.rebuild();
                        if self.render_error.is_none() {
                            self.revision += 1;
                            self.status = format!("File loaded / revision {}", self.revision);
                        }
                    }
                    Err(error) => self.source_error = Some(error.to_string()),
                }
            }
            Err(error) => {
                let message = format!("{}: {error}", self.path.display());
                if self.source_error.as_ref() == Some(&message) {
                    return false;
                }
                self.source_error = Some(message);
            }
        }
        true
    }

    fn rebuild(&mut self) {
        if let Some(diagram) = &self.diagram {
            match render(diagram, &self.options) {
                Ok(canvas) => {
                    self.canvas = Some(canvas);
                    self.render_error = None;
                }
                Err(error) => self.render_error = Some(error.to_string()),
            }
        }
    }

    fn export(&mut self) -> io::Result<()> {
        let Some(canvas) = &self.canvas else {
            self.status = "Nothing to export yet".into();
            return Ok(());
        };
        let stem = self.path.file_stem().unwrap_or_default().to_string_lossy();
        for suffix in 0..1000 {
            let name = if suffix == 0 {
                format!("{stem}.marazmatic.txt")
            } else {
                format!("{stem}.marazmatic-{suffix}.txt")
            };
            let path = self.path.with_file_name(name);
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut file) => {
                    if let Err(error) =
                        file.write_all(canvas.to_text(false, false, self.options.theme).as_bytes())
                    {
                        let _ = fs::remove_file(&path);
                        return Err(error);
                    }
                    self.status = format!("Exported {} (plain text)", path.display());
                    return Ok(());
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "too many existing exports",
        ))
    }

    fn key(&mut self, key: KeyEvent, viewport_height: usize) -> bool {
        if key.kind == KeyEventKind::Release {
            return false;
        }
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return true;
        }
        if self.help {
            match key.code {
                KeyCode::Char('q') => return true,
                KeyCode::Esc | KeyCode::Char('?') | KeyCode::Enter => self.help = false,
                _ => {}
            }
            return false;
        }
        let mut rebuild = false;
        let step = if key.modifiers.contains(KeyModifiers::SHIFT) {
            16
        } else {
            4
        };
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => return true,
            KeyCode::Char('?') => self.help = true,
            KeyCode::Tab => self.sidebar = !self.sidebar,
            KeyCode::Char('c') => self.colors = !self.colors,
            KeyCode::Char('s') => self.accents = !self.accents,
            KeyCode::Char('t') => self.options.theme = self.options.theme.next(),
            KeyCode::Char('b') => {
                self.options.border = self.options.border.next();
                rebuild = true;
            }
            KeyCode::Char('a') => {
                self.options.border = if self.options.border == Border::Ascii {
                    Border::Rounded
                } else {
                    Border::Ascii
                };
                rebuild = true;
            }
            KeyCode::Char('d') => {
                if self
                    .diagram
                    .as_ref()
                    .is_some_and(|d| d.name() == "sequence diagram")
                {
                    self.status = "Sequence diagrams preserve participant and time order".into();
                } else {
                    self.options.direction = match self.options.direction {
                        None => Some(Direction::Tb),
                        Some(Direction::Tb) => Some(Direction::Lr),
                        Some(Direction::Lr) => Some(Direction::Bt),
                        Some(Direction::Bt) => Some(Direction::Rl),
                        Some(Direction::Rl) => None,
                    };
                    rebuild = true;
                }
            }
            KeyCode::Char('+') | KeyCode::Char('=') => {
                self.options.gap = (self.options.gap + 1).min(12);
                rebuild = true;
            }
            KeyCode::Char('-') => {
                self.options.gap = self.options.gap.saturating_sub(1).max(2);
                rebuild = true;
            }
            KeyCode::Char(']') => {
                self.options.node_width = (self.options.node_width + 2).min(80);
                rebuild = true;
            }
            KeyCode::Char('[') => {
                self.options.node_width = self.options.node_width.saturating_sub(2).max(8);
                rebuild = true;
            }
            KeyCode::Char('p') => {
                self.options.padding = (self.options.padding + 1) % 4;
                rebuild = true;
            }
            KeyCode::Char('v') => {
                self.options.edge_labels = !self.options.edge_labels;
                rebuild = true;
            }
            KeyCode::Char('w') => {
                self.watch = !self.watch;
                self.status = if self.watch {
                    "Watching file"
                } else {
                    "File watching paused / r to reload"
                }
                .into();
            }
            KeyCode::Char('r') => {
                self.reload(true);
            }
            KeyCode::Char('e') => {
                if let Err(error) = self.export() {
                    self.status = format!("Export failed: {error}");
                }
            }
            KeyCode::Left | KeyCode::Char('h') => self.pan.0 = self.pan.0.saturating_sub(step),
            KeyCode::Right | KeyCode::Char('l') => self.pan.0 = self.pan.0.saturating_add(step),
            KeyCode::Up | KeyCode::Char('k') => self.pan.1 = self.pan.1.saturating_sub(step),
            KeyCode::Down | KeyCode::Char('j') => self.pan.1 = self.pan.1.saturating_add(step),
            KeyCode::PageUp => {
                self.pan.1 = self.pan.1.saturating_sub(viewport_height.saturating_sub(2))
            }
            KeyCode::PageDown => {
                self.pan.1 = self.pan.1.saturating_add(viewport_height.saturating_sub(2))
            }
            KeyCode::Home | KeyCode::Char('g') => self.pan = (0, 0),
            KeyCode::End | KeyCode::Char('G') => {
                self.pan.1 = self.canvas.as_ref().map_or(0, Canvas::height)
            }
            KeyCode::Char('0') => {
                self.options = self.initial.0.clone();
                self.colors = self.initial.1;
                self.accents = self.initial.2;
                self.pan = (0, 0);
                self.status = "Restored startup settings".into();
                rebuild = true;
            }
            _ => {}
        }
        if rebuild {
            self.rebuild();
        }
        false
    }

    fn draw(&mut self, columns: usize, rows: usize) -> Canvas {
        let mut screen = Canvas::new(columns, rows, self.options.border);
        if columns < 46 || rows < 14 {
            screen.text(1, 1, "MARAZMATIC", Role::Accent, Emphasis::BOLD);
            screen.text(
                1,
                3,
                "Resize to at least 46 x 14. q quits.",
                Role::Text,
                Emphasis::default(),
            );
            return screen;
        }
        screen.text(2, 0, "M / MARAZMATIC", Role::Accent, Emphasis::BOLD);
        screen.text(
            columns - 20,
            0,
            if self.watch {
                "LIVE / WATCHING"
            } else {
                "LIVE / PAUSED"
            },
            Role::Accent,
            Emphasis::default(),
        );
        let file = clean(&self.path.display().to_string());
        screen.text(
            2,
            1,
            &truncate(&file, columns - 4),
            Role::Muted,
            Emphasis::default(),
        );
        let sidebar = self.sidebar && columns >= 104;
        let panel_width = if sidebar { 31 } else { 0 };
        let main_width = columns - panel_width;
        let frame_height = rows - 7;
        let viewport = (main_width - 2, frame_height - 2);
        screen.rect(
            0,
            3,
            main_width,
            frame_height,
            Role::Muted,
            self.options.border,
        );
        let caption = format!(
            " {} ",
            self.diagram.as_ref().map_or("Diagram", Diagram::name)
        );
        screen.blank_rect(2, 3, caption.width(), 1);
        screen.text(2, 3, &caption, Role::Accent, Emphasis::default());
        if let Some(canvas) = &self.canvas {
            if self.initial_view {
                if let Some(Diagram {
                    kind: marazmatic::model::DiagramKind::Graph(graph),
                    ..
                }) = &self.diagram
                {
                    if self
                        .options
                        .direction
                        .unwrap_or(graph.direction)
                        .horizontal()
                    {
                        self.pan.1 = canvas.height().saturating_sub(viewport.1) / 2;
                    } else {
                        self.pan.0 = canvas.width().saturating_sub(viewport.0) / 2;
                    }
                }
                self.initial_view = false;
            }
            self.pan.0 = self.pan.0.min(canvas.width().saturating_sub(viewport.0));
            self.pan.1 = self.pan.1.min(canvas.height().saturating_sub(viewport.1));
            screen.viewport((1, 4), canvas, self.pan, viewport);
        } else {
            screen.text(
                3,
                6,
                "Waiting for a valid diagram",
                Role::Error,
                Emphasis::BOLD,
            );
            let message = if self.watch {
                "Edit the Mermaid file and save to update."
            } else {
                "Press r to load the file again."
            };
            for (row, line) in wrap(message, main_width - 7, Emphasis::default())
                .iter()
                .enumerate()
            {
                screen.text_line(3, 8 + row, line, Role::Muted);
            }
        }
        if sidebar {
            let x = main_width + 1;
            screen.rect(
                x,
                3,
                panel_width - 1,
                frame_height,
                Role::Muted,
                self.options.border,
            );
            screen.blank_rect(x + 2, 3, 12, 1);
            screen.text(x + 2, 3, " CONTROLS ", Role::Accent, Emphasis::BOLD);
            let direction = self.options.direction.map_or("auto", Direction::name);
            let lines = [
                format!("b    Border      {}", self.options.border.name()),
                format!("t    Theme       {}", self.options.theme.name()),
                format!("c    Colors      {}", on_off(self.colors)),
                format!("s    Accents     {}", on_off(self.accents)),
                format!("d    Direction   {direction}"),
                format!("+/-  Spacing     {}", self.options.gap),
                format!("[/]  Label width {}", self.options.node_width),
                format!("p    Padding     {}", self.options.padding),
                format!("v    Labels      {}", on_off(self.options.edge_labels)),
                String::new(),
                "arrows / hjkl    Pan".into(),
                "PgUp / PgDn      Scroll".into(),
                "g    Back to origin".into(),
                "r    Reload file".into(),
                "e    Export plain text".into(),
                "0    Reset settings".into(),
                "?    Help   Tab panel".into(),
                "q    Quit".into(),
            ];
            for (row, line) in lines
                .iter()
                .enumerate()
                .take(frame_height.saturating_sub(3))
            {
                screen.text(x + 2, 5 + row, line, Role::Text, Emphasis::default());
            }
        }
        let error = self.source_error.as_ref().or(self.render_error.as_ref());
        let warning = self.diagram.as_ref().and_then(|d| d.warnings.first());
        let (status, role) = if let Some(error) = error {
            (format!("ERROR / {error}"), Role::Error)
        } else if let Some(warning) = warning {
            (format!("NOTE / {warning}"), Role::LabelDecoration)
        } else {
            (self.status.clone(), Role::Accent)
        };
        screen.text(
            1,
            rows - 3,
            &truncate(&clean(&status), columns - 2),
            role,
            Emphasis::default(),
        );
        let hints = "b borders  t theme  c color  s style  d direction  +/- gap  ? help  q quit";
        screen.text(
            1,
            rows - 2,
            &truncate(hints, columns - 2),
            Role::Muted,
            Emphasis::default(),
        );
        if let Some(canvas) = &self.canvas {
            let counts = self.diagram.as_ref().map_or((0, 0), Diagram::counts);
            let label = if self
                .diagram
                .as_ref()
                .is_some_and(|d| d.name() == "sequence diagram")
            {
                "participants / messages"
            } else {
                "nodes / edges"
            };
            let footer = format!(
                "{} / {} {label}  |  {} x {} cells  |  x {}  y {}{}",
                counts.0,
                counts.1,
                canvas.width(),
                canvas.height(),
                self.pan.0,
                self.pan.1,
                if error.is_some() {
                    "  |  showing last good render"
                } else {
                    ""
                }
            );
            screen.text(
                1,
                rows - 1,
                &truncate(&footer, columns - 2),
                Role::Muted,
                Emphasis::default(),
            );
        }
        if self.help {
            self.draw_help(&mut screen, columns, rows);
        }
        screen
    }

    fn draw_help(&self, screen: &mut Canvas, columns: usize, rows: usize) {
        let width = 66.min(columns - 4);
        let height = 24.min(rows - 2);
        let x = (columns - width) / 2;
        let y = (rows - height) / 2;
        screen.blank_rect(x, y, width, height);
        screen.rect(x, y, width, height, Role::Accent, self.options.border);
        let lines = [
            "MARAZMATIC / live controls",
            "",
            "b       Cycle rounded / square / double / heavy / ASCII",
            "a       Toggle ASCII borders and arrows",
            "t       Cycle ocean / ember / forest / mono palette",
            "c / s   Toggle color / font style accents separately",
            "d       Cycle auto / TB / LR / BT / RL (graphs)",
            "+ / -   Increase / decrease node spacing",
            "[ / ]   Decrease / increase label wrapping width",
            "p / v   Cycle node padding / toggle edge labels",
            "",
            "arrows  Pan (also h j k l); Shift moves faster",
            "PgUp/Dn Scroll a page; g / G go to top / bottom",
            "Tab     Toggle the settings panel",
            "w / r   Toggle file watching / reload now",
            "e       Export entire diagram as plain text",
            "0       Restore startup settings and viewport",
            "q       Quit (also Ctrl+C)",
            "",
            "Save the source file to reload automatically.",
            "Invalid edits keep the last good render visible.",
            "? / Esc / Enter closes help",
        ];
        for (i, line) in lines.iter().take(height - 2).enumerate() {
            screen.text(
                x + 2,
                y + 1 + i,
                &truncate(line, width - 4),
                if i == 0 { Role::Accent } else { Role::Text },
                if i == 0 {
                    Emphasis::BOLD
                } else {
                    Emphasis::default()
                },
            );
        }
    }
}

fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}
fn clean(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
}
fn truncate(text: &str, width: usize) -> String {
    if text.width() <= width {
        return text.to_owned();
    }
    let mut result = String::new();
    let mut columns = 0;
    for glyph in text.graphemes(true) {
        if columns + glyph.width() > width.saturating_sub(2) {
            break;
        }
        result.push_str(glyph);
        columns += glyph.width();
    }
    result.push_str("..");
    result
}

pub fn run(path: PathBuf, options: RenderOptions, colors: bool, accents: bool) -> io::Result<()> {
    let _guard = TerminalGuard::enter()?;
    let mut app = App {
        path,
        initial: (options.clone(), colors, accents),
        options,
        colors,
        accents,
        diagram: None,
        canvas: None,
        source: None,
        source_error: None,
        render_error: None,
        status: "Watching file".into(),
        revision: 0,
        pan: (0, 0),
        initial_view: true,
        watch: true,
        sidebar: true,
        help: false,
    };
    app.reload(true);
    let mut dirty = true;
    let mut last_check = Instant::now();
    let mut stdout = io::stdout().lock();
    loop {
        let (columns, rows) = terminal::size()?;
        if dirty {
            let screen = app.draw(columns.into(), rows.into());
            for y in 0..rows {
                queue!(stdout, MoveTo(0, y), Clear(ClearType::CurrentLine))?;
                write!(
                    stdout,
                    "{}",
                    screen.row(
                        y.into(),
                        0,
                        columns.into(),
                        app.colors,
                        app.accents,
                        app.options.theme
                    )
                )?;
            }
            stdout.flush()?;
            dirty = false;
        }
        if event::poll(Duration::from_millis(80))? {
            match event::read()? {
                Event::Key(key) => {
                    if app.key(key, rows.saturating_sub(9).into()) {
                        break;
                    }
                    dirty = true;
                }
                Event::Resize(_, _) => dirty = true,
                Event::Mouse(mouse) => {
                    match mouse.kind {
                        MouseEventKind::ScrollDown => app.pan.1 = app.pan.1.saturating_add(3),
                        MouseEventKind::ScrollUp => app.pan.1 = app.pan.1.saturating_sub(3),
                        MouseEventKind::ScrollRight => app.pan.0 = app.pan.0.saturating_add(3),
                        MouseEventKind::ScrollLeft => app.pan.0 = app.pan.0.saturating_sub(3),
                        _ => {}
                    }
                    dirty = true;
                }
                _ => {}
            }
        }
        if app.watch && last_check.elapsed() >= Duration::from_millis(250) {
            dirty |= app.reload(false);
            last_check = Instant::now();
        }
    }
    Ok(())
}
