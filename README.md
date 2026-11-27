# Solia

**A strongly typed, declarative language and application runtime for AI-native, interactive applications and games.**

Solia is an early-stage language and platform concept. Its goal is to make an application—not a web page—the primary unit of development. A Solia application can describe its data, state, user interface, application logic, asynchronous work, AI capabilities, and interactive scenes in one coherent language and run inside a cross-platform native host.

> **Project status:** The Phase 1 desktop prototype runs the Counter example. The implemented subset and commands are described below. The wider language and platform sections describe the design roadmap.

## Run the first MVP

From this repository with a current stable Rust toolchain and a graphical desktop:

```sh
cargo run -- check examples/counter.solia
cargo run -- build examples/counter.solia target/counter.sapp
cargo run -- run target/counter.sapp
```

The native window displays **Count: 0**. Activate **Increment** to run its compiled WebAssembly handler and see **Count: 1**. Each launch creates fresh application state. To run directly from source, use `cargo run -- run examples/counter.solia`. To exercise the compiler, package loader, and runtime without opening a window, use `cargo test --locked`.

### Implemented syntax

A file defines one `app Name` with an optional `config`, an optional `state`, and a required `view`, in that order. Whitespace separates tokens and `//` starts a line comment. Identifiers use ASCII letters, digits, and underscores and start with a letter or underscore.

- **Configuration.** Set `title` to a quoted string, `window` to a pair of integer dimensions (1–8192), and `theme` to `dark` or `light`. Defaults are the app name, 640 × 480, and light.
- **State.** Declare `count: i32 = 0` or `ready = false`. The compiler infers omitted types from signed integer and boolean literals. It resolves names and checks assignments before emitting code.
- **Views.** Nest `Column` and wrapping `Row` layouts. Use `Text`, `Heading`, `Divider()`, and `Button` for presentation and actions. Bind `Checkbox("Mute", checked: muted)` to a boolean and `Slider("Volume", value: volume, min: 0, max: 100)` to an integer. Labels support Unicode and `{stateName}` interpolation. [Settings example](examples/settings.solia)
- **Handlers.** Use `=` for assignment and `+=` or `-=` for integer updates. The right-hand side is a literal or a state name of the same type. Statements execute in order. Integer arithmetic wraps at 32 bits.

### Package and host

The Rust frontend produces a typed program with resolved state slots, view bindings, and handlers. The backend emits a WebAssembly module with one exported i32 global per slot and one parameterless function per button. Boolean values use 0 and 1. The Rust host uses wasmi for execution and eframe for windowing, layout, semantic widgets, keyboard focus, and rendering.

Both package versions contain the four bytes `SAPP`, a little-endian u32 version, a little-endian u32 manifest length, a UTF-8 JSON view manifest, and the WebAssembly bytes. A single column of text and buttons emits v1. Extended components emit v2 with a preorder node list, exclusive subtree ends, and typed state bindings. The host checks the version, container bounds, view references, input mutability, and export types before opening a window. Modules run with an empty import set and a per-click instruction budget.

This milestone covers the single-file Counter workflow. Modules, user-defined functions and types, editable text, async tasks, network permissions, and the formatter are future work.

## Vision

Solia explores a new application platform that does not depend on the HTML/CSS/JavaScript stack or a conventional browser engine.

The platform is intended to combine:

- The distribution and sandboxing model of mini-app platforms.
- A statically typed language with inference and compiler-visible program structure.
- Declarative, reactive UI instead of a DOM and CSS cascade.
- A shared runtime for conventional interfaces and real-time 2D/3D experiences.
- AI, agents, multimodal input, and typed tools as first-class platform capabilities.
- A Rust-based native host with a portable WebAssembly application target for the first release.

The ambition is not to reproduce the Web platform. Solia deliberately starts with a different application model and a smaller, purpose-built API surface.

## Design principles

1. **Application-first:** an application is the primary artifact; a single source file should be enough for a small app, while larger projects can use modules and packages.
2. **Static types by default:** types are checked before execution, with local inference and explicit boundaries for dynamic data.
3. **Declarative UI:** UI structure and state dependencies are described, not manually manipulated through a DOM.
4. **One application model:** UI, game scenes, asynchronous work, AI tasks, resources, and permissions belong to one application lifecycle.
5. **AI-native, provider-neutral:** models and agents are runtime capabilities, not assumptions tied to one vendor.
6. **Secure by default:** applications receive explicit capabilities through a host-controlled permission model.
7. **Portable first, native where needed:** the first application backend is WebAssembly; the compiler architecture should remain independent of any single backend.
8. **Accessible by design:** semantic UI, keyboard navigation, focus, text input, and assistive-technology support are foundational requirements.
9. **AI-friendly development:** syntax, diagnostics, metadata, and tooling should be easy for both people and coding agents to inspect and modify.

