//! Content-channel think-marker repair for the llama.cpp backend.
//!
//! llama-server splits balanced `<think>...</think>` spans into
//! `reasoning_content`, but under a dynamic thinking toggle Spark-X2.6
//! sometimes emits a closing `</think>` whose opening tag never reached the
//! wire (the chat template prefilled or consumed it). The server then leaves
//! the stray closer — and the reasoning text preceding it — in the `content`
//! channel. That text is channel misclassification, not prose: forwarded
//! verbatim it reaches the visible reply and persists into history, priming
//! later turns to leak again. This filter restores the channel split on our
//! side of the wire.
//!
//! Two hardening rules scope the repair (leakfix-r2 hardening round):
//! - Markers inside markdown code context (fences, inline code) are literal
//!   text, never protocol — every leak observed in the corpus sat in plain
//!   prose. A marker mentioned in PLAIN prose is still treated as protocol;
//!   that residual ambiguity is documented, not solved.
//! - The grace hold is caller-controlled: only requests that finally sent
//!   `enable_thinking=false` (the leak-prone mode) buffer the opening run;
//!   thinking-on requests stream content with only a ≤7-char marker-prefix
//!   holdback.
//!
//! A stream without markers must pass through byte-identical.

const OPEN_TAG: &str = "<think>";
const CLOSE_TAG: &str = "</think>";

/// Content-channel characters held back while a stray closing marker could
/// still reclassify the opening text as reasoning. Sized from the leakfix-r2
/// capture corpus via the pre-registered rule in
/// `/tmp/neomind-leakfix-r2/acceptance.json`: 9/9 marked requests had
/// prefixes 172–1078 chars (G=1024 covered only 8/9), so the smallest
/// qualifying value is 2048.
pub(crate) const GRACE_CHARS: usize = 2048;

/// Longest proper prefix of either marker (`</think>` minus one char).
const MAX_PARTIAL_MARKER_CHARS: usize = CLOSE_TAG.len() - 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Start of the content channel: a stray closer may still arrive and
    /// reclassify everything so far as reasoning, so hold up to the grace
    /// budget before the first content emission.
    Probing,
    /// Inside an unclosed `<think>` that arrived in the content channel:
    /// everything until the closing tag (or stream end) is reasoning.
    InsideThink,
    /// Past the first closing marker or the grace budget: stream content
    /// through, strip stray markers, re-enter [`Phase::InsideThink`] on a
    /// new opening tag.
    Streaming,
}

/// What one response's filter pass repaired, for observability.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ThinkFilterStats {
    /// Marker occurrences removed from the content channel.
    pub(crate) markers_stripped: u32,
    /// Characters moved from the content channel to the thinking channel.
    pub(crate) chars_reclassified: usize,
}

/// Markdown code context carried across chunk boundaries so markers inside
/// code fences and inline code spans stay literal text. Subset of CommonMark:
/// fences are line-leading runs of ≥3 backticks/tildes closed by a line-
/// leading run of ≥ the opening length; inline spans are backtick runs
/// closed by a run of the exact same length and abandoned at a blank line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CodeState {
    fence: Option<(char, usize)>,
    inline: Option<usize>,
    at_line_start: bool,
}

impl Default for CodeState {
    fn default() -> Self {
        Self {
            fence: None,
            inline: None,
            at_line_start: true,
        }
    }
}

fn marker_kind_at(text: &str, pos: usize) -> Option<bool> {
    if text[pos..].starts_with(OPEN_TAG) {
        Some(true)
    } else if text[pos..].starts_with(CLOSE_TAG) {
        Some(false)
    } else {
        None
    }
}

