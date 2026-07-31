# ctrlsys — Tech Stack

## Language and toolchain

- **Rust, edition 2024.** Stable for building and testing.
- **Nightly for Miri only** (`cargo +nightly miri test`). Nightly features are not
  used in the crate itself.
- `std` is available and used freely — see the `no_std` decision below.

## Runtime dependencies

**None.**

`Cargo.toml` `[dependencies]` is empty and stays empty. Adding one requires amending
this document.

### Why zerocopy was dropped

zerocopy's original job was to prove that a composite signal type could be safely
reconstructed from a flat byte buffer — `FromBytes` for validity, `IntoBytes` for the
absence of padding.

**The slice-per-link design removes that job entirely.** A composite input is never
materialised in a buffer. `SystemDataIn::from_slices` receives one `&[u8]` per link,
each pointing at the *producer's* own output buffer, and builds `(&'a T, &'a U)` — a
tuple of references, on the stack. Tuple layout is irrelevant because no tuple is ever
written to or read from bytes. Only leaf types are ever cast, each from its own slice,
each aligned by its producer's `output_alignment()`.

This also means there are no input buffers and no copies at all: every input is a
borrow of the producer's output.

zerocopy could not have solved the original tuple problem anyway — it implements its
traits only for `()` among tuples, because `(A, B)` layout is unspecified. The
pre-`pool/` code's response was to delete the bounds and hand-roll transmutes, which
is what introduced the alignment UB. The slice-per-link design solves the same problem
by never needing the layout in the first place.

## Development dependencies

Unrestricted. Currently planned:

| Crate | Purpose |
|---|---|
| `proptest` | Random DAGs for ordering property tests |
| `trybuild` | Compile-fail tests guarding the soundness boundary |
| `approx` | Tolerance comparisons in analytical golden tests |
| `criterion` | Step-loop benchmarks (later phases only) |

## Lints

In `src/lib.rs`, enforced rather than reviewed:

```rust
#![warn(clippy::pedantic)]
#![deny(clippy::perf)]

// --- Article 3: unsafe must justify itself ---
#![deny(unsafe_op_in_unsafe_fn)]              // forces explicit blocks inside `unsafe fn`
#![deny(clippy::undocumented_unsafe_blocks)]  // `// SAFETY:` on every block and `unsafe impl`
#![deny(clippy::missing_safety_doc)]          // `# Safety` on exported `unsafe fn` / `unsafe trait`
#![deny(clippy::multiple_unsafe_ops_per_block)] // one op per block, so the comment means something
#![deny(clippy::unnecessary_safety_comment)]  // catches SAFETY comments left on safe code
#![deny(clippy::unnecessary_safety_doc)]

#![deny(clippy::missing_panics_doc)]
#![deny(clippy::missing_errors_doc)]
```

A lint that is enabled and ignored is worse than one never enabled. The existing
`clippy::pedantic` backlog is cleared in Phase 0 before anything else lands.

### What each lint actually catches

Verified against a crate containing one deliberate instance of each case. Every row
below is a hard error under the block above.

| Case | Caught by |
|---|---|
| `unsafe { … }` with no `// SAFETY:` | `undocumented_unsafe_blocks` |
| `unsafe impl Trait for T` with no `// SAFETY:` | `undocumented_unsafe_blocks` |
| unsafe op in an `unsafe fn` body with no inner block | `unsafe_op_in_unsafe_fn` (`E0133`) |
| Two unsafe ops under one `// SAFETY:` | `multiple_unsafe_ops_per_block` |
| `// SAFETY:` left on code that is no longer unsafe | `unnecessary_safety_comment` |
| **Exported** `unsafe fn` / `unsafe trait` with no `# Safety` | `missing_safety_doc` |

### The gap the lints cannot close

`clippy::missing_safety_doc` only fires on **externally reachable** items:

| Declaration | Caught |
|---|---|
| `pub unsafe fn` in a `pub mod` | yes |
| `pub unsafe fn` in a private mod | no |
| `pub(crate) unsafe fn` | no |
| private `unsafe fn` | no |

This is not hypothetical — it is exactly the current code's situation. `ref_from_bytes`
and `mut_from_bytes` are private `unsafe fn`s whose contract (alignment, length,
validity) is what every other safety argument in the crate depends on, and no lint
configuration asks for it.

