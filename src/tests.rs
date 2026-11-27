use super::*;
use model::Widget;

const COUNTER: &str = include_str!("../examples/counter.solia");

fn package_for(source: &str) -> Vec<u8> {
    package::build(&parser::compile(source).unwrap()).unwrap()
}

fn text_at(host: &host::Host, index: usize) -> String {
    let Widget::Text(parts) = &host.view.nodes[index + 1] else {
        panic!("expected Text")
    };
    host.label(parts)
}

#[test]
fn packaged_counter_executes_in_wasm() {
    let bytes = package_for(COUNTER);
    let mut host = host::Host::load(&bytes).unwrap();
    assert_eq!(host.view.config.window, [800, 600]);
    assert!(matches!(host.view.nodes[0], Widget::Column { gap: 16, .. }));
    assert!(host.view.config.dark);
    assert_eq!(text_at(&host, 0), "Count: 0");
    for count in 1..=3 {
        host.click(0).unwrap();
        assert_eq!(text_at(&host, 0), format!("Count: {count}"));
    }
    assert_eq!(text_at(&host::Host::load(&bytes).unwrap(), 0), "Count: 0");
}

#[test]
fn handlers_read_current_typed_state_in_source_order() {
    let bytes = package_for(
        r#"app Meter {
      state { x = -4 step: i32 = 2 done = false }
      view { Column {
        Text("Δ {x} / {done}")
        Button("+{step}") { x += step done = true }
        Button("Reset") { x = step x -= 2 done = false }
      } }
    }"#,
    );
    let mut host = host::Host::load(&bytes).unwrap();
    host.click(0).unwrap();
    assert_eq!(text_at(&host, 0), "Δ -2 / true");
    host.click(1).unwrap();
    assert_eq!(text_at(&host, 0), "Δ 0 / false");
}

#[test]
fn invalid_source_reports_a_location() {
    for (from, to, message) in [
        ("i32 = 0", "bool = 0", "type mismatch"),
        ("= 0", "= 0 count = 1", "duplicate state"),
        ("{count}", "{typo}", "unknown state"),
        ("count += 1", "count += false", "incompatible types"),
        ("count += 1", "count = 2147483648", "i32 range"),
        ("Text(", "Image(", "unknown widget"),
        ("Count: {count}", "Count: {count", "unclosed interpolation"),
    ] {
        let err = parser::compile(&COUNTER.replace(from, to)).err().unwrap();
        assert!(err.contains(message), "{err}");
        assert!(
            err.split(':').take(2).all(|s| s.parse::<usize>().is_ok()),
            "{err}"
        );
    }
    assert!(parser::compile(&format!("{COUNTER} garbage")).is_err());
    assert!(parser::compile("app").is_err());
}

#[test]
fn loader_rejects_incompatible_and_truncated_artifacts() {
    let bytes = package_for(COUNTER);
    for size in [0, 4, 11, 12, bytes.len() - 1] {
        assert!(host::Host::load(&bytes[..size]).is_err());
    }
    let mut newer = bytes;
    newer[4] = 4;
    assert!(
        host::Host::load(&newer)
            .err()
            .unwrap()
            .contains("unsupported SAPP version")
    );
}

#[test]
fn loader_checks_manifest_bindings_and_wasm_imports() {
    let bytes = package_for(COUNTER);
    let (mut view, _) = package::decode(&bytes).unwrap();
    view.nodes[1] = Widget::Text(vec![model::Part::State(1)]);
    let json = serde_json::to_vec(&view).unwrap();
    let mut broken = bytes[..8].to_vec();
    broken[4] = 2;
    broken.extend((json.len() as u32).to_le_bytes());
    broken.extend(json);
    assert!(
        host::Host::load(&broken)
            .err()
            .unwrap()
            .contains("state reference")
    );

    let (_, wasm) = package::decode(&bytes).unwrap();
    for module in ["(module)", r#"(module (import "host" "read" (func)))"#] {
        let mut bad_module = bytes[..bytes.len() - wasm.len()].to_vec();
        bad_module.extend(wat::parse_str(module).unwrap());
        assert!(host::Host::load(&bad_module).is_err());
    }
}

#[test]
fn counter_button_repaints_the_label_after_pointer_input() {
    use eframe::{App, egui};
    let mut host = host::Host::load(&package_for(COUNTER)).unwrap();
    let ctx = egui::Context::default();
    let mut frame = eframe::Frame::_new_kittest();
    let mut render = |events| {
        let mut output = ctx.run_ui(
            egui::RawInput {
                events,
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800., 600.),
                )),
                ..Default::default()
            },
            |ui| host.ui(ui, &mut frame),
        );
        output.textures_delta.clear();
        output
    };
    fn label_position(output: &egui::FullOutput, label: &str) -> Option<egui::Pos2> {
        output.shapes.iter().find_map(|s| match &s.shape {
            egui::Shape::Text(text) if text.galley.text() == label => {
                Some(text.visual_bounding_rect().center())
            }
            _ => None,
        })
    }
    let first = render(vec![]);
    assert!(label_position(&first, "Count: 0").is_some());
    let pos = label_position(&first, "Increment").unwrap();
    for pressed in [true, false] {
        render(vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            },
        ]);
    }
    assert!(label_position(&render(vec![]), "Count: 1").is_some());
}

