# ctrlsys — Roadmap

Ordering principle: **make the gates real, then fix what they catch, then simplify.**
Soundness fixes come before feature work, because every later phase's correctness
argument rests on them.

Each phase is small enough to land in one sitting. A phase is done when its
**Done when** items hold and the four gates in `tech-stack.md` are green.

Written against `3fe6860`. The slice-per-link design introduced in `0baca90` and
`3fe6860` removed an entire class of planned work — composite layout, offset
computation and input-coverage checking are all unnecessary when each input is its own
slice. Phases that existed to solve those problems have been replaced by a single
deletion phase.

---

## Phase 0 — Gates before work

> 🚧 In progress — see [specs/2026-07-30-gates-before-work/](2026-07-30-gates-before-work/) (branch `feature/gates-before-work`)

Nothing functional changes. Establishes the checks every later phase is measured by.

- Empty `[dependencies]`; remove `zerocopy` (already unused by the current code).
- Add dev-dependencies (`proptest`, `trybuild`, `approx`).
- Add the lint block from `tech-stack.md` to `src/lib.rs` — all six unsafe-related
  lints, not just `undocumented_unsafe_blocks`.
- `tests/safety_docs.rs` is in the repo and **currently fails on 7 declarations** in
  `src/system/mod.rs` (lines 55, 60, 68, 74, 85, 100, 119). Write their contracts:
  what a caller must guarantee about slice count, slice length, and alignment.
- Clear the ~24 clippy warnings, including the missing `# Panics` on
  `AlignedBuffer::new` and the missing `# Errors` on `simulate`.

**Done when:** `cargo clippy --all-targets -- -D warnings` is clean and
`cargo test --test safety_docs` passes.

---

## Phase 1 — Aligned buffers ✅ done in `0baca90`

`AlignedBuffer` allocates per system with `alloc_zeroed` at the alignment reported by
`RawSystem::output_alignment()`. This closed the original "unaligned reference"
UB — leaf casts in `from_slices` are now aligned by construction.

Residual, folded into Phase 0: `AlignedBuffer::new` has no `# Panics` section despite
`.expect()` on the layout, and its three `unsafe` blocks have no `// SAFETY:` comments.

---

## Phase 2 — Close the two live UB sites

Both are Article 1 violations. Neither is subtle once located.

**`src/pool/mod.rs:79-84` — Miri-confirmed.**

```
error: Undefined Behavior: trying to retag from <34148> for Unique permission
       at alloc12940[0x0], but that tag only grants SharedReadOnly permission
```

`let output_buf = &output_buffers[idx]` takes a *shared* borrow; casting its `as_ptr()`
to `*mut u8` does not grant write permission, so `from_raw_parts_mut` retags a
SharedReadOnly tag as Unique. `output_buffers` is not even declared `mut`. The fix is
to derive the raw pointer from a `&mut` once, before the loop, and take both the shared
input borrows and the unique output borrow from that pointer — the pattern the
pre-`pool/` code used, which Miri accepted under both Stacked and Tree Borrows. The
existing `assert!(!from.ids().contains(&to.id()))` in `link` is what makes it sound,
and the `// SAFETY:` comment must say so.

**`src/system/discrete/mod.rs:61-62` — found by reading, not yet reachable by Miri.**

```rust
let output_bytes = &output as *const Self::Output<'s>;
let output_clone = unsafe { output_bytes.read() };
```

`Output` is typically `&'a mut T`, so `ptr::read` duplicates a mutable reference. The
duplicate is passed to `calculate` while the original is still live and used
afterwards by `payload_ref()` and `update_output()` — two aliasing `&mut` to the same
place. Miri aborts on the `pool` error first, so this is unconfirmed; it should be
re-checked once Phase 2's first item lands.

**Done when:** `cargo +nightly miri run` completes the demo with no diagnostics.

---

## Phase 3 — Solver loop hardening

- `assert!` that each block's returned `next_time` is strictly greater than `time`,
  naming the system index and both values.
- `let inp_max = links_to_node.iter().map(...).max().unwrap()` panics on an empty
  pool. Handle zero systems.
- Clamp the final step so the loop ends exactly at `total_time`.
- Promote the arity checks in `from_slices` from `debug_assert!` to `assert!` — link
  count versus input arity is a user contract (Article 4), not an internal invariant.
- Document the in-out `output` contract on the `System` trait.

**Done when:** a block returning a non-monotonic `next_time` panics with a message
naming it, covered by a `#[should_panic]` test.

---

## Phase 4 — Delete the vestigial offset machinery

Slice-per-link made the flat-buffer bookkeeping unnecessary. The compiler already
agrees: `fields to_input_offset and num_bytes are never read`.

- Delete `SystemLayout`, `SystemLink::to_input_offset`, `SystemLink::num_bytes`, and
  `SystemOut::layouts`.
- `SystemMux` reduces to an ordered list of producer ids.
- This also removes a latent bug rather than requiring it to be fixed:
  `src/pool/link.rs:141`, in `From<(&T, &U, &V)>`, builds `ids` from `t` and `u` only —
  `v.ids()` is missing. Since `layouts` gets three entries and `ids` gets two,
  `add_links_to` would index `self.ids[2]` and panic. Deleting layouts removes the
  mismatch; the `ids` chain still needs `v` added.