`tests/safety_docs.rs` closes the gap: it walks `src/`, finds every `unsafe fn` and
`unsafe trait` declaration regardless of visibility, and fails if the attached doc
block has no `# Safety` heading. It runs under `cargo test`, so it is already one of
the four gates and needs no extra tooling.

**What nothing can enforce:** whether a `// SAFETY:` comment is *true*, or merely
present. Article 3 requires it to name the specific invariant relied upon and where
that invariant is established — "SAFETY: this is fine" satisfies every lint above and
satisfies no article. That one stays a review judgement.

## Verification gates

Every phase must leave all four green:

```bash
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && cargo +nightly miri test
```

Miri is slow. Simulation tests that run under Miri are `#[cfg(miri)]`-gated to short
`total_time` values; the full-length versions run on stable only.

`tests/safety_docs.rs` is `#[cfg_attr(miri, ignore)]`. It reads `src/` from disk, and
Miri isolates filesystem access, so it fails at `opendir` before doing anything —
which meant `cargo +nightly miri test` never passed at all between `cdd17e9` and
Phase 2. Nothing is lost by skipping it: it is a static scan of source text and
executes no crate code. Do **not** reach for `-Zmiri-disable-isolation` to accommodate
it; that would weaken every other Miri run.

Run Miri under **both** aliasing models. They are different claims, and on the Phase 2
baseline they blamed different lines for the same bug — Stacked Borrows the retag in
the solver, Tree Borrows the eventual write inside innocent user block code:

```bash
MIRIFLAGS=-Zmiri-tree-borrows cargo +nightly miri test
```

## Architecture

Layers, innermost first. Each is testable without the one above it.

### `system` — the signal interface (contains `unsafe`)

The block-author side:

```rust
pub trait DiscreteSystem {
    // `output` is `&mut`, not by value, so a holder can lend it to `calculate` and
    // still read it back afterwards. Write through it as `**output = value`.
    fn calculate(&mut self, time: f64, input: Self::Input<'_>, output: &mut Self::Output<'_>);
    fn timestep(&self) -> f64;
}
```

Passing by value forced `HeldSystem` to duplicate the output view through a raw
pointer, which for the usual `&mut T` case duplicated a mutable reference. Article 6
makes this contract public API, so the change is recorded rather than assumed.

```rust
pub trait SystemDataIn<'a> {
    type Payload: ?Sized;
    unsafe fn from_slices(slices: &[&'a [u8]]) -> Self;
}

pub trait SystemDataOut<'a> {
    type Payload: ?Sized;
    unsafe fn from_slices_mut(slices: &mut [&'a mut [u8]]) -> Self;
    fn copy_from_payload(&mut self, payload: &Self::Payload);
    fn payload_ref(&self) -> &Self::Payload;
}
```

**One slice per link.** `Input<'a> = (&'a f64, &'a f64)` is built from two independent
slices, each borrowed from a different producer's output buffer. Composite signals are
never laid out in memory, so there are no offsets, no padding questions, no coverage
tiling, and no copies.

Implemented for `()`, `&'a T`, `&'a mut T`, and tuples of references. Extending to
higher arities means adding impls, not computing layouts.

### `pool::buffer` — aligned storage and the disjoint borrow (contains `unsafe`)

`AlignedBuffer`: one `alloc_zeroed` allocation per system, sized and aligned from
`RawSystem::output_size()` / `output_alignment()`, with a zero-size path that does not
allocate. This is what makes the leaf casts in `from_slices` aligned by construction.

`BufferSet` owns those buffers and provides the one borrow the solver needs:
`with_split(out, inputs, f)` lends buffer `out` mutably and every buffer in `inputs`
immutably, for the duration of `f`. It asserts `out ∉ inputs` first.

This lives here rather than in the solver because Article 1's placement rule puts
`unsafe` in buffer allocation and forbids it in solver code. `SystemPool::simulate`
contains no `unsafe` at all as a result.

Because each `AlignedBuffer` owns a **separate** heap allocation, the whole operation
needs exactly one `unsafe` op — extending an input reference's lifetime so the scratch
vector holding them can be reused across steps. The mutable borrow is an ordinary
`&mut self.buffers[out]`, taken after the input borrows are released; it touches a
different allocation, so it cannot invalidate them.