/// Walk `text` continuing from `state`, returning the first think marker
/// that lies OUTSIDE code context — `(byte position, tag length, is_open)` —
/// together with the code state at the stopping point (at the marker, or
/// after all of `text` when no marker was found). The marker itself is not
/// consumed and does not change code state.
fn walk(text: &str, mut state: CodeState) -> (Option<(usize, usize, bool)>, CodeState) {
    let mut it = text.char_indices().peekable();
    while let Some((i, c)) = it.next() {
        if c == '`' || c == '~' {
            let mut len = 1usize;
            while let Some(&(_, next)) = it.peek() {
                if next == c {
                    it.next();
                    len += 1;
                } else {
                    break;
                }
            }
            if let Some((fence_char, fence_len)) = state.fence {
                if state.at_line_start && c == fence_char && len >= fence_len {
                    state.fence = None;
                }
            } else if let Some(inline_len) = state.inline {
                if c == '`' && len == inline_len {
                    state.inline = None;
                }
            } else if state.at_line_start && len >= 3 {
                state.fence = Some((c, len));
            } else if c == '`' {
                state.inline = Some(len);
            }
            state.at_line_start = false;
        } else {
            if c == '<' && state.fence.is_none() && state.inline.is_none() {
                if let Some(is_open) = marker_kind_at(text, i) {
                    let tag_len = if is_open {
                        OPEN_TAG.len()
                    } else {
                        CLOSE_TAG.len()
                    };
                    return (Some((i, tag_len, is_open)), state);
                }
            }
            if c == '\n' {
                // A blank line abandons an unclosed inline span, mirroring
                // paragraph scope: one stray backtick must not disable
                // repair for the rest of the reply.
                if state.at_line_start {
                    state.inline = None;
                }
                state.at_line_start = true;
            } else {
                state.at_line_start = false;
            }
        }
    }
    (None, state)
}

/// Length in chars of the longest trailing partial-marker suffix of `s`
/// (0 when the tail cannot extend into either marker). Callers must have
/// consumed any complete marker first.
fn partial_marker_suffix_len(s: &str) -> usize {
    let total = s.chars().count();
    let probe = total.min(MAX_PARTIAL_MARKER_CHARS);
    for k in (1..=probe).rev() {
        let suffix: String = s.chars().skip(total - k).collect();
        if OPEN_TAG.starts_with(&suffix) || CLOSE_TAG.starts_with(&suffix) {
            return k;
        }
    }
    0
}

/// Byte offset after the first `chars_to_emit` chars of `s`.
fn byte_offset_after_chars(s: &str, chars_to_emit: usize) -> usize {
    s.char_indices()
        .nth(chars_to_emit)
        .map(|(i, _)| i)
        .unwrap_or(s.len())
}

/// Stateful splitter for one response's `content` channel.
///
/// Feed every content delta via [`ThinkContentFilter::feed`], then
/// [`ThinkContentFilter::finish`] once the stream ends; both return
/// `(thinking, content)` chunks to emit on the respective channels.
pub(crate) struct ThinkContentFilter {
    phase: Phase,
    pending: String,
    code: CodeState,
    stats: ThinkFilterStats,
    grace: usize,
}

impl ThinkContentFilter {
    /// Grace budget for requests that disabled thinking; `0` (thinking-on
    /// requests) disables the opening-run hold: stray closers strip the
    /// marker only, balanced blocks and unclosed opens are still repaired
    /// without any hold.
    pub(crate) fn with_grace(grace_chars: usize) -> Self {
        Self {
            phase: Phase::Probing,
            pending: String::new(),
            code: CodeState::default(),
            stats: ThinkFilterStats::default(),
            grace: grace_chars,
        }
    }

    pub(crate) fn stats(&self) -> ThinkFilterStats {
        self.stats
    }

    pub(crate) fn feed(&mut self, delta: &str) -> (String, String) {
        self.pending.push_str(delta);
        self.process(false)
    }

    /// Flush everything still held; an unclosed `<think>` sends its tail to
    /// the thinking channel, unmarked probing text goes out as content.
    pub(crate) fn finish(&mut self) -> (String, String) {
        self.process(true)
    }