## A first look at the language

The following is an illustrative syntax sketch. It demonstrates the intended style; it is not yet a formal grammar.

```solia
app Counter {
    config {
        title: "Counter"
        window: (800, 600)
        theme: dark
    }

    state {
        count: i32 = 0
    }

    view {
        Column(gap: 16) {
            Text("Count: {count}")

            Button("Increment") {
                count += 1
            }
        }
    }
}
```

A single `.solia` source file should be able to describe a small application. Larger applications will be split into modules, reusable components, and packages without changing the underlying programming model.

## Language design outline

### 1. Type system

The proposed type system is statically checked and designed to preserve useful type information through the compiler pipeline.

**Primitive types**

- `bool`
- Signed and unsigned integers: `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, `u64`
- Floating-point values: `f32`, `f64`
- `String`, `Bytes`
- Time-related types such as `Duration` and `Timestamp`

**Composite types**

- `struct`, `enum`, tuple, array, list, and map
- `Option<T>` and `Result<T, E>`
- Functions, generics, and interface/trait-style abstractions

**Type-system goals**

- Local type inference without making public APIs ambiguous.
- Pattern matching and exhaustiveness checking.
- Explicit conversions; no surprising implicit coercions.
- A clear boundary for dynamic or untrusted data such as JSON and model output.
- Compiler-visible object layouts and function signatures where they can be proven.

Type information is retained in typed compiler IR for analysis and optimization. It does not imply that every type must remain represented at runtime or in the final WebAssembly binary.

### 2. Memory and resource management

Ordinary application developers should not need to manage memory manually.

The current direction is:

- Managed memory for ordinary application values.
- Efficient contiguous buffers for large arrays, images, audio, and binary data.
- Arena-style allocation where short-lived data makes it useful.
- Opaque, host-managed handles for GPU, audio, window, and other native resources.

The exact collector, allocation strategy, and low-level escape hatches remain open design questions. Language semantics should not be tied to WebAssembly's current memory implementation.

### 3. Declarative UI and reactive state

UI is a language-level concept, not HTML rendered inside an embedded browser.

The proposed UI pipeline has four related parts:

1. **View tree:** the declarative structure of the interface.
2. **Reactive graph:** dependencies between application state, derived values, and UI properties.
3. **Layout engine:** measurement, constraints, text layout, and placement.
4. **Render engine:** display lists/render graphs and GPU execution.

State changes should update only the affected parts of the reactive graph. The runtime should avoid requiring developers to manually mutate a tree or perform application-level DOM diffing.

The initial standard UI vocabulary may include text, images, buttons, text fields, rows, columns, grids, stacks, lists, scroll views, and embedded scene views.

**Accessibility is part of the model.** Semantic roles, accessible labels, focus management, keyboard navigation, screen-reader exposure, text selection, IME input, bidirectional text, and RTL support must be designed into the UI runtime rather than added as an afterthought.

### 4. Functions, effects, and capabilities

The language should make external effects visible to the compiler and runtime.

An illustrative declaration might look like:

```solia
function read_text(path: Path) -> Result<String>
    requires filesystem.read