The callback shape is not stylistic. Returning the borrows would tie them to a single
`&mut self`, and a scratch vector hoisted out of the solver's loop cannot hold
references shorter-lived than itself. Keeping them inside a callback is what lets the
steady state allocate nothing per step.

### `pool::link` — wiring (safe)

`SystemRef<In, Out>` (`Copy`) identifies a system; `SystemMux<Out>` is an ordered list
of producer ids built via `From<(&T, &U)>` / `From<(&T, &U, &V)>`, so a mux is written
as a tuple of references at the call site. `link` requires `SI: SystemIn<In = Out>`,
so a producer/consumer type mismatch is a compile error.

**Ports are being split.** Today `add_system` returns one handle implementing both
`SystemIn` and `SystemOut`, so nothing in the type system distinguishes a system's
input side from its output side. The agreed design returns a pair:

```rust
let (f_i, f_o) = pool.add_system(Filter);
pool.link(s_o, &f_i);
let recv = pool.probe(&f_o);
```

The reason is `probe`, not `link`: a probe attaches to an *output*, and with one
combined handle `pool.probe(&f_i)` type-checks and means nothing. Scheduled together
with the deletion below, because both restructure the same three types.

This does **not** make self-feed a compile-time error — `pool.link(f_o, &f_i)` for the
same system still type-checks, since ids are runtime values. The
`assert!(!from.ids().contains(&to.id()))` in `link` stays, and stays load-bearing: it
is what every `// SAFETY:` comment in `system/mod.rs` cites for the absence of
shared/unique aliasing on a buffer. Per Article 2 it carries a comment saying why it
cannot be lifted to the type system.

`SystemLayout`, `SystemLink::to_input_offset` and `SystemLink::num_bytes` are
**vestigial** — carried over from the flat-buffer design and no longer read by the
solver. Their removal is a roadmap phase.

### `pool::graph` — ordering (safe, pure)

Tarjan SCC, algebraic-loop rejection via `breaks_algebraic_loops()`, dataflow
ordering within each SCC. Pure functions over an adjacency list — fully testable
with no simulation.

### `pool` — the solver (safe)

Owns buffers and links, computes the order once per `simulate`, runs the step loop.

Also the home of observation. `pool.probe(&f_o)` attaches a recorder to an output port
and `pool.probe((&f_o, &s_o))` to a mux of them, reusing the same `From<(&T, &U)>`
conversion `link` uses. A probe is an ordinary block that returns `f64::INFINITY`, so
it contributes nothing to the `next_time` minimum and cannot change the step sequence —
observation is non-invasive by construction, and there is a test asserting it rather
than only the argument.

`pool.sample(&f_o, timestep)` — a fixed-interval sampler — is **not** in that category.
Requesting periodic time events changes the step sequence, which for a rate-dependent
block changes the trajectory: exactly the failure `mission.md` Article 5 records. It
waits until rate-independence holds.

## Decisions recorded

| Decision | Choice | Consequence |
|---|---|---|
| `no_std` | Not supported | `Box<dyn>`, `Vec`, `HashSet`, `std::sync::mpsc` used freely. Retrofitting means replacing collections and the probe transport. |
| Composite signals | Flat tuples only | No nesting, so `link` never unifies nested shapes and inference stays simple. |
| Named-field signals | Atomic for now | Decomposing them needs a derive macro; deferred, not rejected. |
| Port handles | Split: `let (f_i, f_o) = pool.add_system(…)` | `probe` takes an output port, so probing an input is a compile error. Does not make self-feed compile-time; that check stays runtime. |
| Observation | `probe` every step, non-invasive | Returns `f64::INFINITY`, adding no time event. A periodic `sample` would perturb the step sequence, so it waits for rate-independence. |
| Time delivery | Absolute only | See `mission.md` Article 6. |
| Error style | `assert!` for user contracts | See `mission.md` Article 4. |
| Soundness checks | Always on, even for internal invariants | A check an `unsafe` block relies on is never `debug_assert!`: compiling it out turns a panic into UB in the least diagnosable build. Extends Article 4. |
| Discrete `calculate` | Takes `&mut Self::Output<'_>` | Lets a holder lend the output and read it back without duplicating a `&mut`. Costs block authors `**output` instead of `*output`. |
