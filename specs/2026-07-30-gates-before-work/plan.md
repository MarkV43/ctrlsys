# Plan: Gates Before Work (Phase 0)

Build order matters here. The lints go on **before** anything is fixed (group 2), so
the remaining work is a mechanical list emitted by the compiler rather than a list
derived by reading. The crate will not pass clippy between group 2 and group 6; that
is intended.

## 1. Dependency baseline

- [ ] Remove `zerocopy` from `[dependencies]` in `Cargo.toml`; leave the section empty.
- [ ] Confirm nothing still references `zerocopy` (`grep -rn zerocopy src/ tests/`).
- [ ] Add `[dev-dependencies]`: `proptest`, `trybuild`, `approx`.
- [ ] Note in the commit message that `criterion` is deliberately *not* added — it is
      listed in `tech-stack.md` as "later phases only".

## 2. Turn the lints on

- [ ] Replace the two-line lint block in `src/lib.rs` with the full eight-lint block
      from `tech-stack.md` (six unsafe-related plus `missing_panics_doc` and
      `missing_errors_doc`).
- [ ] Record the resulting failure count as the work list for groups 3–6. Current
      measured baseline: **19 hard errors** (13 undocumented unsafe blocks, 3
      multiple-ops blocks, 2 missing `# Safety` on exported fns, 1 `useless_vec`) plus
      ~23 unique pedantic warnings.

## 3. Declaration contracts — the 7 `# Safety` sections

The gate: `cargo test --test safety_docs`, currently failing on exactly these.
Written as prose for an outside reader (see `requirements.md` — these are dissertation
material, not lint-appeasement).

- [ ] `src/system/mod.rs:55` — `SystemDataIn::from_slices` (trait declaration). The
      load-bearing one: slice **count** must equal the arity of the implementing
      `Self` type; each slice must be at least `size_of` its leaf type, aligned to
      `align_of` it, and contain a valid value of it.
- [ ] `src/system/mod.rs:60` — `SystemDataOut::from_slices_mut` (trait declaration).
      As above, plus the uniqueness obligation: no other live reference may alias the
      slice for the returned lifetime.
- [ ] `src/system/mod.rs:68` — `from_slices` for `()`. Contract is vacuous; say so and
      say why (no slice is read).
- [ ] `src/system/mod.rs:74` — `from_slices_mut` for `()`. Same.
- [ ] `src/system/mod.rs:85` — `from_slices` for `&'a T`.
- [ ] `src/system/mod.rs:100` — `from_slices_mut` for `&'a mut T`.
- [ ] `src/system/mod.rs:119` — `from_slices` for `(&'a T, &'a U)`. Two independent
      slices, each with its own alignment obligation — state that no tuple layout is
      involved, since that is the design's whole claim.
- [ ] Cross-check each contract against `tech-stack.md`'s architecture section so the
      docs and the design document do not drift.

## 4. Use-site `// SAFETY:` comments — the sites that stay

Nine blocks in code no later phase deletes. Each comment must name the invariant and
where it is established (Article 3), not restate the operation.

- [ ] `src/pool/buffer.rs:34` — `alloc_zeroed`. Cite the non-zero size check above it
      and the validated `Layout`.
- [ ] `src/pool/buffer.rs:54` — `dealloc` in `Drop`. Cite the `layout.size() != 0`
      guard and that the layout is the one stored at allocation.
- [ ] `src/pool/buffer.rs:66` — `from_raw_parts` in `Deref`.
- [ ] `src/pool/buffer.rs:72` — `from_raw_parts_mut` in `DerefMut`. Cite `&mut self`
      for uniqueness.
- [ ] `src/system/mod.rs:46` — `Sys::Input::from_slices` call in `raw_update`. This is
      the caller side of group 3's contract; the comment must point at whoever
      guarantees the arity (today: the `debug_assert!`s, which Phase 3 promotes to
      `assert!` — see `requirements.md`).
- [ ] `src/system/mod.rs:47` — `Sys::Output::from_slices_mut` call in `raw_update`.
- [ ] `src/system/mod.rs:93` — `&*ptr` in `&'a T`'s impl.
- [ ] `src/system/mod.rs:104` — `&mut *ptr` in `&'a mut T`'s impl.
- [ ] `src/system/mod.rs:128` — **also a `multiple_unsafe_ops_per_block` error** (2
      ops). Split `unsafe { (&*t_ptr, &*u_ptr) }` into two single-op blocks bound to
      locals, each with its own comment, then build the tuple in safe code. No
      behaviour change.
- [ ] Add `# Panics` to `AlignedBuffer::new` — it has **two** panic paths, not one:
      the `.expect()` on `Layout::from_size_align` and the `.unwrap()` on the
      zero-size `Layout::from_size_align(0, 1)`.

## 5. `#[expect]` markers — the four sites that cannot be documented here

Per the warnings decision: `#[expect(…, reason = "…Phase N")]`, never `#[allow]`, so
the marker self-destructs when the owning phase lands. Attach at the narrowest scope
that compiles (statement, else enclosing fn).

**Phase 2 owns these — the code is unsound, so no true SAFETY comment exists:**

- [ ] `src/pool/mod.rs:84` — the Miri-confirmed SharedReadOnly→Unique retag.
- [ ] `src/system/discrete/mod.rs:62` — `ptr::read` duplicating a `&'s mut`.

**Phase 4 owns these — `From<(&T, &U)>` / `From<(&T, &U, &V)>` offset machinery, deleted wholesale:**

- [ ] `src/pool/link.rs:109` — 2 `addr_of!` ops (fires both `undocumented_unsafe_blocks`
      and `multiple_unsafe_ops_per_block`).
- [ ] `src/pool/link.rs:154` — 3 `addr_of!` ops (same two lints).
- [ ] Each reason string names the phase and, for the Phase 2 pair, states that the
      block is *known unsound* — the marker is a defect record, not a suppression.

## 6. Clear the remaining pedantic backlog

- [ ] `src/pool/mod.rs:64` — `useless_vec` (the one pre-existing **hard error**, denied
      via `clippy::perf`).
- [ ] `src/pool/mod.rs:39` — `# Panics`; `src/pool/mod.rs:119` — `# Errors` on `simulate`.
- [ ] `ptr_as_ptr` ×4, `explicit_iter_loop` ×5, `redundant_closure`,
      `needless_borrow`, `explicit_auto_deref`, `iter_copied_collect`,
      `semicolon_if_nothing_returned` ×2, `new_without_default` on `SystemPool`.
- [ ] `#[expect]` (not fix) the phase-owned remainder:
      - `src/pool/link.rs:7` — dead `to_input_offset` / `num_bytes` fields → **Phase 4**
      - `src/pool/graph.rs:116,137,141` — `isize`/`usize` cast lints in Tarjan → **Phase 5**
      - `src/system/discrete/holder.rs:98`, `src/system/discrete/mod.rs:48` — `float_cmp`
        → **Phase 7** (fixing these changes numerical behaviour, which this phase forbids)

## 7. Verify and record

- [ ] `cargo clippy --all-targets -- -D warnings` clean.
- [ ] `cargo test --test safety_docs` passes.
- [ ] `cargo fmt --check` clean.
- [ ] Run `cargo +nightly miri run` and paste the current diagnostic into
      `validation.md` as the recorded Phase 2 baseline. **Miri stays red** — see
      `requirements.md` for why that does not block this phase.
- [ ] Confirm zero behaviour change: `src/main.rs` demo output is byte-identical to
      its output on `main`.