```

Applications declare the capabilities they need. The host decides whether to grant them and mediates access.

Candidate capabilities include local storage, selected-file access, network access, AI inference, audio input/output, camera, microphone, notifications, and game input. Network permissions should be narrow where practical (for example, host-specific rather than unrestricted access).

### 5. Asynchronous work and concurrency

The language is expected to support:

- `async` / `await`
- Futures and streams
- Structured concurrency and task scopes
- Cancellation and timeouts
- Event-driven UI work
- Background tasks

UI events, real-time game updates, and AI/network operations have different scheduling needs. They should share the application lifecycle and resource model without being forced into one execution loop.

The first version can use cooperative asynchronous tasks. Parallel WebAssembly threads are not an MVP requirement.

### 6. AI as a first-class capability

AI should be integrated through a provider-neutral runtime API rather than being hard-coded into the language around one model vendor.

Candidate standard concepts include:

- Models and inference sessions
- Typed messages and multimodal content blocks
- Streaming output and cancellation
- Structured output
- Typed tools with declared inputs and outputs
- Agent execution, task state, and context
- Budgets for steps, time, and token usage

Illustrative tool declaration:

```solia
tool search_books(query: String) -> Result<List<Book>>
```

Illustrative agent declaration:

```solia
agent ReadingAssistant {
    model: runtime.ai.default
    tools: [search_books]
    max_steps: 8
}
```

Model credentials should be managed by the host or an explicitly configured provider, not embedded as secrets in an application package. Tool permissions, data access, execution limits, and user confirmation must be enforced by the runtime. Static types help validate tool contracts, but they do not by themselves solve prompt injection or model-trust problems.

### 7. Game and real-time capabilities

Games should be applications within the same platform, not a separate runtime with an unrelated UI system.

The longer-term game model may include:

- Scenes, entities, components, and systems (ECS)
- Transforms, cameras, sprites, and materials
- Animation and state machines
- Physics and collision queries
- Audio and spatial audio
- Particles and effects
- Input mapping and fixed-timestep updates
- 2D and 3D rendering

The UI and game runtime should share the window, input, resource, rendering, and application lifecycle infrastructure. Their scheduling models can remain distinct: reactive/event-driven updates for UI and fixed-timestep/frame updates for simulation.

The MVP should prove the shared model with basic UI and a small 2D scene before attempting a full 3D engine.

## Compiler and runtime architecture

### Compiler pipeline

The compiler should have a language-focused front end and backend-independent intermediate representations.

```text
.solia source
    ↓
Lexer / Parser
    ↓
AST
    ↓
Name and module resolution
    ↓
Type checking + effect checking
    ↓
Typed HIR
    ↓
Typed, backend-independent IR
    ↓
Optimization and lowering
    ↓
WebAssembly backend (v1)
    ↓