- Reconsider `has_unique_elements` in `SystemPool::link` — it forbids muxing one
  producer into two inputs of the same consumer, which is legal and useful.

**Done when:** `cargo clippy` reports no dead fields and a three-way mux links
correctly, covered by a test.

---

## Phase 5 — `graph.rs` correctness

Tests first — the ordering bug is silent, so the test must fail before it passes.

- Unit tests over hand-built adjacency lists: topological order, self-loops, multi-node
  SCCs, `AlgebraicLoop` rejection, empty graph.
- Fix intra-SCC ordering: members are sorted by node index, so a chain `a → b → c`
  inside an SCC executes in index order and silently introduces a one-step delay. Cut
  the loop-breaking blocks' outgoing edges, then topologically sort the rest.
- Delete the condensation + Kahn pass; Tarjan already emits reverse topological order.
- `OrderError`: implement `Display` and `std::error::Error`.
- Property test: random DAGs, assert every link's source precedes its target.

**Done when:** the ordering test fails on the old implementation and passes on the new.

---

## Phase 6 — Analytical golden tests

- A first-order lag driven by a step, asserted against `1 - e^(-t/τ)` within tolerance.
- A rate-independence test: the same block in a model containing an unrelated faster
  block must produce the same trajectory. **This currently fails** — 0.431 vs 0.994 at
  t = 1.0 — and is the executable form of Article 6.
- Rewrite `Filter` in `src/main.rs` to the rate-independent template: derive `dt` from
  its own stored last-update time.

**Done when:** both tests pass.

---

## Phase 7 — Discrete-system hardening

`HeldSystem` and the `Holder` implementations are new and not yet covered by tests.

- `FirstOrderHold::update_output` computes `prop = t_base / dt` where
  `dt = curr_time - last_time`. On the first sample both are equal, so `dt == 0` and
  the result is NaN. The `debug_assert!(t_base < dt)` above it does not catch this and
  is compiled out in release regardless.
- `HeldSystem::update` advances `self.last_time += req_dt` then returns
  `self.last_time + 2.0 * req_dt`. Confirm whether the factor of two is intended; the
  next event after an update at `last_time` would normally be `last_time + req_dt`.
- Tests: a ZOH-held discrete block sampled at a known rate produces a staircase; an
  FOH-held block interpolates linearly between samples.

**Done when:** both holders are covered by tests and no path divides by a zero step.

---

## Phase 8 — Block contract completion

- Rename `is_stateful` to `breaks_algebraic_loops`, default `false`. Document the
  distinction: a block may have state *and* direct feedthrough (a PID with a
  proportional term), and only the latter matters for loop breaking.
- Add `initialize(&mut self, output: …)` so initial conditions are expressible without
  relying on the zero bit pattern.
- Document that per-*port* feedthrough granularity is a known coarseness.

**Done when:** a closed loop containing a stateful-but-feedthrough block is correctly
rejected as an algebraic loop.

---

## Phase 9 — Probes and recording

- `Recorder` trait; a `Vec`-backed implementation for tests and an `mpsc::Sender`
  implementation for live consumers.
- `pool.probe(&sys) -> Receiver<Sample<T>>` as the ergonomic front end.
- Probes registered before `simulate`; document the unbounded-growth caveat.

**Done when:** the golden tests read trajectories from a `Vec` recorder rather than
from `println!`.

---

## Phase 10 — Block library seed

Enough blocks to demonstrate the design, not a comprehensive library.

- `Gain`, `Sum`, `Saturation`, `Integrator`, `Step` (reusing the existing
  `ZeroOrderHold` and `FirstOrderHold`).
- A closed-loop PI-controlled first-order plant as the headline example, asserted
  against the analytical solution.

**Done when:** the PI example runs, is numerically correct, and is the crate-level doc
example.

---

## Deferred

Not scheduled. Each is a stated non-goal in `mission.md`; listed here so the reason is
findable from the roadmap.

| Item | Why deferred | What it would need |
|---|---|---|
| State events (zero-crossing detection) | Blocks cannot predict state-dependent crossings; only the solver can bisect | `zero_crossing_signals()` hook plus step bisection |
| Step rejection / global error control | Blocks integrate internally | `save_state` / `restore_state`, which conflicts with in-out `output` |
| Per-port direct feedthrough | System-level granularity is sufficient for v1 | Port-level dependency edges in `graph.rs` |
| Mux **outputs** | `output_ref_buffer` is hard-coded to one slice; single outputs cover current needs | `SystemDataOut` impls for tuples of `&mut`, plus output-side link routing |
| Higher mux arities | 2 and 3 cover current needs | More `From<(…)>` impls, or a macro over arities |
| `no_std` | `std` chosen deliberately | Replace `HashSet`, `Box<dyn>` strategy, probe transport |
| Matrix / state-space blocks | Out of dissertation scope | `nalgebra` or `faer` |
