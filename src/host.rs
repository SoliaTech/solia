use crate::{model::*, package};
use eframe::egui;
use wasmi::{
    Config, Engine, Global, Linker, Module, Store, StoreLimits, StoreLimitsBuilder, TypedFunc,
    core::ValType,
};

use std::sync::atomic::{AtomicU64, Ordering};

enum Event {
    Click(usize),
    Write(usize, i32),
}

pub struct Host {
    id: u64,
    pub(crate) view: View,
    store: Store<StoreLimits>,
    globals: Vec<Global>,
    handlers: Vec<TypedFunc<(), ()>>,
    error: Option<String>,
}

impl Host {
    pub fn load(bytes: &[u8]) -> Result<Self, String> {
        let (view, wasm) = package::decode(bytes)?;
        let mut config = Config::default();
        config.consume_fuel(true);
        let engine = Engine::new(&config);
        let module = Module::new(&engine, wasm).map_err(|e| format!("invalid WebAssembly: {e}"))?;
        let limits = StoreLimitsBuilder::new()
            .instances(1)
            .memories(0)
            .tables(0)
            .build();
        let mut store = Store::new(&engine, limits);
        store.limiter(|limits| limits);
        let instance = Linker::new(&engine)
            .instantiate(&mut store, &module)
            .map_err(|e| e.to_string())?
            .ensure_no_start(&mut store)
            .map_err(|e| e.to_string())?;
        let mut globals = vec![];
        for i in 0..view.config.types.len() {
            let name = format!("state_{i}");
            let g = instance
                .get_global(&store, &name)
                .ok_or_else(|| format!("missing global {name}"))?;
            if g.ty(&store).content() != ValType::I32 {
                return Err(format!("expected i32 global {name}"));
            }
            globals.push(g);
        }
        for node in &view.nodes {
            if let Widget::Checkbox { slot, .. } | Widget::Slider { slot, .. } = node
                && !globals[*slot].ty(&store).mutability().is_mut()
            {
                return Err(format!("input binding state_{slot} is immutable"));
            }
        }
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        let handlers = (0..view.config.handlers)
            .map(|i| instance.get_typed_func(&store, &format!("click_{i}")))
            .collect::<Result<_, _>>()
            .map_err(|e| e.to_string())?;
        Ok(Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            view,
            store,
            globals,
            handlers,
            error: None,
        })
    }

    pub fn click(&mut self, index: usize) -> Result<(), String> {
        let handler = self.handlers.get(index).ok_or("unknown button")?;
        self.store.set_fuel(1_000_000).map_err(|e| e.to_string())?;
        handler
            .call(&mut self.store, ())
            .map_err(|e| format!("button handler failed: {e}"))
    }

    pub(crate) fn label(&self, parts: &[Part]) -> String {
        let mut s = String::new();
        for p in parts {
            match p {
                Part::Text(text) => s.push_str(text),
                Part::State(i) => {
                    let value = self.globals[*i]
                        .get(&self.store)
                        .i32()
                        .expect("validated i32 global");
                    match self.view.config.types[*i] {
                        Type::I32 => s.push_str(&value.to_string()),
                        Type::Bool => s.push_str(if value == 0 { "false" } else { "true" }),
                    }
                }
            }
        }
        s
    }
}

impl Host {
    pub fn title(&self) -> &str {
        &self.view.config.title
    }

    fn state(&self, slot: usize) -> i32 {
        self.globals[slot]
            .get(&self.store)
            .i32()
            .expect("validated i32 global")
    }

    fn draw(&self, ui: &mut egui::Ui, index: usize, events: &mut Vec<Event>) -> usize {
        let node = &self.view.nodes[index];
        match node {
            Widget::Column { gap, end } | Widget::Row { gap, end } => {
                let row = matches!(node, Widget::Row { .. });
                let children = |ui: &mut egui::Ui| {
                    if row {
                        ui.spacing_mut().item_spacing.x = (*gap).into();
                    } else {
                        ui.spacing_mut().item_spacing.y = (*gap).into();
                    }
                    let mut child = index + 1;
                    while child < *end {
                        child = self.draw(ui, child, events);
                    }
                };
                if row {
                    ui.horizontal_wrapped(children);
                } else {
                    ui.vertical(children);
                }
                return *end;
            }
            Widget::Heading(parts) => {
                let response = ui.heading(self.label(parts));
                ui.ctx().accesskit_node_builder(response.id, |n| {
                    n.set_role(egui::accesskit::Role::Heading);
                    n.set_level(1);
                });
            }
            Widget::Text(parts) => {
                ui.label(self.label(parts));
            }
            Widget::Divider => {
                ui.separator();
            }
            Widget::Button { label, handler } => {
                if ui.button(self.label(label)).clicked() {
                    events.push(Event::Click(*handler));
                }
            }
            Widget::Checkbox { label, slot } => {
                let mut checked = self.state(*slot) != 0;
                if ui.checkbox(&mut checked, self.label(label)).changed() {
                    events.push(Event::Write(*slot, i32::from(checked)));
                }
            }
            Widget::Slider {
                label,
                slot,
                min,
                max,
            } => {
                let mut value = self.state(*slot);
                let slider = egui::Slider::new(&mut value, *min..=*max)
                    .clamping(egui::SliderClamping::Edits)
                    .show_value(false)
                    .text(self.label(label));
                if ui.add(slider).changed() {
                    events.push(Event::Write(*slot, value.clamp(*min, *max)));
                }
            }
        }
        index + 1
    }

    pub fn render(&mut self, ui: &mut egui::Ui) {
        let visuals = if self.view.config.dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        ui.visuals_mut().clone_from(&visuals);
        egui::Frame::central_panel(ui.style()).show(ui, |ui| {
            ui.set_min_size(ui.available_size());

            if let Some(message) = &self.error {
                ui.label(message);
                return;
            }
            let mut events = Vec::new();
            ui.push_id(self.id, |ui| self.draw(ui, 0, &mut events));
            for event in events {
                let result = match event {
                    Event::Click(handler) => self.click(handler),
                    Event::Write(slot, value) => self.globals[slot]
                        .set(&mut self.store, wasmi::Val::I32(value))
                        .map_err(|e| e.to_string()),
                };
                ui.ctx().request_repaint();
                if let Err(error) = result {
                    self.error = Some(error);
                    break;
                }
            }
        });
    }
}

impl eframe::App for Host {
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        self.render(ui);
    }
}

pub fn run(host: Host) -> Result<(), String> {
    let title = host.view.config.title.clone();
    let [w, h] = host.view.config.window;
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([f32::from(w), f32::from(h)]),
        ..Default::default()
    };
    eframe::run_native(
        &title,
        options,
        Box::new(move |cc| {
            cc.egui_ctx.set_theme(if host.view.config.dark {
                egui::Theme::Dark
            } else {
                egui::Theme::Light
            });
            Ok(Box::new(host))
        }),
    )
    .map_err(|e| e.to_string())
}
