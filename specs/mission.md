# ctrlsys — Mission

## What this is

A block-diagram simulator for control systems, written in Rust. Users define blocks
(`System` implementations) with typed inputs and outputs, wire them into a graph, and
run them under a variable-step solver that respects each block's own timing needs.

The simulation model is **FMI Co-Simulation semantics**: each block advances its own
internal state, and the pool acts as a master that chooses communication step sizes
from what the blocks request. There is no global ODE solver.

## Who it is for and what it must deliver

Primary deliverable is a **mestrado dissertation** (UFSC). The crate must be correct,
soundness-provable, and architecturally defensible in writing. A reader who has never
seen the code must be able to follow why it is safe from the docs alone.

Secondary: the crate should remain **publishable later without a rewrite**. That
imposes discipline now — no `unsafe` in the public API, semver-aware naming, docs
written for an outside reader — but does not impose scope. Breadth of block library,
benchmarks, and API stability guarantees are deferred, not designed against.

## Non-goals

Explicitly out of scope. Adding any of these requires amending this document first.

- **State events.** Zero crossings that depend on integrated state (bouncing ball,
  relay on a state threshold) are not supported. Only *time events* — transitions a
  block can predict as a function of time — are handled.
- **Step rejection and global error control.** Blocks integrate internally, so a step
  cannot be retried at a smaller size. This is the accepted cost of the co-simulation
  model.
- **`no_std` / embedded deployment.** `std` is used freely.
- **Matrix / state-space blocks**, and any linear-algebra dependency.
- **Demux of user-defined structs.** Tuples decompose into ports; named-field structs
  are atomic until a derive macro exists.

## Articles

These govern all work in this repository. A phase is not complete until it satisfies
every article that applies to it.

### Article 1 — Soundness is not negotiable

No safe function may cause undefined behaviour for any input. A safe API that can
trigger UB is the highest-severity class of bug in this repository, ranked above
incorrect results.

`unsafe` appears only in designated modules (signal layout and buffer allocation).
It must not appear in solver, graph, or block code.

`cargo +nightly miri test` passes. A phase that leaves Miri red is not done.

### Article 2 — Prefer the compiler to the runtime

Invariants are enforced in the type system wherever the type system can express them.
A runtime check in the public API must carry a comment stating why it could not be a
compile-time check.

Established precedents:

- `link` is bounded `SI: SystemIn<In = Out>`, so wiring a producer to a consumer of a
  different type is a compile error, not a runtime check.
- `SystemDataIn` is implemented per input *shape*, so a system's arity is fixed by its
  `Input<'a>` type rather than validated at wiring time.
- Composite layout is not checked because it is not computed: one slice per link means
  there are no offsets that could disagree with the type.

What remains runtime, and why: the number of links attached to a system must match the
arity its `Input<'a>` expects. That relates a *graph* property to a *type* property,
and nothing in the type system connects them. Per Article 4 it is a user contract, so
it is an always-on check.

### Article 3 — Every unsafe block explains why it cannot fail

Two distinct obligations, often conflated:

- **`// SAFETY:` at the use site** — why *this code* is permitted to do this. Required
  on every `unsafe` block and every `unsafe impl`. It must name the specific invariant
  relied upon and where that invariant is established, not restate what the code does.
- **`# Safety` on the declaration** — what a *caller* must guarantee. Required on every
  `unsafe fn` and `unsafe trait`, **at any visibility**, including private ones.

Also `# Panics` and `# Errors` on every public function that can panic or return `Err`.

Enforcement is mechanical, not review discipline — but it is not complete, and the
incompleteness is stated rather than assumed:

- Lints cover use sites entirely, and declarations only when externally reachable.
- `tests/safety_docs.rs` covers declarations at every visibility, closing that gap.
- **Nothing can check that a SAFETY comment is true.** `// SAFETY: this is fine` passes
  every automated gate and satisfies no part of this article. That remains a review
  judgement, and it is the reason this article specifies *what the comment must say*
  rather than merely that one must exist.

See `tech-stack.md` for the verified per-case breakdown.

### Article 4 — `assert!` for user contracts, `debug_assert!` for internal invariants

A check that guards something *user code* can get wrong is always on. A check that
guards an invariant *this crate* maintains may be `debug_assert!`.

Rationale, with precedent: the most natural way to write a discrete block's next
sample time — `(time / period + 1.0).floor() * period` — returns its own current time
at `t = 0.29` due to float rounding, and hangs the simulator forever with no output.
Compiling that check out would move the failure to exactly the build where it cannot
be diagnosed.

### Article 5 — No silent numerical wrongness

A model that cannot be simulated correctly must fail loudly, at construction time or
at `simulate()` setup. It must never produce plausible-looking wrong numbers.

Precedents this rule exists for:

- **Historical, in the pre-`pool/` flat-buffer design:** the mux offset computation
  placed the second input at byte 128 instead of 8. The simulation ran to completion
  and reported zeros. The slice-per-link design has since removed the whole class.
- **Current:** a filter with fixed coefficients and no rate compensation changes its
  step response from 0.431 to 0.994 at t = 1.0 when an *unrelated* block is added to
  the model. Still reproducible against `src/main.rs`.

### Article 6 — The block author contract is public API

Documented, tested, and versioned like any other interface:

- `update` receives **absolute time only**. The pool cannot know a block's effective
  `dt`, because whether a block updated on a given step is the block's own decision.
- A block with internal dynamics **must be rate-independent**: derive `dt` from its
  own stored last-update time, not from the assumption that it is called at a fixed
  rate.
- The returned `next_time` **must be strictly greater than** `time`. Discrete sample
  times are derived from an integer counter, never by rounding floats.
- `output` is **in-out**: it persists across steps and blocks may read their previous
  value as state. Initial conditions are set through `initialize`, not by assuming a
  zero bit pattern.

### Article 7 — Scope is stated, not implied

The non-goals above are decisions, not omissions. Each is documented where a reader
would otherwise assume oversight. Changing one means amending this document in the
same change that implements it.

### Article 8 — Tests are gates, not aspirations

Each category is responsible for a distinct failure class, and each is a merge gate:

| Category | Catches |
|---|---|
| `graph.rs` unit tests | Wrong execution order, missed algebraic loops |
| Layout tests | Computed offsets disagreeing with `offset_of!` |
| Analytical golden tests | Numerically wrong dynamics (first-order lag vs `1 - e^(-t/τ)`) |
| Property tests | Ordering violations on random DAGs |
| `trybuild` compile-fail | Erosion of the soundness boundary — a double-link or a non-`Signal` type that starts compiling |
| Miri | Alignment, aliasing, uninitialised reads |
