# AGENTS.md — Instructions for Coding Agents

This file contains operational instructions for agents working on Rustique.

For architecture and product intent, read `DESIGN.md`.

For implementation order and current progress, read `PLAN.md`.

## 1. Start Every Task by Reading Context

Before changing code:

1. Read the relevant sections of `DESIGN.md`.
2. Read the relevant phase in `PLAN.md`.
3. Inspect the existing implementation before proposing changes.
4. Do not assume unfinished phases are available.

Do not reread unrelated design sections unless needed.

---

## 2. Work on One Scoped Task at a Time

Prefer the smallest coherent change that satisfies the requested task.

Do not:

- implement future phases speculatively
- refactor unrelated modules
- add frameworks that are not needed yet
- build UI when the task concerns the renderer
- redesign architecture without an explicit reason

If the user asks to implement a phase, implement only that phase unless dependencies require a small prerequisite.

---

## 3. Update PLAN.md

When completing work described in `PLAN.md`:

- change completed items from `[ ]` to `[x]`
- use `[-]` for work that is partially completed
- use `[!]` for genuinely blocked work
- add a short note below a checkbox when the result needs explanation

Never mark acceptance criteria complete unless they actually pass.

Do not rewrite the entire plan.

---

## 4. Architectural Rules

These rules are mandatory unless the user explicitly changes the design.

- Rust owns simulation state.
- GPU owns particle state whenever practical.
- Tauri / React must not own engine logic.
- `render-core` must not depend on Tauri.
- The renderer must work without a window.
- The headless CLI and desktop editor must share the same render core.
- Simulation must not depend on wall-clock time.
- Simulation must not depend on render resolution.
- Audio analysis must be precomputed for offline rendering.
- Presets and mappings should be serialized data where practical.
- Cloud rendering must consume the same project representation as local rendering.

---

## 5. Platform Rules

Primary development platform:

- Windows
- NVIDIA RTX 4070 12 GB
- 64 GB system RAM

Target headless platform:

- Linux / WSL
- Vulkan
- Docker / RunPod

Core crates must remain cross-platform.

Avoid Windows-only dependencies in:

- `render-core`
- `simulation`
- `audio-engine`
- `project-format`
- `exporter`
- `particle-render`

If platform-specific behavior is unavoidable, isolate it clearly behind platform-specific modules or `cfg(...)`.

---

## 6. Rust Style

Prefer clear, boring Rust over clever Rust.

Use:

- explicit structs and enums
- `Result` for fallible operations
- structured error types where useful
- small modules
- descriptive names
- comments for GPU synchronization and memory layout

Avoid:

- unnecessary macros
- unnecessary generic abstractions
- unnecessary traits
- premature ECS architecture
- unnecessary `unsafe`
- complex lifetime tricks when ownership can be simplified

If `unsafe` is introduced, explain why it is required and document its safety invariants.

---

## 7. GPU Rules

- Do not create large per-particle CPU objects.
- Do not read particle buffers back to CPU during normal simulation.
- Reuse GPU buffers and textures.
- Avoid per-frame GPU resource allocation where practical.
- Prefer compute shaders for large parallel simulation work.
- Keep Rust and WGSL buffer layouts explicitly synchronized.
- Document buffer layouts.
- Consider alignment and padding when modifying GPU structs.
- Avoid naive O(N^2) particle interactions.
- Profile before optimizing.

When adding a GPU feature, include enough logging or diagnostics to debug adapter/backend/resource failures.

---

## 8. Determinism Rules

Offline rendering must be deterministic to the practical extent possible.

Use:

```text
simulation_time = frame_index / fps
```

Do not use elapsed wall-clock time for offline simulation.

Random behavior must derive from stored seeds or deterministic hashes.

Preview quality settings may reduce count or rendering quality, but should not arbitrarily change the core scene behavior.

---

## 9. Error Handling

Return useful errors.

Errors should include context such as:

- operation attempted
- file/path where relevant
- GPU operation where relevant
- FFmpeg exit information where relevant

Do not silently ignore failures.

Avoid `unwrap()` and `expect()` in library/runtime code unless the invariant is genuinely guaranteed and documented.

They are acceptable in narrow tests and disposable prototypes.

---

## 10. Dependencies

Before adding a dependency:

1. Check whether the standard library or an existing dependency already solves the problem.
2. Prefer mature and actively maintained crates.
3. Avoid adding large frameworks for small tasks.
4. Briefly explain significant new dependencies in the task summary.

Do not upgrade unrelated dependencies unless required.

---

## 11. Validation

After code changes, run the narrowest relevant checks.

At minimum, when practical:

```bash
cargo fmt --check
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Also run relevant tests or binaries for the task.

Examples:

```bash
cargo test --workspace
cargo run -p particle-render -- --gpu-info
```

Do not claim something works unless it was actually validated or clearly state that it was not run.

---

## 12. Performance Work

Do not optimize based only on intuition.

When performance is part of the task:

- measure before
- make one meaningful change
- measure after
- record results when the plan asks for benchmarks

Distinguish between:

- CPU cost
- GPU compute cost
- vertex cost
- fragment/overdraw cost
- memory bandwidth
- GPU readback
- encoding cost

---

## 13. Project Format

Project and preset schemas are long-lived interfaces.

When modifying them:

- preserve explicit versioning
- avoid silent breaking changes
- add migration logic when required
- keep defaults explicit
- add representative test fixtures

Do not serialize temporary GPU/runtime state.

---

## 14. Shader Changes

When adding or changing WGSL:

- keep shaders focused
- avoid giant all-purpose shaders unless proven necessary
- document non-obvious math
- keep binding layouts obvious
- keep Rust-side bindings in sync
- validate shader compilation
- avoid magic numbers where a uniform/config value belongs

---

## 15. Tauri / React Rules

The frontend is an editor, not the engine.

React may manage:

- UI state
- inspector state
- timeline state
- preset selection
- project editing
- render job configuration

React must not implement:

- particle simulation
- final audio analysis
- final frame rendering
- GPU resource ownership
- FFmpeg export logic

Keep the native command boundary small and explicit.

---

## 16. Headless First

For engine features, prefer this order:

1. implement in headless/core path
2. validate via CLI or tests
3. expose to desktop UI afterward

Do not make engine correctness dependent on Tauri being open.

---

## 17. Token-Conscious Agent Behavior

Keep responses concise and action-oriented.

Do not:

- restate `DESIGN.md`
- restate entire phases from `PLAN.md`
- narrate obvious code
- provide long tutorials unless asked
- generate large speculative implementations

When finishing a task, summarize only:

- what changed
- files changed
- checks run
- remaining blocker, if any

---

## 18. Git Discipline

Do not modify unrelated generated files.

Do not commit secrets, local paths, credentials, large render outputs, or build artifacts.

Suggested commit scope should map to one task or one coherent phase slice.

Do not rewrite Git history unless explicitly asked.

---

## 19. When Requirements Conflict

Priority order:

1. Explicit latest user instruction
2. `AGENTS.md`
3. `DESIGN.md`
4. `PLAN.md`
5. Existing implementation conventions

If a requested change conflicts with architecture, point out the conflict briefly and propose the smallest compatible solution.

---

## 20. Definition of Done

A coding task is done only when:

- requested behavior is implemented
- relevant code is formatted
- relevant checks pass
- tests/commands were run where practical
- `PLAN.md` is updated if the task corresponds to a checklist item
- no unrelated changes were introduced