const SETTINGS: &str = include_str!("../examples/settings.solia");

#[test]
fn compatible_versions_and_checked_ui_tree() {
    let old = include_bytes!("../tests/fixtures/counter-v1.sapp");
    let mut host = host::Host::load(old).unwrap();
    host.click(0).unwrap();
    assert_eq!(text_at(&host, 0), "Count: 1");
    assert_eq!(package_for(COUNTER)[4], 1);
    assert_eq!(package_for(SETTINGS)[4], 2);
    for (from, to, reason) in [
        ("checked: muted", "checked: volume", "requires Bool"),
        ("value: volume", "value: muted", "requires I32"),
        ("max: 100", "max: 0", "min < max"),
        ("min: 0", "min: 0, min: 1", "expected max"),
    ] {
        let err = parser::compile(&SETTINGS.replace(from, to)).err().unwrap();
        assert!(err.contains(reason), "{err}");
    }
    for (levels, ok) in [(32, true), (33, false)] {
        let src = format!(
            "app Deep {{ view {{ {} {} }} }}",
            "Row {".repeat(levels),
            "}".repeat(levels)
        );
        assert_eq!(parser::compile(&src).is_ok(), ok);
    }
    for (leaves, ok) in [(4095, true), (4096, false)] {
        let src = format!(
            "app Wide {{ view {{ Column {{ {} }} }} }}",
            "Divider() ".repeat(leaves)
        );
        assert_eq!(parser::compile(&src).is_ok(), ok);
    }
}

#[test]
fn loader_checks_input_slots_mutability_and_tree_bounds() {
    let bytes = package_for(SETTINGS);
    let (mut view, _) = package::decode(&bytes).unwrap();
    fn artifact(view: &model::View, wasm: &[u8]) -> Vec<u8> {
        let mut bytes = b"SAPP".to_vec();
        bytes.extend(2u32.to_le_bytes());
        let manifest = serde_json::to_vec(view).unwrap();
        bytes.extend((manifest.len() as u32).to_le_bytes());
        bytes.extend(manifest);
        bytes.extend(wasm);
        bytes
    }
    let wasm = wat::parse_str(
        r#"(module
        (global (export "state_0") i32 (i32.const 0))
        (global (export "state_1") (mut i32) (i32.const 40))
        (func (export "click_0")))"#,
    )
    .unwrap();
    assert!(
        host::Host::load(&artifact(&view, &wasm))
            .err()
            .unwrap()
            .contains("immutable")
    );
    view.nodes[2] = Widget::Checkbox {
        label: vec![],
        slot: 1,
    };
    assert!(
        package::decode(&artifact(&view, &wasm))
            .err()
            .unwrap()
            .contains("binding")
    );
    view.nodes[2] = Widget::Column {
        gap: 1,
        end: usize::MAX,
    };
    assert!(
        package::decode(&artifact(&view, &wasm))
            .err()
            .unwrap()
            .contains("container bounds")
    );
}

#[test]
fn narrow_row_wraps_and_painting_keeps_out_of_range_state() {
    use eframe::egui;
    let src = r#"app Compact {
        state { volume = 150 a = false b = true }
        view { Column {
            Row { Button("First action") {} Button("Second action") {} }
            Checkbox("Flag", checked: a) Checkbox("Flag", checked: b)
            Slider("Volume", value: volume, min: 0, max: 100)
            Text("{a}/{b}/{volume}")
        } }
    }"#;
    let mut host = host::Host::load(&package_for(src)).unwrap();
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut paint = |width, events| {
        let mut frame = ctx.run_ui(
            egui::RawInput {
                events,
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 600.),
                )),
                ..Default::default()
            },
            |ui| host.render(ui),
        );
        frame.textures_delta.clear();
        frame
    };
    let pos = |frame: &egui::FullOutput, name| {
        frame
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.text() == name => {
                    Some(t.visual_bounding_rect().center())
                }
                _ => None,
            })
            .unwrap()
    };
    let wide = paint(600., vec![]);
    assert!((pos(&wide, "First action").y - pos(&wide, "Second action").y).abs() < 2.);
    paint(120., vec![]);
    let narrow = paint(120., vec![]);
    assert!(pos(&narrow, "First action").y + 10. < pos(&narrow, "Second action").y);
    let checks: Vec<_> = narrow
        .platform_output
        .accesskit_update
        .as_ref()
        .unwrap()
        .nodes
        .iter()
        .filter(|(_, node)| node.role() == egui::accesskit::Role::CheckBox)
        .collect();
    assert_eq!(checks.len(), 2);
    assert_ne!(checks[0].0, checks[1].0);
    let rect = checks
        .iter()
        .map(|(_, node)| node.bounds().unwrap())
        .max_by(|a, b| a.y0.total_cmp(&b.y0))
        .unwrap();
    let point = egui::pos2(
        ((rect.x0 + rect.x1) / 2.) as f32,
        ((rect.y0 + rect.y1) / 2.) as f32,
    );
    for pressed in [true, false] {
        paint(
            120.,
            vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ],
        );
    }
    pos(&paint(120., vec![]), "false/false/150");
}

