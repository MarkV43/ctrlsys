# Validation: Gates Before Work (Phase 0)

## Done when

- [ ] `cargo clippy --all-targets -- -D warnings` exits 0.
- [ ] `cargo test --test safety_docs` passes (it fails today on 7 declarations).
- [ ] `cargo fmt --check` exits 0.
- [ ] `Cargo.toml` has an empty `[dependencies]` and a `[dev-dependencies]` with
      `proptest`, `trybuild`, `approx` — and no `criterion`.
- [ ] `src/lib.rs` carries all eight lints from `tech-stack.md`, verbatim.
- [ ] `grep -rn zerocopy src/ tests/ Cargo.toml` returns nothing.
- [ ] Every `#[expect]` in the tree has a `reason = "…"` naming the phase that removes
      it. No bare `#[allow]` was added anywhere in this phase.
- [ ] The `src/main.rs` demo prints byte-identical output to its output on `main`.
- [ ] Miri's diagnostic is pasted into the baseline section below. Miri is expected to
      remain **red** — Phase 2 owns it; see `requirements.md`.

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

with the 8-lint block applied   → 19 hard errors:
  13× undocumented_unsafe_blocks
       buffer.rs:34,54,66,72 · link.rs:109,154 · pool/mod.rs:84
       discrete/mod.rs:62 · system/mod.rs:46,47,93,104,128
   3× multiple_unsafe_ops_per_block
       link.rs:109 (2 ops) · link.rs:154 (3 ops) · system/mod.rs:128 (2 ops)
   2× missing_safety_doc (the two exported trait fns)
   1× useless_vec
```

Reproduce the third block without editing `lib.rs`:

```bash
cargo clippy --all-targets -- -D unsafe_op_in_unsafe_fn -D clippy::undocumented_unsafe_blocks -D clippy::missing_safety_doc -D clippy::multiple_unsafe_ops_per_block -D clippy::unnecessary_safety_comment -D clippy::unnecessary_safety_doc -D clippy::missing_panics_doc -D clippy::missing_errors_doc
```

### Zero-behaviour-change check

```bash
git stash && cargo run --quiet > /tmp/before.txt; git stash pop && cargo run --quiet > /tmp/after.txt && diff /tmp/before.txt /tmp/after.txt && echo IDENTICAL
```

Must print `IDENTICAL`. Any diff means a clippy fix changed semantics — find it and
convert it to an `#[expect]` owned by the phase that should make that change.

### Miri baseline to record

Run and paste the output here as part of this phase:

```bash
cargo +nightly miri run
```

Expected: the `pool/mod.rs` retag error quoted in the roadmap's Phase 2 section.
Paste the verbatim diagnostic below so Phase 2 starts from a dated, recorded fact.

```text
<paste `cargo +nightly miri run` output here>
```

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
