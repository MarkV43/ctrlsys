# Requirements: Gates Before Work (Phase 0)

## Scope

**In** — all five roadmap items for Phase 0, as written:

1. `[dependencies]` emptied; `zerocopy` removed.
2. `proptest`, `trybuild`, `approx` added as dev-dependencies.
3. The full eight-lint block from `tech-stack.md` in `src/lib.rs` — all six
   unsafe-related lints, not just `undocumented_unsafe_blocks`.
4. `# Safety` contracts written for the 7 declarations in `src/system/mod.rs` that
   `tests/safety_docs.rs` currently fails on (lines 55, 60, 68, 74, 85, 100, 119 —
   verified, the test names exactly these).
5. The clippy backlog cleared, including `# Panics` on `AlignedBuffer::new` and
   `# Errors` on `simulate`.

**Consequence of item 3, not an expansion of scope:** turning on
`undocumented_unsafe_blocks` and `multiple_unsafe_ops_per_block` makes 16 additional
use sites hard errors, which the roadmap's "~24 clippy warnings" estimate does not
include. Measured: **19 hard errors** total once the lints are on. This subsumes the
residual the roadmap folded into Phase 0 from Phase 1 (`AlignedBuffer`'s undocumented
unsafe blocks) — the lint finds **four** such blocks in `buffer.rs`, not the three the
roadmap names.

**Out:**

- Any behaviour change. See the hard rule below.
- Fixing the UB at `src/pool/mod.rs:84` or `src/system/discrete/mod.rs:62` — Phase 2.
- Deleting the offset machinery in `src/pool/link.rs` — Phase 4.
- `criterion` — `tech-stack.md` scopes it to later phases.
- Making Miri green — see the gate tension below.

## Decisions

### Warnings in code a later phase will rewrite: `#[expect]`, naming the owning phase

Several lint failures sit in code that Phases 2, 4, 5 and 7 will delete or rewrite.
Fixing them properly means pulling those phases forward and abandoning "nothing
functional changes"; suppressing them with `#[allow]` leaves a marker that stays
silently valid forever after the owning phase lands.

`#[expect(lint, reason = "…Phase N")]` resolves both. When the later phase deletes or
fixes the code, the expectation itself starts warning, so the marker cannot rot — the
gate that Phase 0 installs is also what removes Phase 0's own concessions.

Applied to: the dead `to_input_offset`/`num_bytes` fields (Phase 4), the Tarjan
`isize`/`usize` casts (Phase 5), the two `float_cmp` sites (Phase 7), the `link.rs`
`addr_of!` blocks (Phase 4), and the two UB sites (Phase 2).

### The four unsafe blocks that get a marker instead of a comment

This is the decision that most affects what Phase 0 actually means, and it follows
from Article 3 rather than from convenience.

- `src/pool/mod.rs:84` and `src/system/discrete/mod.rs:62` are the two live UB sites
  Phase 2 exists to fix. Article 3 requires a `// SAFETY:` comment to name the
  invariant relied upon. **These blocks rely on no such invariant — they are
  unsound.** Any comment written here would be false, and `tech-stack.md` already
  states that nothing mechanical can catch a false SAFETY comment. Writing one to
  satisfy a lint would defeat the article the lint exists to serve.
- `src/pool/link.rs:109` and `:154` are the `MaybeUninit` + `addr_of!` offset
  computation that Phase 4 deletes wholesale. Documenting five unsafe operations in
  code scheduled for deletion is waste.

So these four carry `#[expect]` with a reason naming the owning phase, and the two UB
sites say plainly in the reason that the block is known-unsound. The marker is a
**defect record**, not a suppression.

### Dev-dependencies added now, unused

`proptest`, `trybuild` and `approx` are added in this phase even though nothing uses
them until Phases 5, 6 and 9. Following the roadmap as written; the cost is three
unused dev-deps in the lockfile, and the benefit is that `tech-stack.md`'s planned
table and `Cargo.toml` agree from here on.

### Gate tension: Miri stays red, and that is recorded rather than hidden

`tech-stack.md` says every phase must leave all four gates green, and Article 1 says a
phase that leaves Miri red is not done. **Phase 0 cannot satisfy that**: the UB Miri
reports is precisely Phase 2's content, and fixing it here would collapse the two
phases.

Resolution: Phase 0's own **Done when** is the roadmap's — clippy clean and
`safety_docs` passing. `cargo fmt --check` is added since it is free. Miri is run and
its diagnostic is pasted into `validation.md` as the recorded Phase 2 baseline, so the
red gate is a written, dated fact rather than an omission. Article 1 is satisfied at
Phase 2, one phase later, and the roadmap already schedules it there.

If this reading is wrong and the intent was that no phase may merge with Miri red,
then Phase 0 and Phase 2 have to merge into one phase — flagging it rather than
deciding it silently.

## Context

### The safety docs are dissertation material

The 7 `# Safety` contracts are not lint-appeasement. They are the crate's core
soundness argument, and `mission.md` requires that "a reader who has never seen the
code must be able to follow why it is safe from the docs alone." They will be quoted
in the dissertation. Write them as prose an outside reader can follow — slice count,
slice length, alignment, validity — not terse one-liners. `from_slices` and
`from_slices_mut` on the two traits (lines 55 and 60) are the load-bearing pair;
everything else in the crate's safety story rests on them.

This is also the gap nothing else closes: five of the seven are private `unsafe fn`s
that `clippy::missing_safety_doc` will never fire on, which is exactly why
`tests/safety_docs.rs` exists.

### Zero behaviour change is a hard rule

Phase 0 establishes the measuring instruments; it does not use them. Any clippy fix
that would alter runtime semantics belongs to its owning phase. This binds most
sharply on the two `float_cmp` warnings in `src/system/discrete/` — `holder.rs:98` and
`mod.rs:48` — which sit next to the `FirstOrderHold` divide-by-zero NaN that Phase 7
owns. Changing a float comparison there is a numerical change wearing a lint fix's
clothes. `#[expect]` them.

Verification is concrete: the `src/main.rs` demo must produce byte-identical output
before and after.

### Contracts must survive Phases 2 and 3

The contracts describe today's code, but two scheduled changes touch what they say:

- **Phase 2** fixes how `raw_update`'s callers derive the pointers passed to
  `from_slices_mut`. The *declaration* contract (what a caller must guarantee) should
  be unaffected; the `// SAFETY:` comments at `src/system/mod.rs:46-47` may need
  updating.
- **Phase 3** promotes the arity `debug_assert!`s in `from_slices` to `assert!`,
  per Article 4. The contracts should therefore state the arity requirement as a
  **caller obligation**, not as "the debug_assert checks this" — phrased that way they
  stay true before and after Phase 3.

Write them so the Phase 2 and 3 diffs touch code, not contracts.

### Why this phase is first

Ordering principle from the roadmap: make the gates real, then fix what they catch.
Every later phase's correctness argument is measured by the four gates, so a gate that
is enabled but ignored is worse than one never enabled. Phase 0 is also the phase that
converts the roadmap's prose findings into machine-checked facts — the 19 hard errors
it surfaces are the first evidence that the gates work.
