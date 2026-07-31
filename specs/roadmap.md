# ctrlsys — Roadmap

Ordering principle: **make the gates real, then fix what they catch, then simplify.**
Soundness fixes come before feature work, because every later phase's correctness
argument rests on them.

Each phase is small enough to land in one sitting. A phase is done when its
**Done when** items hold and the four gates in `tech-stack.md` are green.

**Renumbered after Phase 0.** Probes and recording moved from Phase 9 to Phase 5, and
the port split was folded into Phase 4; old Phases 5–8 shifted up by one, Phase 10 kept
its number. Observation has to exist before the phases that test values, and
`cargo +nightly miri test` currently runs no simulation at all, so the Miri gate is
gating nothing until a recorded trajectory test exists. Phase numbers cited elsewhere
in the repo — including `#[expect]` reason strings — now name the phase by title too,
because this renumber silently invalidated three of them.

Written against `3fe6860`. The slice-per-link design introduced in `0baca90` and
`3fe6860` removed an entire class of planned work — composite layout, offset
computation and input-coverage checking are all unnecessary when each input is its own
slice. Phases that existed to solve those problems have been replaced by a single
deletion phase.

---

## Phase 0 — Gates before work ✅ done in `a413e05`

Spec: [specs/2026-07-30-gates-before-work/](2026-07-30-gates-before-work/).

Nothing functional changed — the trajectory is bit-identical across 2016 samples.
`[dependencies]` is empty, the eight-lint block is in `src/lib.rs`, the 7 `# Safety`
contracts in `src/system/mod.rs` are written, and `fmt` / `clippy -D warnings` /
`test` are all green.

Two things it turned up that later phases depend on:

- The estimate of "~24 clippy warnings" was low. The six unsafe lints had never been
  run; with the full block denied in `lib.rs` they produce **22 hard errors**. Nine
  `#[expect]` markers remain, each naming the phase that removes it.
- **`cargo +nightly miri test` exercises no simulation at all** — the only test in the
  tree reads source files. The Miri gate currently works only via `miri run` over
  `src/main.rs`. Phase 5 is what makes it real.

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

## Phase 4 — Delete the vestigial offset machinery, split the port handles

Two changes to the same types, done in one pass because doing them separately means
editing `SystemRef`, `SystemIn` and `SystemOut` twice.

### Deletion

Slice-per-link made the flat-buffer bookkeeping unnecessary. The compiler already
agrees: `fields to_input_offset and num_bytes are never read`.

- Delete `SystemLayout`, `SystemLink::to_input_offset`, `SystemLink::num_bytes`, and
  `SystemOut::layouts`. Phase 0 left `#[expect(dead_code, …)]` on the two fields; those
  attributes go with them, and the expectation firing is what proves the deletion
  landed.
- `SystemMux` reduces to an ordered list of producer ids.
- Phase 0 also left `#[expect]` on the two `addr_of!` blocks in `link.rs` — the whole
  `MaybeUninit` offset computation disappears here.
- This also removes a latent bug rather than requiring it to be fixed:
  `src/pool/link.rs:163`, in `From<(&T, &U, &V)>`, builds `ids` from `t` and `u` only —
  `v.ids()` is missing. Since `layouts` gets three entries and `ids` gets two,
  `add_links_to` would index `self.ids[2]` and panic. Deleting layouts removes the
  mismatch; the `ids` chain still needs `v` added.
- Reconsider `has_unique_elements` in `SystemPool::link` — it forbids muxing one
  producer into two inputs of the same consumer, which is legal and useful.

### Split ports

`add_system` returns one handle that implements both `SystemIn` and `SystemOut`, so
nothing distinguishes a system's input side from its output side. Split it:

```rust
let (f_i, f_o) = pool.add_system(Filter);
pool.link(s_o, &f_i);
```

The payoff is in Phase 5: `probe` takes an *output* port, so probing an input becomes a
compile error rather than something that type-checks and means nothing.

**What this does not buy, so it must not be claimed:** self-feed stays a runtime check.
`pool.link(f_o, &f_i)` for the same system still type-checks, because ids are runtime
values. That `assert!(!from.ids().contains(&to.id()))` is load-bearing for soundness —
every `// SAFETY:` comment written in Phase 0 cites it — and per Article 2 it keeps its
comment explaining why it cannot be a compile-time check.

**Done when:** `cargo clippy` reports no dead fields, and a three-way mux builds the
correct `ids` chain, covered by a test that inspects the constructed links. That test
is *structural*, not numerical — asserting the values actually flow through a
three-way mux needs a probe, and belongs to Phase 5.

---

## Phase 5 — Probes and recording

Moved ahead of the test-writing phases (was Phase 9). Four later phases need to observe
signal values — the mux trajectory above, the golden tests, the ZOH/FOH staircase
tests, and the PI example — and the old ordering had Phase 6 written against `println!`
and retrofitted afterwards.

