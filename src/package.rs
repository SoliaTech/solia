use crate::model::*;
use std::fmt::Write;

const MAGIC: &[u8; 4] = b"SAPP";
#[derive(serde::Serialize)]
struct LegacyOutput<'a> {
    #[serde(flatten)]
    config: &'a Config,
    gap: u16,
    widgets: &'a [Widget],
}

#[derive(serde::Deserialize)]
struct LegacyInput {
    #[serde(flatten)]
    config: Config,
    gap: u16,
    #[serde(deserialize_with = "bounded_nodes")]
    widgets: Vec<Widget>,
}

fn legacy(widgets: &[Widget]) -> bool {
    widgets
        .iter()
        .all(|w| matches!(w, Widget::Text(_) | Widget::Button { .. }))
}

pub const MAX_BYTES: usize = 8 * 1024 * 1024;

pub fn build(program: &Program) -> Result<Vec<u8>, String> {
    let mut module = String::from("(module\n");
    for (i, state) in program.states.iter().enumerate() {
        writeln!(
            module,
            "(global $s{i} (export \"state_{i}\") (mut i32) (i32.const {}))",
            state.initial
        )
        .unwrap();
    }
    for (i, body) in program.handlers.iter().enumerate() {
        writeln!(module, "(func (export \"click_{i}\")").unwrap();
        for assignment in body {
            let target = assignment.target;
            if !matches!(assignment.op, Op::Set) {
                writeln!(module, "global.get $s{target}").unwrap();
            }
            match assignment.value {
                Value::Literal(n) => writeln!(module, "i32.const {n}").unwrap(),
                Value::State(slot) => writeln!(module, "global.get $s{slot}").unwrap(),
            }
            match assignment.op {
                Op::Set => {}
                Op::Add => module.push_str("i32.add\n"),
                Op::Subtract => module.push_str("i32.sub\n"),
            }
            writeln!(module, "global.set $s{target}").unwrap();
        }
        module.push_str(")\n");
    }
    module.push(')');
    let wasm = wat::parse_str(module).map_err(|e| e.to_string())?;
    let view = &program.view;
    let (version, manifest) = match view.nodes.split_first() {
        Some((Widget::Column { gap, .. }, children)) if legacy(children) => (
            1u32,
            serde_json::to_vec(&LegacyOutput {
                config: &view.config,
                gap: *gap,
                widgets: children,
            }),
        ),
        _ => (2, serde_json::to_vec(view)),
    };
    let manifest = manifest.map_err(|e| e.to_string())?;
    if manifest.len() + wasm.len() + 12 > MAX_BYTES {
        return Err("package exceeds 8 MiB".into());
    }
    let mut bytes = MAGIC.to_vec();
    bytes.extend(version.to_le_bytes());
    bytes.extend((manifest.len() as u32).to_le_bytes());
    bytes.extend(manifest);
    bytes.extend(wasm);
    Ok(bytes)
}

pub fn decode(bytes: &[u8]) -> Result<(View, &[u8]), String> {
    if bytes.len() > MAX_BYTES || bytes.len() < 12 || &bytes[..4] != MAGIC {
        return Err("invalid SAPP header or size".into());
    }
    let version = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
    if !matches!(version, 1 | 2) {
        return Err(format!(
            "unsupported SAPP version {version} (expected 1 or 2)"
        ));
    }
    let n = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    let (manifest, wasm) = bytes[12..]
        .split_at_checked(n)
        .ok_or("truncated SAPP manifest")?;
    let view: View = match version {
        1 => {
            let old: LegacyInput =
                serde_json::from_slice(manifest).map_err(|e| format!("invalid manifest: {e}"))?;
            if !legacy(&old.widgets) || old.widgets.len() >= MAX_NODES {
                return Err("invalid v1 widget list".into());
            }
            let mut nodes = vec![Widget::Column {
                gap: old.gap,
                end: old.widgets.len() + 1,
            }];
            nodes.extend(old.widgets);
            View {
                config: old.config,
                nodes,
            }
        }
        _ => serde_json::from_slice(manifest).map_err(|e| format!("invalid manifest: {e}"))?,
    };
    validate_view(&view)?;
    Ok((view, wasm))
}

fn validate_view(view: &View) -> Result<(), String> {
    let config = &view.config;
    if config.window.iter().any(|n| !(1..=8192).contains(n)) {
        return Err("invalid layout dimensions".into());
    }
    if !matches!(view.nodes.first(), Some(Widget::Column {end,..} | Widget::Row {end,..}) if *end == view.nodes.len())
    {
        return Err("invalid root layout".into());
    }
    let mut ends = Vec::new();
    let mut buttons = 0;
    for (i, widget) in view.nodes.iter().enumerate() {
        while ends.last() == Some(&i) {
            ends.pop();
        }
        if ends.len() >= MAX_DEPTH {
            return Err("view exceeds depth 32".into());
        }
        let parts = match widget {
            Widget::Row { gap, end } | Widget::Column { gap, end } => {
                if *gap > 8192
                    || *end <= i
                    || *end > ends.last().copied().unwrap_or(view.nodes.len())
                {
                    return Err("invalid container bounds".into());
                }
                ends.push(*end);
                continue;
            }
            Widget::Divider => continue,
            Widget::Text(label) | Widget::Heading(label) => label,
            Widget::Button { label, handler } => {
                if *handler >= config.handlers {
                    return Err("invalid handler reference".into());
                }
                buttons += 1;
                label
            }
            Widget::Checkbox { label, slot } => {
                if config.types.get(*slot) != Some(&Type::Bool) {
                    return Err("invalid checkbox binding".into());
                }
                label
            }
            Widget::Slider {
                label,
                slot,
                min,
                max,
            } => {
                if config.types.get(*slot) != Some(&Type::I32) || min >= max {
                    return Err("invalid slider binding or bounds".into());
                }
                label
            }
        };
        if parts
            .iter()
            .any(|p| matches!(p, Part::State(i) if *i >= config.types.len()))
        {
            return Err("invalid state reference".into());
        }
    }
    if buttons != config.handlers {
        return Err("invalid handler count".into());
    }
    Ok(())
}
