#![warn(clippy::pedantic)]
#![deny(clippy::perf)]
// --- Article 3: unsafe must justify itself ---
#![deny(unsafe_op_in_unsafe_fn)] // forces explicit blocks inside `unsafe fn`
#![deny(clippy::undocumented_unsafe_blocks)] // `// SAFETY:` on every block and `unsafe impl`
#![deny(clippy::missing_safety_doc)] // `# Safety` on exported `unsafe fn` / `unsafe trait`
#![deny(clippy::multiple_unsafe_ops_per_block)] // one op per block, so the comment means something
#![deny(clippy::unnecessary_safety_comment)] // catches SAFETY comments left on safe code
#![deny(clippy::unnecessary_safety_doc)]
#![deny(clippy::missing_panics_doc)]
#![deny(clippy::missing_errors_doc)]

pub mod pool;
pub mod system;
