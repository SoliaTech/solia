use crate::model::*;
use std::collections::HashMap;

#[derive(Debug, PartialEq)]
enum Kind {
    Word(String),
    Number(String),
    Str(String),
    Symbol(char),
    End,
}

struct Token {
    kind: Kind,
    line: usize,
    col: usize,
}

fn lex(src: &str) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let mut chars = src.chars().peekable();
    let (mut line, mut col) = (1, 1);
    while let Some(c) = chars.next() {
        let start = col;
        col += 1;
        let kind = match c {
            '\n' => {
                line += 1;
                col = 1;
                continue;
            }
            c if c.is_whitespace() => continue,
            '/' if chars.peek() == Some(&'/') => {
                while chars.next_if(|c| *c != '\n').is_some() {
                    col += 1;
                }
                continue;
            }
            '"' => {
                let mut text = String::new();
                loop {
                    let ch = match chars.next() {
                        None | Some('\n') => {
                            return Err(format!("{line}:{start}: unterminated string"));
                        }
                        Some(c) => c,
                    };
                    col += 1;
                    match ch {
                        '"' => break,
                        '\\' => {
                            text.push(match chars.next() {
                                Some('"') => '"',
                                Some('\\') => '\\',
                                Some('n') => '\n',
                                _ => return Err(format!("{line}:{col}: invalid string escape")),
                            });
                            col += 1;
                        }
                        _ => text.push(ch),
                    }
                }
                Kind::Str(text)
            }
            c if c.is_ascii_alphabetic() || c == '_' => {
                let mut word = String::from(c);
                while let Some(ch) = chars.next_if(|c| c.is_ascii_alphanumeric() || *c == '_') {
                    word.push(ch);
                    col += 1;
                }
                Kind::Word(word)
            }
            c if c.is_ascii_digit() => {
                let mut digits = String::from(c);
                while let Some(ch) = chars.next_if(char::is_ascii_digit) {
                    digits.push(ch);
                    col += 1;
                }
                Kind::Number(digits)
            }
            c if "{}():,=+-".contains(c) => Kind::Symbol(c),
            _ => return Err(format!("{line}:{start}: unexpected character {c:?}")),
        };
        tokens.push(Token {
            kind,
            line,
            col: start,
        });
    }
    tokens.push(Token {
        kind: Kind::End,
        line,
        col,
    });
    Ok(tokens)
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    slots: HashMap<String, usize>,
    states: Vec<State>,
}

type Result<T, E = String> = std::result::Result<T, E>;

pub fn compile(src: &str) -> Result<Program> {
    Parser {
        tokens: lex(src)?,
        pos: 0,
        slots: HashMap::new(),
        states: vec![],
    }
    .program()
}

