# Validation: Gates Before Work (Phase 0)

## Done when

- [x] `cargo clippy --all-targets -- -D warnings` exits 0.
- [x] `cargo test --test safety_docs` passes (it failed on 7 declarations).
- [x] `cargo fmt --check` exits 0. Note this was **already red on `main`**, on
      `tests/safety_docs.rs`; Phase 0 fixes that too.
- [x] `Cargo.toml` has an empty `[dependencies]` and a `[dev-dependencies]` with
      `proptest`, `trybuild`, `approx` — and no `criterion`.
- [x] `src/lib.rs` carries all eight lints from `tech-stack.md`, verbatim.
- [x] `ctrlsys` declares no runtime dependency on `zerocopy`: its `Cargo.lock` entry
      lists only the three dev-deps. The crate still appears *in* the lock file as a
      transitive dependency of `proptest` (via `rand_chacha` → `ppv-lite86`), which is
      expected and does not violate the empty-`[dependencies]` rule. One prose mention
      survives in `src/system/mod.rs`, explaining why a zerocopy-style bound on
      composite signals is unnecessary — deliberate, and matching `tech-stack.md`.
- [x] Every `#[expect]` in the tree has a `reason = "…"` naming the phase that removes
      it (9 total). No bare `#[allow]` was added anywhere in this phase.
- [x] The simulated trajectory is bit-identical to `main` — 2016 samples. See the
      method below; a byte-diff of the demo's stdout does not work.
- [x] Miri's diagnostic is pasted into the baseline section below. Miri remains
      **red** — Phase 2 owns it; see `requirements.md`.

## How to check

### The four gates

```bash
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && cargo +nightly miri test
```

Phase 0 must leave the first three green. The fourth stays red by design.

### Baseline to beat

Measured on `main` at `cdd17e9`, before any Phase 0 work:

```text
cargo test --test safety_docs   → FAILED, 7 declarations:
  src/system/mod.rs:55, 60, 68, 74, 85, 100, 119

cargo clippy --all-targets      → 1 error (useless_vec, src/pool/mod.rs:64)
                                  + ~23 unique pedantic warnings

cargo fmt --check               → already FAILING on tests/safety_docs.rs
                                  (2 hunks, introduced by cdd17e9)

with the 8-lint block applied   → 22 hard errors:
  13× undocumented_unsafe_blocks
       buffer.rs:34,54,66,72 · link.rs:109,154 · pool/mod.rs:84
       discrete/mod.rs:62 · system/mod.rs:46,47,93,104,128
   3× multiple_unsafe_ops_per_block
       link.rs:109 (2 ops) · link.rs:154 (3 ops) · system/mod.rs:128 (2 ops)
   2× missing_safety_doc (the two exported trait fns)
   2× missing_panics_doc (pool/mod.rs:39 simulate, :119 link)
   1× missing_errors_doc (pool/mod.rs:39 simulate)
   1× useless_vec
```

Measure the last block by editing `src/lib.rs`, not from the command line: crate-level
attributes take priority over `-D` flags, so the existing `#![warn(clippy::pedantic)]`
holds `missing_panics_doc` and `missing_errors_doc` at warn level and a command-line
probe undercounts by 3.

### Zero-behaviour-change check

Diffing `cargo run` output does **not** work: `src/main.rs` prints only
`Elapsed: <duration>`, which is nondeterministic timing and no simulation values at
all. (That absence is the gap Phase 9's `Recorder` closes, and the reason Phase 6's
golden tests cannot be written against `main.rs` as it stands.)

The method used instead, and the one to repeat for any later phase claiming no
behaviour change:

1. Write a throwaway `tests/tmp_behaviour_trace.rs` that rebuilds `main.rs`'s model —
   `Filter`, `Input`, `Test`, and the ZOH-held `DiscreteTest`.
2. Add a `Probe` block (`Input<'a> = &'a f64`, `Output<'a> = ()`, returns
   `f64::INFINITY` so it introduces no time event) that pushes `(tag, time, value)`
   into a `thread_local` `Vec`. Append the probes **after** the real blocks so the
   existing systems keep their indices, and link one to each of `filter`, `test` and
   `discr`.
3. Print every sample as `{tag} {time:.17e} {value:.17e}` — full mantissa, so the
   comparison is bit-for-bit rather than to a printed precision.
4. Capture on the branch, then `git stash push -- src/ Cargo.toml tests/safety_docs.rs`
   (an untracked test file survives the stash), capture again, `git stash pop`, diff.
5. Delete the harness before committing.

**Result for this phase: identical, 2016 samples, bit for bit.**

Any diff means a clippy fix changed semantics — find it and convert it to an
`#[expect]` owned by the phase that should make that change.

### Miri baseline to record

Run and paste the output here as part of this phase:

```bash
cargo +nightly miri run
```

Recorded 2026-07-30 on `feature/gates-before-work`. This is the roadmap's Phase 2
error, unchanged in substance — only the line numbers moved, and `data_ptr` now reads
`.cast_mut()` where it read `as *mut u8`, which is the same cast written the way
clippy prefers.

```text
error: Undefined Behavior: trying to retag from <33736> for Unique permission at
       alloc12817[0x0], but that tag only grants SharedReadOnly permission for this location
   --> src/pool/mod.rs:122:45
    |
122 |     let output_slice = unsafe { std::slice::from_raw_parts_mut(data_ptr, len) };
    |                                 ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    |                                 this error occurs as part of retag at alloc12817[0x0..0x8]
help: <33736> was created by a SharedReadOnly retag at offsets [0x0..0x8]
   --> src/pool/mod.rs:108:32
    |
108 |     let data_ptr = output_buf.as_ptr().cast_mut();
    |                    ^^^^^^^^^^^^^^^^^^^
    = note: stack backtrace:
            0: ctrlsys::pool::SystemPool::simulate at src/pool/mod.rs:122:45: 122:90
            1: main at src/main.rs:105:5: 105:30

error: aborting due to 1 previous error
```

Note that Miri aborts here, so `src/system/discrete/mod.rs`'s `ptr::read` duplication
of a `&mut` is still **unconfirmed by Miri** — it was found by reading. Phase 2 must
re-run Miri after fixing this site to see whether the second one surfaces.

Also note `cargo +nightly miri run` exercises only `src/main.rs`. `cargo +nightly miri
test` currently runs no simulation at all, because the only test in the tree is
`safety_docs`, which reads source files. Phase 2's Done-when should use `miri run`, or
land a simulation test first.

### Review judgements the gates cannot make

These are the pass criteria no command checks — they are the reason Article 3 specifies
what a comment must *say*:

- [ ] Each of the 7 `# Safety` sections states what a **caller** must guarantee
      (count, length, alignment, validity, uniqueness where applicable) — not what the
      body does.
- [ ] Each of the 9 `// SAFETY:` comments names the specific invariant and **where it
      is established**. "SAFETY: this is fine" passes every gate and fails this.
- [ ] The arity requirement is phrased as a caller obligation, so Phase 3's
      `debug_assert!` → `assert!` promotion does not falsify it.
- [ ] The two Phase 2 `#[expect]` reasons state that the block is known-unsound, not
      merely that it is undocumented.
