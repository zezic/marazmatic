use clap::ValueEnum;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum Direction {
    #[default]
    #[value(alias = "TD", alias = "TB", alias = "td")]
    Tb,
    #[value(alias = "BT")]
    Bt,
    #[value(alias = "LR")]
    Lr,
    #[value(alias = "RL")]
    Rl,
}

impl Direction {
    pub fn horizontal(self) -> bool {
        matches!(self, Self::Lr | Self::Rl)
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Tb => "TB",
            Self::Bt => "BT",
            Self::Lr => "LR",
            Self::Rl => "RL",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Shape {
    #[default]
    Rectangle,
    Rounded,
    Stadium,
    Decision,
    Hexagon,
    Circle,
    DoubleCircle,
    Cylinder,
    Subroutine,
    Start,
    End,
}

#[derive(Clone, Debug)]
pub struct Node {
    pub id: String,
    pub label: String,
    pub shape: Shape,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Stroke {
    #[default]
    Solid,
    Dashed,
    Thick,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Arrow {
    None,
    #[default]
    Point,
    Circle,
    Cross,
}

#[derive(Clone, Debug)]
pub struct Edge {
    pub from: usize,
    pub to: usize,
    pub label: String,
    pub stroke: Stroke,
    pub start: Arrow,
    pub end: Arrow,
}

#[derive(Clone, Debug, Default)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub direction: Direction,
    pub state_diagram: bool,
}

impl Graph {
    pub fn node(&mut self, id: &str) -> usize {
        if let Some(i) = self.nodes.iter().position(|node| node.id == id) {
            return i;
        }
        let i = self.nodes.len();
        self.nodes.push(Node {
            id: id.to_owned(),
            label: id.to_owned(),
            shape: Shape::default(),
        });
        i
    }
}

#[derive(Clone, Debug)]
pub struct Participant {
    pub id: String,
    pub label: String,
    pub actor: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoteSide {
    Over,
    Left,
    Right,
}

#[derive(Clone, Debug)]
pub enum SequenceEvent {
    Message {
        from: usize,
        to: usize,
        text: String,
        stroke: Stroke,
        start: Arrow,
        end: Arrow,
        activate: bool,
        deactivate: bool,
    },
    Note {
        from: usize,
        to: usize,
        side: NoteSide,
        text: String,
    },
    BlockStart {
        kind: String,
        text: String,
    },
    BlockElse(String),
    BlockEnd,
    Activate(usize),
    Deactivate(usize),
}

#[derive(Clone, Debug, Default)]
pub struct Sequence {
    pub participants: Vec<Participant>,
    pub events: Vec<SequenceEvent>,
    pub autonumber: bool,
}

impl Sequence {
    pub fn participant(&mut self, id: &str) -> usize {
        if let Some(i) = self.participants.iter().position(|p| p.id == id) {
            return i;
        }
        let i = self.participants.len();
        self.participants.push(Participant {
            id: id.to_owned(),
            label: id.to_owned(),
            actor: false,
        });
        i
    }
}

#[derive(Clone, Debug)]
pub enum DiagramKind {
    Graph(Graph),
    Sequence(Sequence),
}

#[derive(Clone, Debug)]
pub struct Diagram {
    pub kind: DiagramKind,
    pub title: Option<String>,
    pub warnings: Vec<String>,
}

impl Diagram {
    pub fn name(&self) -> &'static str {
        match &self.kind {
            DiagramKind::Graph(g) if g.state_diagram => "state diagram",
            DiagramKind::Graph(_) => "flowchart",
            DiagramKind::Sequence(_) => "sequence diagram",
        }
    }

    pub fn counts(&self) -> (usize, usize) {
        match &self.kind {
            DiagramKind::Graph(g) => (g.nodes.len(), g.edges.len()),
            DiagramKind::Sequence(s) => (
                s.participants.len(),
                s.events
                    .iter()
                    .filter(|e| matches!(e, SequenceEvent::Message { .. }))
                    .count(),
            ),
        }
    }
}
