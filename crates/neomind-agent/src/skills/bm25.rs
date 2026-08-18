//! Minimal BM25 over the skill corpus — dependency-free.
//!
//! The corpus is a few dozen skill descriptions; a full-text engine
//! (tantivy & friends) would add a dependency tree for zero benefit at this
//! scale. What BM25 adds over the matcher's flat `contains` signals is
//! **IDF weighting**: a query term that appears in every skill ("device",
//! "rule") contributes ~nothing, while a rare term ("LoRaWAN", "驼峰",
//! "onboarding") dominates — exactly the ranking `contains` cannot express.
//!
//! Tokenization: latin runs lowercase on non-alphanumerics; CJK text yields
//! character bigrams (a single CJK char is too ambiguous, and exact
//! substrings are already covered by the matcher's keyword signal — bigrams
//! catch reordered/partial phrasings like 泵停 vs 停泵).

use std::collections::HashMap;

const K1: f32 = 1.2;
const B: f32 = 0.75;

pub(crate) struct Bm25Index {
    docs: Vec<HashMap<String, usize>>, // term -> tf per doc
    df: HashMap<String, usize>,        // document frequency
    avg_len: f32,
    n_docs: f32,
}

impl Bm25Index {
    pub fn build<I, S>(docs: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut tokenized: Vec<Vec<String>> = Vec::new();
        for d in docs {
            tokenized.push(tokenize(d.as_ref()));
        }
        let n_docs = tokenized.len() as f32;
        let avg_len = if tokenized.is_empty() {
            0.0
        } else {
            tokenized.iter().map(|t| t.len() as f32).sum::<f32>() / n_docs
        };
        let mut doc_maps = Vec::with_capacity(tokenized.len());
        let mut df: HashMap<String, usize> = HashMap::new();
        for toks in &tokenized {
            let mut m: HashMap<String, usize> = HashMap::new();
            for t in toks {
                *m.entry(t.clone()).or_insert(0) += 1;
            }
            for term in m.keys() {
                *df.entry(term.clone()).or_insert(0) += 1;
            }
            doc_maps.push(m);
        }
        Self {
            docs: doc_maps,
            df,
            avg_len,
            n_docs,
        }
    }

    /// Raw BM25 score of one document against the query tokens. Unbounded
    /// above zero; ~3.0+ means strong rare-term overlap on this corpus size.
    pub fn score(&self, doc_idx: usize, query_tokens: &[String]) -> f32 {
        let Some(doc) = self.docs.get(doc_idx) else {
            return 0.0;
        };
        if self.n_docs == 0.0 || self.avg_len == 0.0 {
            return 0.0;
        }
        let len = doc.values().sum::<usize>() as f32;
        let mut total = 0.0f32;
        for qt in query_tokens {
            let Some(&tf_u) = doc.get(qt) else { continue };
            let tf = tf_u as f32;
            let df = *self.df.get(qt).unwrap_or(&0) as f32;
            // Robertson-Sparck-Jones IDF with the standard +1 smoothing so a
            // term present in every doc scores ~0 instead of negative.
            let idf = ((self.n_docs - df + 0.5) / (df + 0.5) + 1.0).ln();
            let tf_norm = tf * (K1 + 1.0) / (tf + K1 * (1.0 - B + B * len / self.avg_len));
            total += idf * tf_norm;
        }
        total
    }
}

/// Tokenize mixed latin/CJK text into BM25 terms.
pub(crate) fn tokenize(text: &str) -> Vec<String> {
    let lower = text.to_lowercase();
    let mut out = Vec::new();
    let mut latin_run = String::new();
    let mut cjk_run: Vec<char> = Vec::new();

    let flush_latin = |run: &mut String, out: &mut Vec<String>| {
        if run.len() >= 2 {
            out.push(run.clone());
        }
        run.clear();
    };
    let flush_cjk = |run: &mut Vec<char>, out: &mut Vec<String>| {
        if run.len() == 1 {
            // A lone CJK char between latin segments is noise-prone; drop
            // unless the whole token was a single char (caller handles).
        } else if run.len() >= 2 {
            for w in run.windows(2) {
                out.push(w.iter().collect::<String>());
            }
        }
        run.clear();
    };

    for ch in lower.chars() {
        if ch.is_ascii_alphanumeric() {
            if !cjk_run.is_empty() {
                flush_cjk(&mut cjk_run, &mut out);
            }
            latin_run.push(ch);
        } else if is_cjk(ch) {
            if !latin_run.is_empty() {
                flush_latin(&mut latin_run, &mut out);
            }
            cjk_run.push(ch);
        } else {
            flush_latin(&mut latin_run, &mut out);
            flush_cjk(&mut cjk_run, &mut out);
        }
    }
    flush_latin(&mut latin_run, &mut out);
    flush_cjk(&mut cjk_run, &mut out);
    out
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x4E00..=0x9FFF   // CJK Unified Ideographs
        | 0x3400..=0x4DBF // Extension A
        | 0xF900..=0xFAFF // Compatibility Ideographs
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenize_latin() {
        // single latin chars are dropped (stopword-class); multi-char runs kept
        assert_eq!(
            tokenize("Create a LoRaWAN bridge!"),
            vec!["create", "lorawan", "bridge"]
        );
    }

    #[test]
    fn test_tokenize_cjk_bigrams() {
        let t = tokenize("把泵停掉");
        assert!(t.contains(&"泵停".to_string()));
        assert!(t.contains(&"停掉".to_string()));
        assert!(!t.contains(&"把泵停掉".to_string()));
    }

    #[test]
    fn test_rare_term_beats_common_term() {
        let idx = Bm25Index::build([
            "device rule dashboard create",
            "device rule dashboard delete",
            "device LoRaWAN lorawan bridge onboarding",
            "device rule alert",
            "device dashboard widget",
            "device rule history",
            "device push channel",
            "device transform metric",
        ]);
        let q = tokenize("LoRaWAN");
        let s_rare = idx.score(2, &q);
        let s_common = idx.score(0, &q);
        assert!(s_rare > s_common);
        // "device" appears in every doc → +1-smoothed IDF stays barely
        // positive but far below the matcher's raw>1.0 contribution gate
        let q2 = tokenize("device");
        let common = idx.score(0, &q2);
        assert!(
            common < 0.2,
            "common term should stay negligible, got {common}"
        );
        assert!(common < s_rare / 10.0, "rare term should dominate by >10x");
    }

    #[test]
    fn test_cjk_rare_phrase_ranks_owner() {
        let idx = Bm25Index::build(["设备接入 onboarding mqtt", "规则管理 rule 告警 联动"]);
        let q = tokenize("帮我接入新的温度计");
        let s0 = idx.score(0, &q); // shares 帮我/接入/新的 bigrams
        let s1 = idx.score(1, &q);
        assert!(
            s0 > s1,
            "onboarding skill should outrank rule skill, {} vs {}",
            s0,
            s1
        );
    }

    #[test]
    fn test_empty_and_no_overlap() {
        let idx = Bm25Index::build(["alpha beta", "gamma delta"]);
        assert_eq!(idx.score(0, &tokenize("zzz qqq")), 0.0);
        let empty = Bm25Index::build(Vec::<String>::new());
        assert_eq!(empty.score(0, &tokenize("x")), 0.0);
    }
}
