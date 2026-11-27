use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Type {
    I32,
    Bool,
}

#[derive(Debug)]
pub struct State {
    pub ty: Type,
    pub initial: i32,
}

#[derive(Debug)]
pub enum Value {
    Literal(i32),
    State(usize),
}

#[derive(Debug)]
pub enum Op {
    Set,
    Add,
    Subtract,
}

#[derive(Debug)]
pub struct Assignment {
    pub target: usize,
    pub op: Op,
    pub value: Value,
}

/// Checked, backend-independent program. State and handler references are resolved slots.
pub struct Program {
    pub view: View,
    pub states: Vec<State>,
    pub handlers: Vec<Vec<Assignment>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Part {
    Text(String),
    State(usize),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Widget {
    Column {
        gap: u16,
        end: usize,
    },
    Row {
        gap: u16,
        end: usize,
    },
    Heading(Vec<Part>),
    Divider,
    Checkbox {
        label: Vec<Part>,
        slot: usize,
    },
    Slider {
        label: Vec<Part>,
        slot: usize,
        min: i32,
        max: i32,
    },
    Text(Vec<Part>),
    Button {
        label: Vec<Part>,
        handler: usize,
    },
}

pub const MAX_DEPTH: usize = 32;
pub const MAX_NODES: usize = 4096;

/// Nodes in source order. A container's exclusive end index bounds its descendants.
#[derive(Debug, Serialize, Deserialize)]
pub struct View {
    #[serde(flatten)]
    pub config: Config,
    #[serde(deserialize_with = "bounded_nodes")]
    pub nodes: Vec<Widget>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub title: String,
    pub window: [u16; 2],
    pub dark: bool,
    pub types: Vec<Type>,
    pub handlers: usize,
}

pub fn bounded_nodes<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<Widget>, D::Error> {
    use serde::de::{Error, SeqAccess, Visitor};
    struct Nodes;
    impl<'de> Visitor<'de> for Nodes {
        type Value = Vec<Widget>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            write!(f, "at most {MAX_NODES} view nodes")
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let mut nodes = Vec::new();
            while nodes.len() < MAX_NODES {
                match seq.next_element()? {
                    Some(node) => nodes.push(node),
                    None => return Ok(nodes),
                }
            }
            if seq.next_element::<serde::de::IgnoredAny>()?.is_some() {
                return Err(A::Error::custom("view exceeds 4096 nodes"));
            }
            Ok(nodes)
        }
    }
    d.deserialize_seq(Nodes)
}
