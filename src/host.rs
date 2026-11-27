use crate::{ai, model::*, package};
use eframe::egui;
use wasmi::{
    Config, Engine, Global, Linker, Module, Store, StoreLimits, StoreLimitsBuilder, TypedFunc,
    core::ValType,
};

use std::sync::atomic::{AtomicU64, Ordering};

enum Event {
    Click(usize),
    Ask(usize),
    Write(usize, i32),
}

struct Pending {
    task: usize,
    prompt: String,
    snapshot: Vec<(usize, i32)>,
    job: Option<ai::Job>,
}

pub struct Host {
    pending: Option<Pending>,
    provider: ai::Settings,
    origin: String,
    status: String,
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
        for field in view.config.tasks.iter().flat_map(|t| &t.fields) {
            if !globals[field.slot].ty(&store).mutability().is_mut() {
                return Err("immutable AI output slot".into());
            }
        }
        let handlers = (0..view.config.handlers)
            .map(|i| instance.get_typed_func(&store, &format!("click_{i}")))
            .collect::<Result<_, _>>()
            .map_err(|e| e.to_string())?;
        let (provider, status) = if view.config.ai_infer {
            match ai::Settings::load() {
                Ok(s) => (s, String::new()),
                Err(e) => (Default::default(), e),
            }
        } else {
            (Default::default(), String::new())
        };
        Ok(Self {
            provider,
            status,
            origin: "Local application".into(),
            pending: None,
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
        let result = handler
            .call(&mut self.store, ())
            .map_err(|e| format!("button handler failed: {e}"));
        self.discard_changed();
        result
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
    pub fn set_source(&mut self, origin: String) {
        self.origin = origin;
    }

    fn ask(&mut self, index: usize) {
        if self.pending.is_some() {
            return;
        }
        let task = &self.view.config.tasks[index];
        let prompt = self.label(&task.prompt);
        if prompt.len() > 4096 {
            self.status = "Expanded prompt exceeds 4 KiB".into();
            return;
        }
        let slots: std::collections::BTreeSet<_> = task
            .prompt
            .iter()
            .filter_map(|p| {
                if let Part::State(i) = p {
                    Some(*i)
                } else {
                    None
                }
            })
            .chain(task.fields.iter().map(|f| f.slot))
            .collect();
        let snapshot = slots.into_iter().map(|i| (i, self.state(i))).collect();
        self.pending = Some(Pending {
            task: index,
            prompt,
            snapshot,
            job: None,
        });
    }
    fn discard_changed(&mut self) {
        if self
            .pending
            .as_ref()
            .is_some_and(|p| p.snapshot.iter().any(|(i, n)| self.state(*i) != *n))
        {
            self.pending = None;
            self.status = "State changed. Request a new suggestion.".into();
        }
    }
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
            Widget::Scene {
                label,
                width,
                height,
                background,
                shapes,
            } => {
                let scale = (ui.available_width().max(0.) / f32::from(*width)).min(1.);
                let size = egui::vec2(f32::from(*width), f32::from(*height)) * scale;
                let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
                ui.ctx().accesskit_node_builder(response.id, |n| {
                    n.set_role(egui::accesskit::Role::Image);
                    n.set_label(self.label(label));
                });
                let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
                let color = |[r, g, b]: [u8; 3]| egui::Color32::from_rgb(r, g, b);
                painter.rect_filled(rect, 0., color(*background));
                let val = |v| {
                    (match v {
                        Value::State(i) => self.state(i),
                        Value::Literal(n) => n,
                    }) as f32
                        * scale
                };
                for shape in shapes {
                    let pos = rect.min + egui::vec2(val(shape.x), val(shape.y));
                    match shape.geometry {
                        Geometry::Rect(w, h) if val(w) > 0. && val(h) > 0. => {
                            painter.rect_filled(
                                egui::Rect::from_min_size(pos, egui::vec2(val(w), val(h))),
                                0.,
                                color(shape.fill),
                            );
                        }
                        Geometry::Circle(r) if val(r) > 0. => {
                            painter.circle_filled(pos, val(r), color(shape.fill));
                        }
                        _ => (),
                    }
                }
            }
            Widget::Run { label, task } => {
                if ui
                    .add_enabled(self.pending.is_none(), egui::Button::new(self.label(label)))
                    .clicked()
                {
                    events.push(Event::Ask(*task));
                }
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
                    Event::Ask(task) => {
                        self.ask(task);
                        Ok(())
                    }
                    Event::Click(handler) => self.click(handler),
                    Event::Write(slot, value) => self.globals[slot]
                        .set(&mut self.store, wasmi::Val::I32(value))
                        .map_err(|e| e.to_string()),
                };
                self.discard_changed();
                ui.ctx().request_repaint();
                if let Err(error) = result {
                    self.error = Some(error);
                    break;
                }
            }
        });
        self.inference_ui(ui);
    }

    fn inference_ui(&mut self, ui: &mut egui::Ui) {
        if !self.view.config.ai_infer {
            return;
        }
        let mut cancel = false;
        let mut completed = None;
        if let Some(p) = &mut self.pending {
            egui::Window::new("Solia · AI request")
                .id(egui::Id::new(("ai", self.id)))
                .collapsible(false)
                .resizable(true)
                .show(ui.ctx(), |ui| {
                    ui.label(format!("Source: {}", self.origin));
                    if let Some(job) = &p.job {
                        match job.result.try_recv() {
                            Ok(result) => completed = Some(result),
                            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                                completed = Some(Err("Inference worker stopped".into()))
                            }
                            Err(std::sync::mpsc::TryRecvError::Empty) => {
                                ui.spinner();
                                ui.label("Waiting for Bifrost…");
                            }
                        }
                    } else {
                        ui.label("Bifrost base URL");
                        ui.text_edit_singleline(&mut self.provider.base_url);
                        ui.label("Model");
                        ui.text_edit_singleline(&mut self.provider.model);
                        ui.label("Virtual key (this instance)");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.provider.session_key)
                                .password(true)
                                .hint_text(format!("Uses {} when empty", self.provider.key_env)),
                        );
                        ui.label("Prompt");
                        ui.label(&p.prompt);
                        ui.label(format!(
                            "Requested fields: {}",
                            self.view.config.tasks[p.task]
                                .fields
                                .iter()
                                .map(|f| f.name.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        ));
                        if ui.button("Allow once").clicked() {
                            match ai::start(
                                &self.provider,
                                p.prompt.clone(),
                                self.view.config.tasks[p.task].fields.clone(),
                                ui.ctx().clone(),
                            ) {
                                Ok(job) => {
                                    p.job = Some(job);
                                    self.status.clear();
                                }
                                Err(e) => self.status = e,
                            }
                        }
                    }
                    cancel = ui.button("Cancel").clicked();
                    if !self.status.is_empty() {
                        ui.label(&self.status);
                    }
                });
        }
        if cancel {
            self.pending = None;
            self.status = "AI request cancelled".into();
        } else if let Some(result) = completed {
            self.pending = None;
            self.status = match result.and_then(|writes| {
                // Slots were validated as mutable i32 globals before instantiation.
                for (slot, value) in writes {
                    self.globals[slot]
                        .set(&mut self.store, wasmi::Val::I32(value))
                        .map_err(|e| e.to_string())?;
                }
                Ok(())
            }) {
                Ok(()) => "AI suggestion applied".into(),
                Err(e) => e,
            };
            ui.ctx().request_repaint();
        }
        if self.pending.is_none() && !self.status.is_empty() {
            egui::Window::new("Solia · AI status")
                .id(egui::Id::new(("ai-status", self.id)))
                .collapsible(false)
                .show(ui.ctx(), |ui| {
                    ui.label(&self.status);
                    if ui.button("Dismiss").clicked() {
                        self.status.clear();
                    }
                });
        }
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

#[cfg(test)]
#[path = "host_ai_tests.rs"]
mod ai_tests;