const STAR: &str = include_str!("../examples/star-map.solia");

#[test]
fn scene_bounds_and_capability_are_checked_in_source_and_packages() {
    let bytes = package_for(STAR);
    assert_eq!(bytes[4], 3);
    let (view, wasm) = package::decode(&bytes).unwrap();
    assert_eq!(view.config.tasks.len(), 1);
    assert!(host::Host::load(&bytes).is_ok());
    for (from, to) in [
        ("capabilities { ai.infer }", ""),
        ("radius: 6", "radius: -1"),
        ("width: 320", "width: 4097"),
        ("-> star_y", "-> star_x"),
        ("run suggest", "run other"),
    ] {
        assert!(check_source(&STAR.replace(from, to)).is_err(), "{to}");
    }
    let mut manifest = serde_json::to_value(&view).unwrap();
    manifest["ai_infer"] = false.into();
    let manifest = serde_json::to_vec(&manifest).unwrap();
    let mut artifact = bytes[..8].to_vec();
    artifact.extend((manifest.len() as u32).to_le_bytes());
    artifact.extend(manifest);
    artifact.extend(wasm);
    assert!(package::decode(&artifact).is_err());
    let mut downgrade = bytes;
    downgrade[4] = 2;
    assert!(package::decode(&downgrade).is_err());
}

#[test]
fn task_outputs_require_complete_unique_typed_values() {
    let fields = vec![
        model::Field {
            name: "x".into(),
            slot: 2,
            range: Some((8, 312)),
        },
        model::Field {
            name: "ok".into(),
            slot: 3,
            range: None,
        },
    ];
    assert_eq!(
        ai::validate(r#"{"ok":true,"x":200}"#, &fields).unwrap(),
        vec![(2, 200), (3, 1)]
    );
    for bad in [
        r#"{"ok":1,"x":200}"#,
        r#"{"ok":false,"ok":true,"x":200}"#,
        r#"{"x":100}"#,
        r#"{"ok":true,"x":313}"#,
        r#"{"ok":true,"x":1.5}"#,
        r#"{"ok":true,"x":200,"z":1}"#,
    ] {
        assert!(ai::validate(bad, &fields).is_err(), "{bad}");
    }
}

#[test]
fn scene_scales_geometry_and_clips_to_the_canvas() {
    use eframe::egui;
    let mut app = host::Host::load(&package_for(STAR)).unwrap();
    let ctx = egui::Context::default();
    for width in [600., 200.] {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 600.),
                )),
                ..Default::default()
            },
            |ui| app.render(ui),
        );
        output.textures_delta.clear();
        let circles: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Circle(circle)
                    if circle.fill == egui::Color32::from_rgb(88, 166, 255)
                        || circle.fill == egui::Color32::from_rgb(255, 179, 92) =>
                {
                    Some((circle, shape.clip_rect))
                }
                _ => None,
            })
            .collect();
        assert_eq!(circles.len(), 2);
        let (base, clip) = circles[0];
        assert_eq!(clip, circles[1].1);
        let scale = clip.width() / 320.;
        assert!((base.radius - 6. * scale).abs() < 0.01);
        assert!((circles[1].0.center.x - base.center.x - 160. * scale).abs() < 0.01);
        assert!((clip.height() - 180. * scale).abs() < 0.01);
    }
}

#[test]
#[ignore = "requires a configured Bifrost model and virtual key"]
fn configured_bifrost_star_task() {
    let p = parser::compile(STAR).unwrap();
    let host = host::Host::load(&package_for(STAR)).unwrap();
    let task = &p.view.config.tasks[0];
    let job = ai::start(
        &ai::Settings::load().unwrap(),
        host.label(&task.prompt),
        task.fields.clone(),
        Default::default(),
    )
    .unwrap();
    let writes = job
        .result
        .recv_timeout(std::time::Duration::from_secs(35))
        .unwrap()
        .unwrap();
    assert_eq!(writes.len(), 2);
}