There is a second reason. `cargo +nightly miri test` currently exercises **no
simulation at all**: the only test in the tree reads source files. Miri is one of the
four merge gates and is presently gating nothing except by way of `miri run` over
`src/main.rs`. A recorded trajectory test is what makes that gate real.

Depends on Phase 2 (probes ride the same `raw_update` path, so their tests are
meaningless while Miri aborts on the UB) and on Phase 4 (mux probing builds on
`SystemMux`, and `probe` takes a split output handle).

- `Recorder` trait; a `Vec`-backed implementation for tests and an `mpsc::Sender`
  implementation for live consumers.
- `pool.probe(&f_o) -> Receiver<Sample<T>>` as the ergonomic front end, taking an
  output port rather than a system.
- Mux probing: `pool.probe((&f_o, &s_o)) -> Receiver<Sample<(T, U)>>`, reusing the
  `From<(&T, &U)>` conversion that already builds a `SystemMux`.
- `probe` behaves as a continuous block, recording every step. It returns
  `f64::INFINITY`, so it contributes nothing to the `next_time` minimum and **cannot
  change the step sequence** — non-invasive by construction. Add a test that asserts
  this rather than trusting the argument: a model's trajectory must be unchanged by
  attaching a probe to it.
- Probes registered before `simulate`; document the unbounded-growth caveat.

**Deferred within this phase: `pool.sample(&f_o, timestep)`.** A fixed-interval sampler
requests time events, which changes the solver's step sequence — the exact mechanism
behind the precedent recorded in `mission.md` Article 5, where an unrelated block moved
a step response from 0.431 to 0.994. Until rate-independence is fixed (Phase 7), a
sampler can change the numbers it exists to measure. Revisit once Phase 7 lands; if it
ships before then it needs a loud caveat in its own docs.

**Done when:** a simulation test records a trajectory through a `Vec` recorder and
asserts against it, `cargo +nightly miri test` runs that test, and attaching a probe to
a model provably does not alter its trajectory.

---

## Phase 6 — `graph.rs` correctness

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

## Phase 7 — Analytical golden tests

Reads trajectories through Phase 5's `Vec` recorder, not `println!`.

- A first-order lag driven by a step, asserted against `1 - e^(-t/τ)` within tolerance.
- A rate-independence test: the same block in a model containing an unrelated faster
  block must produce the same trajectory. **This currently fails** — 0.431 vs 0.994 at
  t = 1.0 — and is the executable form of Article 6.
- Rewrite `Filter` in `src/main.rs` to the rate-independent template: derive `dt` from
  its own stored last-update time.
- Once rate-independence holds, revisit `pool.sample(&f_o, timestep)`, deferred out of
  Phase 5 because a fixed-interval sampler perturbs the step sequence.

**Done when:** both tests pass.

---

## Phase 8 — Discrete-system hardening

`HeldSystem` and the `Holder` implementations are new and not yet covered by tests.

- `FirstOrderHold::update_output` computes `prop = t_base / dt` where
  `dt = curr_time - last_time`. On the first sample both are equal, so `dt == 0` and
  the result is NaN. The `debug_assert!(t_base < dt)` above it does not catch this and
  is compiled out in release regardless.
- `HeldSystem::update` advances `self.last_time += req_dt` then returns
  `self.last_time + 2.0 * req_dt`. Confirm whether the factor of two is intended; the
  next event after an update at `last_time` would normally be `last_time + req_dt`.
- Phase 0 left `#[expect(clippy::float_cmp, …)]` on the two `f64::MIN` sentinel tests,
  in `holder.rs` and `discrete/mod.rs`, deferred here because changing how
  initialisation is detected is part of the first-sample fix rather than a lint sweep.
- Tests: a ZOH-held discrete block sampled at a known rate produces a staircase; an
  FOH-held block interpolates linearly between samples. Both read trajectories through
  Phase 5's recorder.

**Done when:** both holders are covered by tests and no path divides by a zero step.

---

## Phase 9 — Block contract completion

- Rename `is_stateful` to `breaks_algebraic_loops`, default `false`. Document the
  distinction: a block may have state *and* direct feedthrough (a PID with a
  proportional term), and only the latter matters for loop breaking.
- Add `initialize(&mut self, output: …)` so initial conditions are expressible without
  relying on the zero bit pattern.
- Document that per-*port* feedthrough granularity is a known coarseness.

**Done when:** a closed loop containing a stateful-but-feedthrough block is correctly
rejected as an algebraic loop.

---

## Phase 10 — Block library seed

Enough blocks to demonstrate the design, not a comprehensive library.

- `Gain`, `Sum`, `Saturation`, `Integrator`, `Step` (reusing the existing
  `ZeroOrderHold` and `FirstOrderHold`).
- A closed-loop PI-controlled first-order plant as the headline example, asserted
  against the analytical solution through Phase 5's recorder.

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