Application package (.sapp, tentative)
```

The typed IR is a strategic boundary. It should preserve the information needed for type checking, data-layout analysis, specialization, UI dependency analysis, and future backend work. The first backend is WebAssembly, but the language front end and core semantics should not be designed as wrappers around WASM instructions.

### First execution target: WebAssembly

The first release will target WebAssembly and run inside a Rust-based native host.

- **Application code:** compiled to WebAssembly.
- **Host runtime:** Rust, responsible for windows, input, rendering, storage, networking, AI provider integration, and native resources.
- **Isolation:** the host exposes only explicitly granted capabilities to an application.
- **Host ABI:** use versioned interfaces; evaluate WIT and the WebAssembly Component Model for capability boundaries and component interoperability.
- **Performance:** batch calls and data exchange across the WASM/host boundary. Avoid one host call per entity, widget, or tiny operation in hot loops.

WebAssembly is the first distribution and isolation target, not the definition of the language. Future native or GPU-oriented backends may be added without changing the source language or typed IR.

### Runtime layers

The native host is expected to contain coordinated subsystems:

- UI runtime and layout/render pipeline
- Game runtime and ECS scheduler
- AI runtime and provider adapters
- Capability and permission manager
- Resource and asset manager
- Async task scheduler
- Platform integration (windowing, input, filesystem mediation, audio, networking)

These subsystems share application lifecycle, resource ownership, diagnostics, and security policy.

## Application packaging and distribution

Tentative file conventions:

- `.solia`: source file
- `.sapp`: distributable application package

The final package format is not yet specified. A package is expected to include a manifest, compiled WebAssembly module(s), assets/resources, runtime compatibility metadata, and a signature or integrity information.

The longer-term platform may provide installation, updates, caching, permissions, app discovery, and app-to-app communication. An application store is a later platform milestone, not an MVP dependency.

## Initial technology direction

These are candidates for prototyping, not mandatory dependencies or final decisions.

| Area | Candidate / direction | Notes |
|---|---|---|
| Compiler and host | Rust | Shared implementation language for the compiler and native runtime |
| WASM execution | Wasmtime | Embedded WebAssembly runtime |
| WASM interfaces | WIT / Component Model | Evaluate for a versioned host ABI |
| GPU abstraction | wgpu | Cross-platform GPU access |
| ECS | Bevy ECS or a compatible design | Prototype before committing to a public API |
| Physics | Rapier | Candidate for later game capabilities |
| Audio | Kira or another Rust audio engine | Evaluate latency, platform support, and licensing |
| Editor tooling | LSP, CLI, formatter, diagnostics | Build incrementally |

## MVP scope

The first vertical slice should prove that a real application can be authored in Solia, compiled to WASM, packaged, and run on Windows, macOS, and Linux without HTML, CSS, JavaScript, or a browser engine.

### MVP must-haves

1. A small, documented single-file syntax.
2. Lexer, parser, diagnostics, and formatter basics.
3. Static types, inference, structs, enums, and functions.
4. Modules/imports for code beyond a single file.
5. A typed IR independent of the WASM backend.
6. WASM compilation and execution in the Rust host.
7. A minimal native window and event loop.
8. Declarative UI with reactive state and basic layout.
9. Text rendering, text input, IME, focus, and accessibility foundations.
10. A minimal capability/permission system.
11. Async tasks and a basic network interface.
12. Application packaging and runtime compatibility checks.

### After the first vertical slice

- AI provider interface, streaming, and typed tools.
- Local storage and asset management.
- Basic 2D scene rendering and input.
- Hot reload and developer preview.
- More complete accessibility and international text support.

### Explicitly out of scope for the first release

- HTML/CSS/JavaScript compatibility
- A general-purpose web browser engine
- Full 3D engine and advanced physics
- Multiplayer/networked game infrastructure
- Arbitrary native plugins for third-party apps
- A complete public app store
- Mobile support as a launch requirement
- A JIT compiler or native backend

## Suggested repository layout

As the implementation grows, the repository may evolve toward:

```text
solia/
├── README.md
├── docs/
│   ├── language-overview.md
│   ├── syntax-and-grammar.md
│   ├── type-system.md
│   ├── memory-model.md
│   ├── compiler-architecture.md
│   ├── runtime-and-host-abi.md
│   ├── ui-model.md
│   ├── ai-runtime.md
│   ├── game-runtime.md
│   └── roadmap.md
├── examples/
│   ├── counter/
│   └── ai-virtual-pet/
├── compiler/
├── runtime/
├── standard-library/
└── tools/
```

This is a proposed organization, not a requirement to create all directories immediately.

## Roadmap

### Phase 0 — Language and architecture RFCs

- Confirm language identity, syntax principles, and naming.
- Specify core type semantics and error handling.
- Define memory/resource semantics at the language level.
- Specify the reactive UI model and accessibility contract.
- Define typed IR boundaries and the first WASM host ABI.
- Threat-model application capabilities and sandbox limits.

### Phase 1 — Compiler and runtime vertical slice

- Implement a minimal parser and type checker.
- Lower a small language subset to typed IR.
- Compile to WASM and execute in the Rust host.
- Render a native window with text and a button.
- Demonstrate reactive state updates.
- Package and run the Counter example on desktop platforms.

### Phase 2 — Application capabilities

- Modules, async tasks, storage, networking, and permissions.
- LSP, formatter, diagnostics, hot reload, and developer preview.
- First AI provider interface and typed tool calls.

### Phase 3 — Interactive and game capabilities

- 2D scenes, input, animation, audio, and basic physics integration.
- Shared UI/game window and resource lifecycle.
- AI-controlled characters or agents as a demonstration application.

### Phase 4 — Platform and ecosystem

- Signed packages, updates, discovery, and app distribution.
- More complete 3D capabilities.
- Evaluate native and GPU-oriented backends based on measured needs.
- Evaluate mobile support.

## Open design questions

The following decisions should be resolved through small RFCs and working prototypes rather than prematurely frozen in this README:

- Exact grammar, keywords, and component syntax.
- Structural versus nominal typing and the role of traits/interfaces.
- Generic implementation strategy and specialization rules.
- Memory collector and resource lifetime model.
- Error handling and effect-system syntax.
- Whether UI descriptions are compiled into a static graph, runtime graph, or a hybrid.
- Text shaping, layout, and accessibility API contracts.
- Core WASM module versus Component Model packaging boundaries.
- Async scheduling and future multithreading model.
- Stable host ABI and application package format.
- AI provider, secret, and tool permission model.
- 2D/3D renderer and ECS integration boundaries.

## Non-goals

Solia is not intended to be:

- A drop-in replacement for websites or a renderer for existing web content.
- A JavaScript engine with a different syntax.
- A thin wrapper around HTML, CSS, and DOM APIs.
- A complete Unity, Unreal, or Godot competitor in its first release.
- A language whose semantics are permanently tied to WebAssembly.

## Contributing and project status

Solia has an initial desktop prototype. Early contributions can exercise the Counter workflow, extend the checked language subset, or explore the open design questions. The language syntax, runtime ABI, package format, and implementation choices may change as prototypes reveal trade-offs.

Before implementation APIs stabilize, please treat all examples and names in this README as provisional.

---

*Solia — rethink how applications are described, compiled, executed, and distributed.*
