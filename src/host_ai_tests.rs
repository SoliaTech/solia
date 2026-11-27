use super::*;
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    time::{Duration, Instant},
};

fn frame(host: &mut Host, ctx: &egui::Context, events: Vec<egui::Event>) -> egui::FullOutput {
    let mut out = ctx.run_ui(
        egui::RawInput {
            events,
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000., 800.),
            )),
            ..Default::default()
        },
        |ui| host.render(ui),
    );
    out.textures_delta.clear();
    out
}
fn press(host: &mut Host, ctx: &egui::Context, label: &str) {
    frame(host, ctx, vec![]);
    let out = frame(host, ctx, vec![]);
    let pos = out
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text() == label => {
                Some(t.visual_bounding_rect().center())
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing {label}"));
    for pressed in [true, false] {
        frame(
            host,
            ctx,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    modifiers: Default::default(),
                    pressed,
                },
            ],
        );
    }
}
#[test]
fn star_request_requires_approval_and_applies_valid_response() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let mut host =
        Host::load(&crate::compile_source(include_str!("../examples/star-map.solia")).unwrap())
            .unwrap();
    host.provider = ai::Settings {
        base_url: format!("http://{}/v1", listener.local_addr().unwrap()),
        model: "test/model".into(),
        key_env: "SOLIA_TEST_UNUSED_KEY".into(),
        session_key: "test-virtual-key".into(),
    };
    let ctx = egui::Context::default();
    press(&mut host, &ctx, "Suggest a star");
    assert!(host.pending.is_some());
    press(&mut host, &ctx, "Cancel");
    assert!(host.pending.is_none());
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    press(&mut host, &ctx, "Suggest a star");
    press(&mut host, &ctx, "Allow once");
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut stream = loop {
        if let Ok((s, _)) = listener.accept() {
            break s;
        }
        assert!(Instant::now() < deadline, "request was not dispatched");
        std::thread::sleep(Duration::from_millis(5));
    };
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut reader = BufReader::new(&stream);
    let mut headers = String::new();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        if line == "\r\n" {
            break;
        }
        headers.push_str(&line);
    }
    assert!(headers.starts_with("POST /v1/chat/completions "));
    assert!(headers.to_lowercase().contains("x-bf-vk: test-virtual-key"));
    let length = headers
        .lines()
        .find_map(|l| {
            l.to_ascii_lowercase()
                .strip_prefix("content-length: ")
                .and_then(|n| n.parse::<usize>().ok())
        })
        .unwrap();
    let mut body = vec![0; length];
    reader.read_exact(&mut body).unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["model"], "test/model");
    assert_eq!(
        body["response_format"]["json_schema"]["schema"]["properties"]["x"]["maximum"],
        312
    );
    assert!(
        body["messages"][1]["content"]
            .as_str()
            .unwrap()
            .contains("(80, 90)")
    );
    let response = serde_json::json!({"choices":[{"message":{"content":"{\"x\":300,\"y\":160}"}}]})
        .to_string();
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",
        response.len()
    )
    .unwrap();
    while host.pending.is_some() {
        assert!(Instant::now() < deadline, "{}", host.status);
        frame(&mut host, &ctx, vec![]);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!((host.state(2), host.state(3)), (300, 160));
    host.ask(0);
    host.globals[0]
        .set(&mut host.store, wasmi::Val::I32(81))
        .unwrap();
    host.discard_changed();
    assert!(host.pending.is_none());
}