    fn process(&mut self, flush: bool) -> (String, String) {
        let mut thinking = String::new();
        let mut content = String::new();
        loop {
            match self.phase {
                Phase::Probing => {
                    let (hit, state_at_hit) = walk(&self.pending, self.code);
                    match hit {
                        Some((pos, tag_len, true)) => {
                            // A genuine answer prefix before an explicit
                            // think block stays visible.
                            content.push_str(&self.pending[..pos]);
                            self.pending.drain(..pos + tag_len);
                            self.code = state_at_hit;
                            self.stats.markers_stripped += 1;
                            self.phase = Phase::InsideThink;
                        }
                        Some((pos, tag_len, false)) => {
                            // Stray closer in plain prose with no opening tag
                            // on the wire: the opening run was reasoning the
                            // server misrouted — reclassify it.
                            let upto = pos + tag_len;
                            self.stats.markers_stripped += 1;
                            self.stats.chars_reclassified += self.pending[..upto].chars().count();
                            thinking.push_str(&self.pending[..upto]);
                            self.pending.drain(..upto);
                            self.code = state_at_hit;
                            self.phase = Phase::Streaming;
                        }
                        None => {
                            if flush || self.pending.chars().count() > self.grace {
                                let keep = if flush {
                                    0
                                } else {
                                    partial_marker_suffix_len(&self.pending)
                                };
                                let split = byte_offset_after_chars(
                                    &self.pending,
                                    self.pending.chars().count() - keep,
                                );
                                self.code = walk(&self.pending[..split], self.code).1;
                                content.push_str(&self.pending[..split]);
                                self.pending.drain(..split);
                                self.phase = Phase::Streaming;
                                continue;
                            }
                            break;
                        }
                    }
                }
                Phase::InsideThink => {
                    let (hit, state_at_hit) = walk(&self.pending, self.code);
                    match hit {
                        Some((pos, tag_len, false)) => {
                            let upto = pos + tag_len;
                            self.stats.markers_stripped += 1;
                            self.stats.chars_reclassified += self.pending[..upto].chars().count();
                            thinking.push_str(&self.pending[..upto]);
                            self.pending.drain(..upto);
                            self.code = state_at_hit;
                            self.phase = Phase::Streaming;
                        }
                        Some((pos, tag_len, true)) => {
                            // A nested open inside an unclosed block is not a
                            // protocol boundary — consume it and keep the
                            // reasoning channel open.
                            self.pending.drain(..pos + tag_len);
                            self.code = state_at_hit;
                        }
                        None => {
                            let keep = if flush {
                                0
                            } else {
                                partial_marker_suffix_len(&self.pending)
                            };
                            let split = byte_offset_after_chars(
                                &self.pending,
                                self.pending.chars().count() - keep,
                            );
                            self.code = walk(&self.pending[..split], self.code).1;
                            self.stats.chars_reclassified += self.pending[..split].chars().count();
                            thinking.push_str(&self.pending[..split]);
                            self.pending.drain(..split);
                            break;
                        }
                    }
                }
                Phase::Streaming => {
                    let (hit, state_at_hit) = walk(&self.pending, self.code);
                    match hit {
                        Some((pos, tag_len, true)) => {
                            content.push_str(&self.pending[..pos]);
                            self.pending.drain(..pos + tag_len);
                            self.code = state_at_hit;
                            self.stats.markers_stripped += 1;
                            self.phase = Phase::InsideThink;
                        }
                        Some((pos, tag_len, false)) => {
                            // Additional stray closers: drop the marker, keep
                            // the text around it as content.
                            content.push_str(&self.pending[..pos]);
                            self.pending.drain(..pos + tag_len);
                            self.code = state_at_hit;
                            self.stats.markers_stripped += 1;
                        }
                        None => {
                            let keep = if flush {
                                0
                            } else {
                                partial_marker_suffix_len(&self.pending)
                            };
                            let split = byte_offset_after_chars(
                                &self.pending,
                                self.pending.chars().count() - keep,
                            );
                            self.code = walk(&self.pending[..split], self.code).1;
                            content.push_str(&self.pending[..split]);
                            self.pending.drain(..split);
                            break;
                        }
                    }
                }
            }
        }
        (thinking, content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(deltas: &[&str]) -> (String, String, ThinkFilterStats) {
        let mut filter = ThinkContentFilter::with_grace(GRACE_CHARS);
        run_with(&mut filter, deltas)
    }

    fn run_with(
        filter: &mut ThinkContentFilter,
        deltas: &[&str],
    ) -> (String, String, ThinkFilterStats) {
        let mut thinking = String::new();
        let mut content = String::new();
        for d in deltas {
            let (t, c) = filter.feed(d);
            thinking.push_str(&t);
            content.push_str(&c);
        }
        let (t, c) = filter.finish();
        thinking.push_str(&t);
        content.push_str(&c);
        (thinking, content, filter.stats())
    }

    #[test]
    fn clean_stream_passes_through_byte_identical() {
        let deltas = [
            "好的，我来处理。",
            "首先读取传感器数据，",
            "然后 name it \"sensor-002\".",
            " Done. </other> stays.",
        ];
        let joined = deltas.concat();
        let (thinking, content, stats) = run(&deltas);
        assert_eq!(content, joined);
        assert_eq!(thinking, "");
        assert_eq!(stats, ThinkFilterStats::default());
    }

    #[test]
    fn stray_closer_with_prefix_is_reclassified() {
        let (thinking, content, stats) = run(&["让我先想想。", "</th", "ink>正式回答"]);
        assert_eq!(thinking, "让我先想想。</think>");
        assert_eq!(content, "正式回答");
        assert_eq!(stats.markers_stripped, 1);
        assert!(stats.chars_reclassified > 0);
    }

    #[test]
    fn stray_closer_at_position_zero() {
        let (thinking, content, _) = run(&["</think>", "Hello"]);
        assert_eq!(thinking, "</think>");
        assert_eq!(content, "Hello");
    }

    #[test]
    fn balanced_block_in_content_moves_to_thinking() {
        let (thinking, content, stats) = run(&["答案A<thi", "nk>中间思考</thi", "nk>答案B"]);
        assert_eq!(thinking, "中间思考</think>");
        assert_eq!(content, "答案A答案B");
        assert_eq!(stats.markers_stripped, 2);
    }

    #[test]
    fn unclosed_open_directs_tail_to_thinking() {
        let (thinking, content, _) = run(&["答案A<think>想了很多，还没完"]);
        assert_eq!(content, "答案A");
        assert_eq!(thinking, "想了很多，还没完");
    }

    #[test]
    fn marker_beyond_grace_budget_strips_marker_only() {
        let filler: String = "字".repeat(GRACE_CHARS + 10);
        let (thinking, content, stats) = run(&[&filler, "</think>", "真实回答"]);
        assert_eq!(content, format!("{}真实回答", filler));
        assert_eq!(thinking, "");
        assert_eq!(stats.markers_stripped, 1);
        assert_eq!(stats.chars_reclassified, 0);
    }

    #[test]
    fn prefix_exactly_at_grace_budget_is_still_reclassified() {
        let prefix: String = "字".repeat(GRACE_CHARS);
        let (thinking, content, _) = run(&[&prefix, "</think>尾巴"]);
        assert_eq!(thinking, format!("{}</think>", prefix));
        assert_eq!(content, "尾巴");
    }

    #[test]
    fn additional_stray_closers_after_first_strip_marker_only() {
        let (thinking, content, stats) = run(&["a</think>b</think>c"]);
        assert_eq!(thinking, "a</think>");
        assert_eq!(content, "bc");
        assert_eq!(stats.markers_stripped, 2);
    }

    #[test]
    fn holdback_releases_partial_marker_tail_on_next_feed() {
        let mut filter = ThinkContentFilter::with_grace(GRACE_CHARS);
        let (t1, c1) = filter.feed("推理</thi");
        assert_eq!((t1.as_str(), c1.as_str()), ("", ""));
        let (t2, c2) = filter.feed("nk>答案");
        assert_eq!((t2.as_str(), c2.as_str()), ("推理</think>", "答案"));
        let (t3, c3) = filter.finish();
        assert_eq!((t3.as_str(), c3.as_str()), ("", ""));
    }

    #[test]
    fn streaming_after_grace_still_reclassifies_new_think_block() {
        let filler: String = "a".repeat(GRACE_CHARS + 5);
        let (thinking, content, _) = run(&[&filler, "<think>二段思考</think>尾"]);
        assert_eq!(thinking, "二段思考</think>");
        assert_eq!(content, format!("{}尾", filler));
    }

    #[test]
    fn grace_hold_does_not_split_multibyte_chars() {
        let mut filter = ThinkContentFilter::with_grace(GRACE_CHARS);
        let chunk1 = "开".repeat(GRACE_CHARS - 1);
        let (t1, c1) = filter.feed(&format!("{}梁", chunk1));
        assert_eq!(t1, "");
        let (t2, c2) = filter.finish();
        let _ = t2;
        let emitted = format!("{}{}", c1, c2);
        assert!(emitted.ends_with('梁'));
        assert_eq!(emitted.chars().count(), GRACE_CHARS);
    }

    // ---- code-context literalness (hardening round) ----

    #[test]
    fn inline_code_marker_is_literal() {
        let deltas = ["Use `</think>` to close ", "the reasoning block."];
        let joined = deltas.concat();
        let (thinking, content, stats) = run(&deltas);
        assert_eq!(content, joined);
        assert_eq!(thinking, "");
        assert_eq!(stats, ThinkFilterStats::default());
    }

    #[test]
    fn fenced_marker_is_literal() {
        let deltas = ["Example:\n```\n<think>x</think>\n```\nDone."];
        let joined = deltas.concat();
        let (thinking, content, stats) = run(&deltas);
        assert_eq!(content, joined);
        assert_eq!(thinking, "");
        assert_eq!(stats, ThinkFilterStats::default());
    }

    #[test]
    fn tilde_fence_marker_is_literal() {
        let deltas = ["~~~\n</think>\n~~~\nafter"];
        let joined = deltas.concat();
        let (thinking, content, stats) = run(&deltas);
        assert_eq!(content, joined);
        assert_eq!(thinking, "");
        assert_eq!(stats, ThinkFilterStats::default());
    }

    #[test]
    fn inline_code_backtick_and_marker_split_across_chunks() {
        let deltas = ["Use `", "</", "think>` to close."];
        let joined = deltas.concat();
        let (thinking, content, stats) = run(&deltas);
        assert_eq!(content, joined);
        assert_eq!(thinking, "");
        assert_eq!(stats, ThinkFilterStats::default());
    }

    #[test]
    fn marker_split_across_chunks_inside_fence() {
        let deltas = ["```\ncode\n</th", "ink>\nstill code\n```", "\nafter"];
        let joined = deltas.concat();
        let (thinking, content, stats) = run(&deltas);
        assert_eq!(content, joined);
        assert_eq!(thinking, "");
        assert_eq!(stats, ThinkFilterStats::default());
    }

    #[test]
    fn unclosed_inline_span_abandoned_at_blank_line() {
        let deltas = ["oops `stays </think> literal\n", "\nreal answer"];
        let joined = deltas.concat();
        let (thinking, content, stats) = run(&deltas);
        assert_eq!(content, joined);
        assert_eq!(thinking, "");
        assert_eq!(stats, ThinkFilterStats::default());
    }

    #[test]
    fn unclosed_fence_to_stream_end_keeps_markers_literal() {
        let deltas = ["```\ncode </think> block never closed"];
        let joined = deltas.concat();
        let (thinking, content, stats) = run(&deltas);
        assert_eq!(content, joined);
        assert_eq!(thinking, "");
        assert_eq!(stats, ThinkFilterStats::default());
    }

    #[test]
    fn fence_inside_grace_window_does_not_swallow_prefix() {
        let deltas = ["Before:\n```\n</think>\n```\nafter ", "done"];
        let joined = deltas.concat();
        let (thinking, content, stats) = run(&deltas);
        assert_eq!(content, joined);
        assert_eq!(thinking, "");
        assert_eq!(stats, ThinkFilterStats::default());
    }

    /// DOCUMENTED RESIDUAL, deliberately pinned: a marker mentioned in PLAIN
    /// prose (no code formatting) still swallows the preceding answer text
    /// into the thinking channel. The corpus shows real leaks always look
    /// like this too, so this behavior is kept — it is NOT claimed as
    /// solved for legitimate plain-prose mentions.
    #[test]
    fn plain_prose_marker_mention_still_swallowed_documented_residual() {
        let (thinking, content, stats) = run(&["The closing tag </think> ends reasoning."]);
        assert_eq!(thinking, "The closing tag </think>");
        assert_eq!(content, " ends reasoning.");
        assert_eq!(stats.markers_stripped, 1);
        assert!(stats.chars_reclassified > 0);
    }

    // ---- conditional grace (hardening round) ----

    #[test]
    fn zero_grace_strips_stray_closer_marker_only() {
        let mut filter = ThinkContentFilter::with_grace(0);
        let (thinking, content, stats) = run_with(&mut filter, &["preface ", "</think>", "answer"]);
        assert_eq!(content, "preface answer");
        assert_eq!(thinking, "");
        assert_eq!(stats.markers_stripped, 1);
        assert_eq!(stats.chars_reclassified, 0);
    }

    #[test]
    fn zero_grace_still_reclassifies_balanced_block() {
        let mut filter = ThinkContentFilter::with_grace(0);
        let (thinking, content, _) = run_with(&mut filter, &["A<think>mid</think>B"]);
        assert_eq!(thinking, "mid</think>");
        assert_eq!(content, "AB");
    }

    #[test]
    fn zero_grace_still_moves_unclosed_open_tail() {
        let mut filter = ThinkContentFilter::with_grace(0);
        let (thinking, content, _) = run_with(&mut filter, &["A<think>tail"]);
        assert_eq!(content, "A");
        assert_eq!(thinking, "tail");
    }
}