impl Parser {
    fn peek(&self) -> &Kind {
        &self.tokens[self.pos].kind
    }
    fn error(&self, msg: impl std::fmt::Display) -> String {
        let t = &self.tokens[self.pos];
        format!("{}:{}: {msg}", t.line, t.col)
    }
    fn take(&mut self, k: &Kind) -> bool {
        if self.peek() == k {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn sym(&mut self, c: char) -> Result<()> {
        if self.take(&Kind::Symbol(c)) {
            Ok(())
        } else {
            Err(self.error(format!("expected '{c}'")))
        }
    }
    fn word(&mut self) -> Result<String> {
        let Kind::Word(w) = self.peek() else {
            return Err(self.error("expected identifier"));
        };
        let w = w.clone();
        self.pos += 1;
        Ok(w)
    }
    fn keyword(&mut self, s: &str) -> Result<()> {
        if self.take(&Kind::Word(s.into())) {
            Ok(())
        } else {
            Err(self.error(format!("expected {s}")))
        }
    }
    fn string(&mut self) -> Result<String> {
        let Kind::Str(s) = self.peek() else {
            return Err(self.error("expected string"));
        };
        let s = s.clone();
        self.pos += 1;
        Ok(s)
    }
    fn number(&mut self) -> Result<i32> {
        let negative = self.take(&Kind::Symbol('-'));
        let Kind::Number(n) = self.peek() else {
            return Err(self.error("expected integer"));
        };
        let raw = if negative { format!("-{n}") } else { n.clone() };
        let n = raw
            .parse()
            .map_err(|_| self.error("integer outside i32 range"))?;
        self.pos += 1;
        Ok(n)
    }
    fn size(&mut self, min: i32) -> Result<u16> {
        let err = self.error(format!("expected size in {min}..=8192"));
        let n = self.number()?;
        if (min..=8192).contains(&n) {
            Ok(n as u16)
        } else {
            Err(err)
        }
    }
    fn literal(&mut self) -> Result<(Type, i32)> {
        for (s, v) in [("true", 1), ("false", 0)] {
            if self.take(&Kind::Word(s.into())) {
                return Ok((Type::Bool, v));
            }
        }
        Ok((Type::I32, self.number()?))
    }
    fn state(&mut self) -> Result<()> {
        let location = self.error("state declaration");
        let name = self.word()?;
        if self.slots.contains_key(&name) || matches!(name.as_str(), "true" | "false") {
            return Err(format!("{location}: invalid or duplicate state '{name}'"));
        }
        let ty = if self.take(&Kind::Symbol(':')) {
            Some(match self.word()?.as_str() {
                "i32" => Type::I32,
                "bool" => Type::Bool,
                other => return Err(format!("{location}: unknown type '{other}'")),
            })
        } else {
            None
        };
        self.sym('=')?;
        let (actual, initial) = self.literal()?;
        if ty.is_some_and(|t| t != actual) {
            return Err(format!("{location}: initializer type mismatch"));
        }
        self.slots.insert(name, self.states.len());
        self.states.push(State {
            ty: actual,
            initial,
        });
        Ok(())
    }
    fn slot(&self, s: &str) -> Result<usize> {
        self.slots
            .get(s)
            .copied()
            .ok_or_else(|| self.error(format!("unknown state '{s}'")))
    }
    fn assignment(&mut self) -> Result<Assignment> {
        let loc = self.error("assignment");
        let name = self.word()?;
        let target = self.slot(&name)?;
        let op = if self.take(&Kind::Symbol('+')) {
            Op::Add
        } else if self.take(&Kind::Symbol('-')) {
            Op::Subtract
        } else {
            Op::Set
        };
        self.sym('=')?;
        let (ty, value) = match self.peek() {
            Kind::Word(w) if w != "true" && w != "false" => {
                let idx = self.slot(w)?;
                self.pos += 1;
                (self.states[idx].ty, Value::State(idx))
            }
            _ => {
                let (ty, n) = self.literal()?;
                (ty, Value::Literal(n))
            }
        };
        if ty != self.states[target].ty || (!matches!(op, Op::Set) && ty != Type::I32) {
            return Err(format!("{loc}: incompatible types for '{name}'"));
        }
        Ok(Assignment { target, op, value })
    }
    fn label(&mut self) -> Result<Vec<Part>> {
        let mut parts = Vec::new();
        let Kind::Str(s) = self.peek() else {
            return Err(self.error("expected label string"));
        };
        let mut rest = s.as_str();
        while let Some(i) = rest.find(['{', '}']) {
            if rest.as_bytes()[i] == b'}' {
                return Err(self.error("unmatched '}' in label"));
            }
            if i > 0 {
                parts.push(Part::Text(rest[..i].into()));
            }
            let (name, tail) = rest[i + 1..]
                .split_once('}')
                .ok_or_else(|| self.error("unclosed interpolation"))?;
            parts.push(Part::State(self.slot(name)?));
            rest = tail;
        }
        if !rest.is_empty() {
            parts.push(Part::Text(rest.into()));
        }
        self.pos += 1;
        Ok(parts)
    }
    fn parameter(&mut self, name: &str) -> Result<()> {
        self.sym(',')?;
        self.keyword(name)?;
        self.sym(':')
    }

    fn binding(&mut self, param: &str, ty: Type) -> Result<usize> {
        self.parameter(param)?;
        let loc = self.error(param);
        let name = self.word()?;
        let slot = self.slot(&name)?;
        let actual = self.states[slot].ty;
        if actual != ty {
            return Err(format!(
                "{loc}: requires {ty:?}, '{name}' has type {actual:?}"
            ));
        }
        Ok(slot)
    }

    fn widget(
        &mut self,
        depth: usize,
        nodes: &mut Vec<Widget>,
        handlers: &mut Vec<Vec<Assignment>>,
    ) -> Result<()> {
        if depth > MAX_DEPTH || nodes.len() >= MAX_NODES {
            return Err(self.error("view exceeds depth 32 or 4096 nodes"));
        }
        let name = self.word()?;
        if matches!(name.as_str(), "Column" | "Row") {
            let mut gap = 8;
            if self.take(&Kind::Symbol('(')) {
                self.keyword("gap")?;
                self.sym(':')?;
                gap = self.size(0)?;
                self.sym(')')?;
            }
            self.sym('{')?;
            let start = nodes.len();
            nodes.push(Widget::Column { gap, end: start });
            while !self.take(&Kind::Symbol('}')) {
                self.widget(depth + 1, nodes, handlers)?;
            }
            let end = nodes.len();
            nodes[start] = if name == "Row" {
                Widget::Row { gap, end }
            } else {
                Widget::Column { gap, end }
            };
            return Ok(());
        }
        if depth == 1 {
            return Err(self.error("expected root Column or Row"));
        }
        self.sym('(')?;
        if name == "Divider" {
            self.sym(')')?;
            nodes.push(Widget::Divider);
            return Ok(());
        }
        let label = self.label()?;
        let widget = match name.as_str() {
            "Text" => Widget::Text(label),
            "Heading" => Widget::Heading(label),
            "Checkbox" => Widget::Checkbox {
                label,
                slot: self.binding("checked", Type::Bool)?,
            },
            "Slider" => {
                let slot = self.binding("value", Type::I32)?;
                self.parameter("min")?;
                let min = self.number()?;
                self.parameter("max")?;
                let max = self.number()?;
                if min >= max {
                    return Err(self.error("Slider requires min < max"));
                }
                Widget::Slider {
                    label,
                    slot,
                    min,
                    max,
                }
            }
            "Button" => {
                self.sym(')')?;
                self.sym('{')?;
                let mut body = vec![];
                while !self.take(&Kind::Symbol('}')) {
                    body.push(self.assignment()?);
                }
                let handler = handlers.len();
                handlers.push(body);
                nodes.push(Widget::Button { label, handler });
                return Ok(());
            }
            _ => return Err(self.error(format!("unknown widget '{name}'"))),
        };
        self.sym(')')?;
        nodes.push(widget);
        Ok(())
    }

    fn program(mut self) -> Result<Program> {
        self.keyword("app")?;
        let mut title = self.word()?;
        let mut window = [640, 480];
        let mut dark = false;
        self.sym('{')?;
        if self.take(&Kind::Word("config".into())) {
            self.sym('{')?;
            let mut seen = std::collections::HashSet::new();
            while !self.take(&Kind::Symbol('}')) {
                let key = self.word()?;
                if !seen.insert(key.clone()) {
                    return Err(self.error(format!("duplicate config '{key}'")));
                }
                self.sym(':')?;
                match key.as_str() {
                    "title" => title = self.string()?,
                    "window" => {
                        self.sym('(')?;
                        window[0] = self.size(1)?;
                        self.sym(',')?;
                        window[1] = self.size(1)?;
                        self.sym(')')?;
                    }
                    "theme" => {
                        dark = match self.word()?.as_str() {
                            "light" => false,
                            "dark" => true,
                            _ => return Err(self.error("theme must be light or dark")),
                        }
                    }
                    _ => return Err(self.error(format!("unknown config '{key}'"))),
                }
            }
        }
        if self.take(&Kind::Word("state".into())) {
            self.sym('{')?;
            while !self.take(&Kind::Symbol('}')) {
                self.state()?;
            }
        }
        self.keyword("view")?;
        self.sym('{')?;
        let mut nodes = vec![];
        let mut handlers = vec![];
        self.widget(1, &mut nodes, &mut handlers)?;
        self.sym('}')?;
        self.sym('}')?;
        if self.peek() != &Kind::End {
            return Err(self.error("unexpected text after app"));
        }
        let view = View {
            nodes,
            config: Config {
                title,
                window,
                dark,
                handlers: handlers.len(),
                types: self.states.iter().map(|s| s.ty).collect(),
            },
        };
        Ok(Program {
            view,
            states: self.states,
            handlers,
        })
    }
}
