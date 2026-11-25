//! Shared low-level primitives for LLM tool-calling loops.
//!
//! Used by both the chat loop (`neomind-agent`) and the standalone agent
//! runtime. Everything here was moved verbatim out of `neomind-agent` (M0
//! extraction): behaviour is unchanged, and the original `neomind-agent`
//! paths re-export from this crate.
//!
//! Generic message compaction is NOT here — it already lives one layer down
//! in `neomind_core::llm::compaction`.

pub mod narration_guard;
pub mod result_sanitizer;
pub mod tokenizer;
pub mod tool_parser;
pub mod types;
