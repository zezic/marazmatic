use std::{error::Error, fmt};

use crate::model::*;

pub const MAX_SOURCE_BYTES: usize = 1_048_576;
const MAX_NODES: usize = 256;
const MAX_EDGES: usize = 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    pub line: usize,
    pub message: String,
}

impl ParseError {
    fn new(line: usize, message: impl Into<String>) -> Self {
        Self {
            line,
            message: message.into(),
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl Error for ParseError {}

/// Parse one Mermaid diagram, optionally enclosed in a Markdown mermaid fence.
/// Limits keep accidental huge inputs from exhausting an interactive terminal.
pub fn parse(source: &str) -> Result<Diagram, ParseError> {
    if source.len() > MAX_SOURCE_BYTES {
        return Err(ParseError::new(1, "diagram exceeds the 1 MiB input limit"));
    }
    let (lines, title, mut warnings) = source_lines(source)?;
    let first = lines
        .iter()
        .find(|(_, l)| !l.trim().is_empty() && !l.trim().starts_with("%%"))
        .ok_or_else(|| {
            ParseError::new(
                1,
                "empty diagram; expected flowchart, sequenceDiagram, or stateDiagram-v2",
            )
        })?;
    let sequence = first.1.trim() == "sequenceDiagram";
    let statements = statements(
        &lines,
        !sequence && !first.1.trim().starts_with("stateDiagram"),
    )?;
    let (line, header) = &statements[0];
    let mut header_parts = header.split_whitespace();
    let keyword = header_parts.next().unwrap_or_default();
    let kind = match keyword {
        "flowchart" | "graph" => {
            let direction = parse_direction(header_parts.next().unwrap_or("TB"), *line)?;
            if header_parts.next().is_some() {
                return Err(ParseError::new(
                    *line,
                    "unexpected text after diagram direction",
                ));
            }
            DiagramKind::Graph(parse_flowchart(&statements[1..], direction, &mut warnings)?)
        }
        "sequenceDiagram" if header == "sequenceDiagram" => {
            DiagramKind::Sequence(parse_sequence(&statements[1..])?)
        }
        "stateDiagram" | "stateDiagram-v2" if header_parts.next().is_none() => {
            DiagramKind::Graph(parse_state(&statements[1..], &mut warnings)?)
        }
        _ => {
            return Err(ParseError::new(
                *line,
                format!(
                    "unsupported diagram type {keyword:?}; supported: flowchart, graph, sequenceDiagram, stateDiagram-v2"
                ),
            ));
        }
    };
    let diagram = Diagram {
        kind,
        title,
        warnings,
    };
    let (nodes, edges) = diagram.counts();
    if nodes == 0 {
        return Err(ParseError::new(
            *line,
            "diagram contains no nodes or participants",
        ));
    }
    if nodes > MAX_NODES || (!sequence && edges > MAX_EDGES) {
        return Err(ParseError::new(
            *line,
            "diagram limit exceeded (256 nodes / 1024 edges)",
        ));
    }
    Ok(diagram)
}

type SourceLines = Vec<(usize, String)>;

fn source_lines(source: &str) -> Result<(SourceLines, Option<String>, Vec<String>), ParseError> {
    let all: SourceLines = source
        .trim_start_matches('\u{feff}')
        .lines()
        .enumerate()
        .map(|(i, s)| (i + 1, s.trim_end_matches('\r').to_owned()))
        .collect();
    let mut lines = if let Some(start) = all
        .iter()
        .position(|(_, s)| s.trim() == "```mermaid" || s.trim() == "~~~mermaid")
    {
        let marker = if all[start].1.trim().starts_with('`') {
            "```"
        } else {
            "~~~"
        };
        let end = all[start + 1..]
            .iter()
            .position(|(_, s)| s.trim() == marker)
            .map(|i| i + start + 1)
            .ok_or_else(|| ParseError::new(all[start].0, "unclosed Mermaid code fence"))?;
        all[start + 1..end].to_vec()
    } else {
        all
    };
    let mut title = None;
    let mut warnings = Vec::new();
    if let Some(start) = lines.iter().position(|(_, s)| !s.trim().is_empty())
        && lines[start].1.trim() == "---"
    {
        let end = lines[start + 1..]
            .iter()
            .position(|(_, s)| s.trim() == "---")
            .map(|i| i + start + 1)
            .ok_or_else(|| ParseError::new(lines[start].0, "unclosed YAML front matter"))?;
        for (_, s) in &lines[start + 1..end] {
            if let Some(t) = s.trim().strip_prefix("title:") {
                title = Some(unquote(t.trim()).to_owned());
            }
        }
        if lines[start + 1..end]
            .iter()
            .any(|(_, s)| s.trim().starts_with("config:"))
        {
            warnings.push(
                "Mermaid front-matter config is ignored; use terminal rendering options".into(),
            );
        }
        lines.drain(..=end);
    }
    Ok((lines, title, warnings))
}

fn statements(lines: &[(usize, String)], delimiters: bool) -> Result<SourceLines, ParseError> {
    let mut result = Vec::new();
    let mut text = String::new();
    let mut start = 1;
    let mut stack = Vec::new();
    let mut quote = false;
    let mut escaped = false;
    for (line, s) in lines {
        let mut chars = s.chars().peekable();
        while let Some(ch) = chars.next() {
            if !quote && stack.is_empty() && ch == '%' && chars.peek() == Some(&'%') {
                break;
            }
            if text.trim().is_empty() && !ch.is_whitespace() {
                start = *line;
            }
            if ch == '"' && !escaped {
                quote = !quote;
            }
            if delimiters && !quote {
                match ch {
                    '[' => stack.push(']'),
                    '(' => stack.push(')'),
                    '{' => stack.push('}'),
                    ']' | ')' | '}' => {
                        if stack.pop() != Some(ch) {
                            return Err(ParseError::new(
                                *line,
                                format!("unexpected closing delimiter {ch}"),
                            ));
                        }
                    }
                    ';' if stack.is_empty() => {
                        if !text.trim().is_empty() {
                            result.push((start, text.trim().to_owned()));
                        }
                        text.clear();
                        continue;
                    }
                    _ => {}
                }
            }
            text.push(ch);
            escaped = ch == '\\' && !escaped;
        }
        if stack.is_empty() && (!quote || !delimiters) {
            if !text.trim().is_empty() {
                result.push((start, text.trim().to_owned()));
            }
            text.clear();
            quote = false;
        } else {
            text.push('\n');
        }
    }
    if !stack.is_empty() || quote {
        return Err(ParseError::new(
            start,
            "unclosed node shape or quoted label",
        ));
    }
    Ok(result)
}

fn parse_direction(value: &str, line: usize) -> Result<Direction, ParseError> {
    match value {
        "TB" | "TD" => Ok(Direction::Tb),
        "BT" => Ok(Direction::Bt),
        "LR" => Ok(Direction::Lr),
        "RL" => Ok(Direction::Rl),
        _ => Err(ParseError::new(
            line,
            format!("invalid direction {value:?}; expected TB, TD, BT, LR, or RL"),
        )),
    }
}

fn presentation_directive(s: &str, line: usize, warnings: &mut Vec<String>) -> bool {
    if ["classDef ", "class ", "style ", "linkStyle ", "click "]
        .iter()
        .any(|p| s.starts_with(p))
    {
        warnings.push(format!(
            "line {line}: Mermaid CSS / click directive ignored; use terminal themes"
        ));
        true
    } else {
        false
    }
}

fn parse_flowchart(
    lines: &[(usize, String)],
    direction: Direction,
    warnings: &mut Vec<String>,
) -> Result<Graph, ParseError> {
    let mut graph = Graph {
        direction,
        ..Graph::default()
    };
    for (line, s) in lines {
        if presentation_directive(s, *line, warnings) {
            continue;
        }
        if s.starts_with("subgraph ") || s == "end" {
            return Err(ParseError::new(
                *line,
                "subgraphs are not supported yet; render the nodes and edges as a flat flowchart",
            ));
        }
        if let Some(value) = s.strip_prefix("direction ") {
            graph.direction = parse_direction(value.trim(), *line)?;
            continue;
        }
        let mut cursor = Cursor {
            rest: s,
            line: *line,
        };
        let mut from = cursor.nodes(&mut graph)?;
        while !cursor.rest.trim().is_empty() {
            let (stroke, start, end, label) = cursor.edge()?;
            let to = cursor.nodes(&mut graph)?;
            for a in &from {
                for b in &to {
                    graph.edges.push(Edge {
                        from: *a,
                        to: *b,
                        stroke,
                        start,
                        end,
                        label: label.clone(),
                    });
                    if graph.edges.len() > MAX_EDGES {
                        return Err(ParseError::new(*line, "too many edges (limit: 1024)"));
                    }
                }
            }
            from = to;
        }
        if graph.nodes.len() > MAX_NODES {
            return Err(ParseError::new(*line, "too many nodes (limit: 256)"));
        }
        if s.contains(":::") && !warnings.iter().any(|w| w.contains("class annotations")) {
            warnings.push("Mermaid class annotations ignored; use terminal themes".into());
        }
    }
    Ok(graph)
}

struct Cursor<'a> {
    rest: &'a str,
    line: usize,
}

impl<'a> Cursor<'a> {
    fn error(&self, message: impl Into<String>) -> ParseError {
        ParseError::new(self.line, message)
    }
    fn trim(&mut self) {
        self.rest = self.rest.trim_start();
    }
    fn nodes(&mut self, graph: &mut Graph) -> Result<Vec<usize>, ParseError> {
        let mut nodes = vec![self.node(graph)?];
        loop {
            self.trim();
            if let Some(tail) = self.rest.strip_prefix('&') {
                self.rest = tail;
                nodes.push(self.node(graph)?);
            } else {
                break;
            }
        }
        Ok(nodes)
    }

    fn node(&mut self, graph: &mut Graph) -> Result<usize, ParseError> {
        self.trim();
        let mut end = 0;
        for (i, ch) in self.rest.char_indices() {
            if ch.is_alphanumeric()
                || ch == '_'
                || ch == '.'
                || (ch == '-'
                    && !self.rest[i..].starts_with("--")
                    && !self.rest[i..].starts_with("-.")
                    && !self.rest[i..].starts_with("->"))
            {
                end = i + ch.len_utf8();
            } else {
                break;
            }
        }
        if end == 0 {
            return Err(self.error(format!(
                "expected a node identifier near {:?}",
                self.rest.chars().take(32).collect::<String>()
            )));
        }
        let id = &self.rest[..end];
        self.rest = &self.rest[end..];
        let index = graph.node(id);
        self.trim();
        let shapes = [
            ("(((", ")))", Shape::DoubleCircle),
            ("((", "))", Shape::Circle),
            ("([", "])", Shape::Stadium),
            ("[(", ")]", Shape::Cylinder),
            ("[[", "]]", Shape::Subroutine),
            ("{{", "}}", Shape::Hexagon),
            ("[", "]", Shape::Rectangle),
            ("(", ")", Shape::Rounded),
            ("{", "}", Shape::Decision),
        ];
        for (open, close, shape) in shapes {
            if let Some(tail) = self.rest.strip_prefix(open) {
                let mut quote = false;
                let mut escaped = false;
                let close_at = tail
                    .char_indices()
                    .find_map(|(i, ch)| {
                        if ch == '"' && !escaped {
                            quote = !quote;
                        }
                        let found = !quote && tail[i..].starts_with(close);
                        escaped = ch == '\\' && !escaped;
                        found.then_some(i)
                    })
                    .ok_or_else(|| self.error(format!("unclosed shape for node {id}")))?;
                let label = tail[..close_at].trim();
                if label.starts_with('/')
                    || label.starts_with('\\')
                    || label.ends_with('/')
                    || label.ends_with('\\')
                {
                    return Err(
                        self.error("parallelogram and trapezoid shapes are not supported yet")
                    );
                }
                graph.nodes[index].label = label_text(label);
                graph.nodes[index].shape = shape;
                self.rest = &tail[close_at + close.len()..];
                break;
            }
        }
        if let Some(tail) = self.rest.strip_prefix(":::") {
            let end = tail
                .find(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
                .unwrap_or(tail.len());
            if end == 0 {
                return Err(self.error("expected a class name after :::"));
            }
            self.rest = &tail[end..];
        }
        Ok(index)
    }

    fn edge(&mut self) -> Result<(Stroke, Arrow, Arrow, String), ParseError> {
        self.trim();
        let mut label = String::new();
        for (open, close, stroke) in [
            ("-- ", "-->", Stroke::Solid),
            ("-. ", ".->", Stroke::Dashed),
            ("== ", "==>", Stroke::Thick),
        ] {
            if let Some(tail) = self.rest.strip_prefix(open) {
                let at = tail
                    .find(close)
                    .ok_or_else(|| self.error(format!("expected {close} after edge label")))?;
                label = label_text(&tail[..at]);
                self.rest = &tail[at + close.len()..];
                return Ok((stroke, Arrow::None, Arrow::Point, label));
            }
        }
        let start = match self.rest.chars().next() {
            Some('<') => Arrow::Point,
            Some('o') => Arrow::Circle,
            Some('x') => Arrow::Cross,
            _ => Arrow::None,
        };
        if start != Arrow::None {
            self.rest = &self.rest[1..];
        }
        let marker = self.rest.chars().next().unwrap_or(' ');
        if marker != '-' && marker != '=' {
            return Err(self.error("expected an edge (-->, ---, -.->, or ==>)"));
        }
        let end = self
            .rest
            .find(|ch: char| ch != '-' && ch != '.' && ch != '=')
            .unwrap_or(self.rest.len());
        let shaft = &self.rest[..end];
        if shaft.len() < 2 || (shaft.contains('.') && !shaft.starts_with("-.")) {
            return Err(self.error("invalid edge; use -->, ---, -.->, or ==>"));
        }
        self.rest = &self.rest[end..];
        let arrow = match self.rest.chars().next() {
            Some('>') => Arrow::Point,
            Some('o') => Arrow::Circle,
            Some('x') => Arrow::Cross,
            _ => Arrow::None,
        };
        if arrow != Arrow::None {
            self.rest = &self.rest[1..];
        }
        if arrow == Arrow::None && shaft.len() < 3 {
            return Err(self.error("an undirected edge requires at least three dashes"));
        }
        let stroke = if marker == '=' {
            Stroke::Thick
        } else if shaft.contains('.') {
            Stroke::Dashed
        } else {
            Stroke::Solid
        };
        self.trim();
        if let Some(tail) = self.rest.strip_prefix('|') {
            let mut quote = false;
            let mut escaped = false;
            let at = tail
                .char_indices()
                .find_map(|(i, ch)| {
                    if ch == '"' && !escaped {
                        quote = !quote;
                    }
                    let found = ch == '|' && !quote;
                    escaped = ch == '\\' && !escaped;
                    found.then_some(i)
                })
                .ok_or_else(|| self.error("unclosed edge label; expected |"))?;
            label = label_text(&tail[..at]);
            self.rest = &tail[at + 1..];
        }
        Ok((stroke, start, arrow, label))
    }
}

fn parse_state(lines: &[(usize, String)], warnings: &mut Vec<String>) -> Result<Graph, ParseError> {
    let mut graph = Graph {
        state_diagram: true,
        ..Graph::default()
    };
    for (line, s) in lines {
        if presentation_directive(s, *line, warnings) {
            continue;
        }
        if let Some(value) = s.strip_prefix("direction ") {
            graph.direction = parse_direction(value.trim(), *line)?;
        } else if let Some((from, rest)) = s.split_once("-->") {
            let (to, label) = rest.split_once(':').unwrap_or((rest, ""));
            let from = state_node(&mut graph, from.trim(), true, *line)?;
            let to = state_node(&mut graph, to.trim(), false, *line)?;
            graph.edges.push(Edge {
                from,
                to,
                label: label_text(label),
                stroke: Stroke::Solid,
                start: Arrow::None,
                end: Arrow::Point,
            });
        } else if let Some(rest) = s.strip_prefix("state ") {
            if rest.contains('{') || rest.contains("<<") {
                return Err(ParseError::new(
                    *line,
                    "composite states, forks, and choices are not supported yet",
                ));
            }
            if let Some((label, id)) = rest.rsplit_once(" as ") {
                let node = state_node(&mut graph, id.trim(), true, *line)?;
                graph.nodes[node].label = label_text(label);
            } else {
                state_node(&mut graph, rest.trim(), true, *line)?;
            }
        } else if let Some((id, label)) = s.split_once(':') {
            let node = state_node(&mut graph, id.trim(), true, *line)?;
            graph.nodes[node].label = label_text(label);
        } else {
            state_node(&mut graph, s, true, *line)?;
        }
        if graph.nodes.len() > MAX_NODES || graph.edges.len() > MAX_EDGES {
            return Err(ParseError::new(
                *line,
                "diagram limit exceeded (256 nodes / 1024 edges)",
            ));
        }
    }
    Ok(graph)
}

fn state_node(graph: &mut Graph, id: &str, source: bool, line: usize) -> Result<usize, ParseError> {
    if id == "[*]" {
        let i = graph.node(if source { "__start" } else { "__end" });
        graph.nodes[i].shape = if source { Shape::Start } else { Shape::End };
        graph.nodes[i].label.clear();
        return Ok(i);
    }
    if !valid_identifier(id) {
        return Err(ParseError::new(
            line,
            format!("unsupported state statement or invalid identifier {id:?}"),
        ));
    }
    Ok(graph.node(id))
}

fn valid_identifier(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

fn sequence_id(sequence: &mut Sequence, id: &str, line: usize) -> Result<usize, ParseError> {
    if !valid_identifier(id) {
        return Err(ParseError::new(
            line,
            format!("invalid participant identifier {id:?}"),
        ));
    }
    let i = sequence.participant(id);
    if sequence.participants.len() > MAX_NODES {
        return Err(ParseError::new(line, "too many participants (limit: 256)"));
    }
    Ok(i)
}

fn parse_sequence(lines: &[(usize, String)]) -> Result<Sequence, ParseError> {
    let mut sequence = Sequence::default();
    let mut blocks = Vec::new();
    for (line, s) in lines {
        if sequence.events.len() >= 4096 {
            return Err(ParseError::new(
                *line,
                "too many sequence events (limit: 4096)",
            ));
        }
        if let Some((prefix, rest)) = s.split_once(' ')
            && matches!(prefix, "participant" | "actor")
        {
            let (id, label) = rest.split_once(" as ").unwrap_or((rest, rest));
            let i = sequence_id(&mut sequence, id.trim(), *line)?;
            sequence.participants[i].label = label_text(label);
            sequence.participants[i].actor = prefix == "actor";
            continue;
        }
        if s == "autonumber" {
            sequence.autonumber = true;
            continue;
        }
        if let Some(id) = s.strip_prefix("activate ") {
            let i = sequence_id(&mut sequence, id.trim(), *line)?;
            sequence.events.push(SequenceEvent::Activate(i));
            continue;
        }
        if let Some(id) = s.strip_prefix("deactivate ") {
            let i = sequence_id(&mut sequence, id.trim(), *line)?;
            sequence.events.push(SequenceEvent::Deactivate(i));
            continue;
        }
        let word = s.split_whitespace().next().unwrap_or_default();
        if matches!(
            word,
            "loop" | "alt" | "opt" | "par" | "break" | "critical" | "rect"
        ) {
            blocks.push((*line, word));
            if blocks.len() > 16 {
                return Err(ParseError::new(
                    *line,
                    "sequence blocks may nest at most 16 levels",
                ));
            }
            sequence.events.push(SequenceEvent::BlockStart {
                kind: word.to_owned(),
                text: label_text(s[word.len()..].trim()),
            });
            continue;
        }
        if matches!(word, "else" | "and" | "option") {
            let Some((_, block)) = blocks.last() else {
                return Err(ParseError::new(*line, "branch outside a sequence block"));
            };
            if !matches!(
                (word, *block),
                ("else", "alt") | ("and", "par") | ("option", "critical")
            ) {
                return Err(ParseError::new(
                    *line,
                    format!("{word} is not valid inside {block}"),
                ));
            }
            sequence
                .events
                .push(SequenceEvent::BlockElse(label_text(s[word.len()..].trim())));
            continue;
        }
        if s == "end" {
            if blocks.pop().is_none() {
                return Err(ParseError::new(*line, "end without an open sequence block"));
            }
            sequence.events.push(SequenceEvent::BlockEnd);
            continue;
        }
        if s.get(..5).is_some_and(|s| s.eq_ignore_ascii_case("note ")) {
            let (position, text) = s[5..]
                .split_once(':')
                .ok_or_else(|| ParseError::new(*line, "note requires a colon and text"))?;
            let (side, ids) = if let Some(ids) = position.strip_prefix("over ") {
                (NoteSide::Over, ids)
            } else if let Some(ids) = position.strip_prefix("left of ") {
                (NoteSide::Left, ids)
            } else if let Some(ids) = position.strip_prefix("right of ") {
                (NoteSide::Right, ids)
            } else {
                return Err(ParseError::new(
                    *line,
                    "expected Note over, Note left of, or Note right of",
                ));
            };
            let (from, to) = ids.split_once(',').unwrap_or((ids, ids));
            let from = sequence_id(&mut sequence, from.trim(), *line)?;
            let to = sequence_id(&mut sequence, to.trim(), *line)?;
            sequence.events.push(SequenceEvent::Note {
                from,
                to,
                side,
                text: label_text(text),
            });
            continue;
        }
        let (message, text) = s.split_once(':').ok_or_else(|| {
            ParseError::new(
                *line,
                "unsupported sequence statement; expected A->>B: message",
            )
        })?;
        let arrows = [
            "<<-->>", "<<->>", "-->>", "--x", "--)", "->>", "-->", "-x", "-)", "->",
        ];
        let (at, arrow) = arrows
            .iter()
            .filter_map(|arrow| message.find(arrow).map(|at| (at, *arrow)))
            .min_by_key(|(at, arrow)| (*at, usize::MAX - arrow.len()))
            .ok_or_else(|| {
                ParseError::new(
                    *line,
                    "unsupported message arrow; try ->>, -->>, ->, -->, -x, or -)",
                )
            })?;
        let from = sequence_id(&mut sequence, message[..at].trim(), *line)?;
        let target = message[at + arrow.len()..].trim();
        let activate = target.starts_with('+');
        let deactivate = target.starts_with('-');
        let to = sequence_id(
            &mut sequence,
            target.trim_start_matches(['+', '-']).trim(),
            *line,
        )?;
        sequence.events.push(SequenceEvent::Message {
            from,
            to,
            text: label_text(text),
            stroke: if arrow.contains("--") {
                Stroke::Dashed
            } else {
                Stroke::Solid
            },
            start: if arrow.starts_with("<<") {
                Arrow::Point
            } else {
                Arrow::None
            },
            end: if arrow.ends_with('x') {
                Arrow::Cross
            } else {
                Arrow::Point
            },
            activate,
            deactivate,
        });
        if sequence.events.len() > 4096 {
            return Err(ParseError::new(
                *line,
                "too many sequence events (limit: 4096)",
            ));
        }
    }
    if let Some((line, _)) = blocks.first() {
        return Err(ParseError::new(
            *line,
            "unclosed sequence block; expected end",
        ));
    }
    Ok(sequence)
}

fn unquote(s: &str) -> &str {
    let s = s.trim();
    if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
        s.get(1..s.len().saturating_sub(1)).unwrap_or_default()
    } else {
        s
    }
}

fn label_text(s: &str) -> String {
    let markdown_string = s.trim().starts_with('"') && s.trim().ends_with('"');
    let s = unquote(s);
    let s = if markdown_string {
        s.strip_prefix('`')
            .and_then(|s| s.strip_suffix('`'))
            .unwrap_or(s)
    } else {
        s
    };
    s.replace("<br/>", "\n")
        .replace("<br />", "\n")
        .replace("<br>", "\n")
        .replace("\\n", "\n")
        .replace("\\\"", "\"")
        .replace("&quot;", "\"")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .replace("#quot;", "\"")
        .chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect::<String>()
        .replace('\t', "    ")
        .trim()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph(source: &str) -> Graph {
        let DiagramKind::Graph(g) = parse(source).unwrap().kind else {
            panic!("expected graph")
        };
        g
    }

    #[test]
    fn chained_grouped_edges_and_quoted_delimiters() {
        let g = graph(
            "graph LR; A[\"a; [b] %% c\"] & B -->|\"yes | really\"| C{Ready?} ==> D((Done))\nC -. retry .-> A",
        );
        assert_eq!(g.direction, Direction::Lr);
        assert_eq!(g.nodes.len(), 4);
        assert_eq!(g.nodes[0].label, "a; [b] %% c");
        assert_eq!(g.nodes[2].shape, Shape::Decision);
        assert_eq!(g.edges.len(), 4);
        assert_eq!(g.edges[0].label, "yes | really");
        assert_eq!(g.edges[3].stroke, Stroke::Dashed);
    }

    #[test]
    fn preserves_declared_labels_and_handles_unicode_multiline() {
        let g = graph("flowchart TB\nA[\"你好<br/>café\"] --> B\nB --> A\nA[\"Updated\nlabel\"]");
        assert_eq!(g.nodes.len(), 2);
        assert_eq!(g.nodes[0].label, "Updated\nlabel");
        assert_eq!(g.nodes[1].label, "B");
    }

    #[test]
    fn errors_retain_markdown_line_numbers() {
        let err = parse("Some documentation\n```mermaid\nflowchart LR\nA -->\n```\n").unwrap_err();
        assert_eq!(err.line, 4);
        assert!(err.message.contains("identifier"));
        assert!(
            parse("flowchart LR\nA[broken")
                .unwrap_err()
                .message
                .contains("unclosed")
        );
        assert!(
            parse("pie\n\"a\": 2")
                .unwrap_err()
                .message
                .contains("unsupported diagram")
        );
    }

    #[test]
    fn state_start_and_end_are_distinct_and_aliases_update_nodes() {
        let g = graph(
            "stateDiagram-v2\n[*] --> Running\nRunning --> [*]: stop\nstate \"In progress\" as Running",
        );
        assert_eq!(g.nodes.len(), 3);
        assert_eq!(g.nodes[0].shape, Shape::Start);
        assert_eq!(g.nodes[1].label, "In progress");
        assert_eq!(g.nodes[2].shape, Shape::End);
    }

    #[test]
    fn sequence_aliases_notes_blocks_self_messages_and_activation() {
        let d = parse("sequenceDiagram\nactor A as Alice\nparticipant B as API\nautonumber\nloop retry\nA->>+B: Send\nNote over A,B: waiting\nB-->>-A: Done\nB->>B: cache\nend").unwrap();
        let DiagramKind::Sequence(s) = d.kind else {
            panic!()
        };
        assert_eq!(s.participants[0].label, "Alice");
        assert!(s.participants[0].actor);
        assert!(s.autonumber);
        assert_eq!(s.events.len(), 6);
        assert!(matches!(
            s.events[1],
            SequenceEvent::Message { activate: true, .. }
        ));
        assert!(
            parse("sequenceDiagram\nalt condition\nA->>B: hi")
                .unwrap_err()
                .message
                .contains("unclosed")
        );
        assert!(
            parse("sequenceDiagram\nelse bad")
                .unwrap_err()
                .message
                .contains("outside")
        );
    }

    #[test]
    fn existing_example_is_supported() {
        let d = parse(include_str!("../assets/example.mermaid")).unwrap();
        assert_eq!(d.counts(), (15, 25));
    }

    #[test]
    fn frontmatter_and_control_characters() {
        let d = parse("---\ntitle: \"Demo\"\nconfig:\n  theme: dark\n---\nflowchart TB\nA[\"safe\u{1b}[31m\"]").unwrap();
        assert_eq!(d.title.as_deref(), Some("Demo"));
        assert_eq!(d.warnings.len(), 1);
        let DiagramKind::Graph(g) = d.kind else {
            panic!()
        };
        assert!(!g.nodes[0].label.contains('\u{1b}'));
    }
}
