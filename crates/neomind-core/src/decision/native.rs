//! Native ONNX inference — the zero-Python deployment path.
//!
//! Loads the pruned, dynamo-exported laya graph plus the ORIGINAL HF
//! tokenizer (`tokenizers` crate — same Rust code the Python side uses) and
//! the vocab-remap table produced by the pruning step. Everything the Python
//! `Agent` did for one question — template, tokenize, remap, forward,
//! temperature softmax — happens here, in-process.
//!
//! Port fidelity contract: `build_sequence`/`render_options` below must stay
//! byte-identical to `laya/common.py` (format:
//! `[CLS] "{type} question: {ins}" [SEP] [MASK]opt0 ... [SEP] state [SEP]`,
//! 48-token option cap, head-budget fallback). The golden test
//! (`native_golden`, env-gated) cross-checks picks against the Python
//! reference — run it whenever either side changes.

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;

use super::runtime::DecisionRuntime;
use super::types::{
    DecisionAnswer, DecisionError, DecisionQuestion, DecisionRequest, QuestionKind,
};

/// Temperatures per question kind: [choice, score, noul].
#[derive(Deserialize)]
struct LayaConfig {
    #[serde(default = "default_temps")]
    temperature: Vec<f32>,
    #[serde(default = "default_max_len")]
    max_len: usize,
    #[serde(default = "default_head_len")]
    head_max_len: usize,
}

fn default_temps() -> Vec<f32> {
    vec![1.0, 1.0, 1.0]
}
fn default_max_len() -> usize {
    1024
}
fn default_head_len() -> usize {
    256
}

/// Native decision backend over the pruned ONNX graph.
pub struct NativeOnnx {
    session: std::sync::Mutex<ort::session::Session>,
    tokenizer: tokenizers::Tokenizer,
    remap: Vec<i64>,
    temps: [f32; 3],
    max_len: usize,
    head_max_len: usize,
    cls: i64,
    sep: i64,
    mask: i64,
}

#[derive(Deserialize)]
struct RemapFile {
    remap: Vec<i64>,
}

impl NativeOnnx {
    /// `dir` is a fine-tuned pruned checkpoint directory containing
    /// `model.onnx` (+ `.onnx.data`), `tokenizer/tokenizer.json`,
    /// `rl_agent_config.json`, and `remap.json` (from the pruning step).
    pub fn load(dir: &Path) -> Result<Self, DecisionError> {
        let err = |e: String| DecisionError::Unavailable(format!("native decision backend: {e}"));
        let cfg: LayaConfig = serde_json::from_reader(
            std::fs::File::open(dir.join("rl_agent_config.json"))
                .map_err(|e| err(e.to_string()))?,
        )
        .map_err(|e| err(e.to_string()))?;
        let remap: RemapFile = serde_json::from_reader(
            std::fs::File::open(dir.join("remap.json")).map_err(|e| err(e.to_string()))?,
        )
        .map_err(|e| err(e.to_string()))?;

        // Default: fp32 `model.onnx` (100% port fidelity). The int8 build
        // (~140MB vs ~550MB) trades ~14% pick agreement on borderline
        // samples for 4x size — opt in explicitly via LAYA_NATIVE_INT8=1
        // for tight-memory deployments.
        let model_path = {
            if std::env::var("LAYA_NATIVE_INT8").map(|v| v == "1").unwrap_or(false) {
                let p = dir.join("model_int8.onnx");
                if p.exists() {
                    p
                } else {
                    dir.join("model.onnx")
                }
            } else {
                dir.join("model.onnx")
            }
        };
        let session = ort::session::Session::builder()
            .and_then(|b| b.commit_from_file(model_path))
            .map_err(|e| err(format!("onnx load: {e}")))?;
        let session = std::sync::Mutex::new(session);
        let tokenizer =
            tokenizers::Tokenizer::from_file(dir.join("tokenizer").join("tokenizer.json"))
                .map_err(|e| err(format!("tokenizer load: {e}")))?;

        // mmBERT-style tokenizers use <bos>/<eos>/<mask>; BERT-style use
        // [CLS]/[SEP]/[MASK]. Probe both so the backend loads either family.
        let probe = |cands: &[&str]| -> Result<i64, DecisionError> {
            for t in cands {
                if let Some(id) = tokenizer.token_to_id(t) {
                    return Ok(id as i64);
                }
            }
            Err(err(format!("tokenizer has none of {cands:?}")))
        };
        let cls = probe(&["[CLS]", "<bos>", "<cls>"])?;
        let sep = probe(&["[SEP]", "<eos>", "<sep>"])?;
        let mask = probe(&["[MASK]", "<mask>", "<mask_>"])?;
        let temps = [
            cfg.temperature[0],
            cfg.temperature[1],
            *cfg.temperature.last().unwrap_or(&1.0),
        ];
        Ok(Self {
            session,
            tokenizer,
            remap: remap.remap,
            temps,
            max_len: cfg.max_len,
            head_max_len: cfg.head_max_len,
            cls,
            sep,
            mask,
        })
    }

    /// `render_options` port — label-index order; noul is always [false, true].
    fn render_options(q: &DecisionQuestion) -> Vec<String> {
        match q.kind {
            QuestionKind::Choice => q
                .criteria
                .iter()
                .map(|(k, d)| {
                    if d.is_empty() {
                        k.clone()
                    } else {
                        format!("{k}: {d}")
                    }
                })
                .collect(),
            QuestionKind::Score => q
                .criteria
                .iter()
                .enumerate()
                .map(|(i, (_, d))| format!("level {i}: {d}"))
                .collect(),
            QuestionKind::Noul => vec![
                "false: no, the statement does not hold".into(),
                "true: yes, the statement holds".into(),
            ],
        }
    }

    fn encode(&self, text: &str) -> Vec<u32> {
        // add_special_tokens=false is the Python contract; unwrap of a
        // user-independent encode is acceptable here (same text each call).
        self.tokenizer
            .encode(text, false)
            .map(|e| e.get_ids().to_vec())
            .unwrap_or_default()
    }

    /// `build_sequence` port. Returns (ids, marker positions).
    fn build_sequence(&self, state: &str, q: &DecisionQuestion) -> (Vec<i64>, Vec<usize>) {
        let opts = Self::render_options(q);
        let kind = match q.kind {
            QuestionKind::Choice => "choice",
            QuestionKind::Score => "score",
            QuestionKind::Noul => "noul",
        };
        let mut head_ids: Vec<u32> = self.encode(&format!("{kind} question: {}", q.instructions));
        let mut opt_ids: Vec<Vec<u32>> = opts
            .iter()
            .map(|o| {
                let mut v = vec![self.mask as u32];
                v.extend(self.encode(&format!(" {o}")).into_iter().take(48));
                v
            })
            .collect();
        let total: usize = opt_ids.iter().map(|o| o.len()).sum();
        let mut opt_budget = self.head_max_len as isize - total as isize;
        if opt_budget < 16 {
            let per = std::cmp::max(
                4,
                ((self.head_max_len as isize - 16) as usize) / std::cmp::max(1, opt_ids.len()),
            );
            opt_ids = opt_ids
                .into_iter()
                .map(|o| o.into_iter().take(per).collect())
                .collect();
            opt_budget = self.head_max_len as isize
                - opt_ids.iter().map(|o| o.len()).sum::<usize>() as isize;
        }
        head_ids.truncate(std::cmp::max(8, opt_budget as usize));

        let mut ids: Vec<i64> = vec![self.cls];
        ids.extend(head_ids.iter().map(|&i| i as i64));
        ids.push(self.sep);
        let mut markers = Vec::with_capacity(opt_ids.len());
        for o in &opt_ids {
            markers.push(ids.len());
            ids.extend(o.iter().map(|&i| i as i64));
        }
        ids.push(self.sep);

        let room = self.max_len.saturating_sub(ids.len() + 1);
        let st: Vec<u32> = self.encode(state).into_iter().take(room).collect();
        ids.extend(st.iter().map(|&i| i as i64));
        ids.push(self.sep);
        ids.truncate(self.max_len);

        let markers: Vec<usize> = markers.into_iter().filter(|&m| m < self.max_len).collect();
        (ids, markers)
    }

    fn softmax_temp(z: &[f32], temp: f32) -> Vec<f32> {
        let t = if temp <= 0.0 { 1.0 } else { temp };
        let m = z.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        let exps: Vec<f32> = z.iter().map(|v| (v - m) / t).map(|v| v.exp()).collect();
        let s: f32 = exps.iter().sum();
        if s <= 0.0 {
            vec![1.0 / z.len().max(1) as f32; z.len()]
        } else {
            exps.iter().map(|e| e / s).collect()
        }
    }

    fn answer_from_probs(&self, q: &DecisionQuestion, probs: &[f32]) -> DecisionAnswer {
        let confidence = probs.iter().cloned().fold(0.0f32, f32::max);
        match q.kind {
            QuestionKind::Choice | QuestionKind::Score => DecisionAnswer {
                kind: q.kind,
                choice: match q.kind {
                    QuestionKind::Choice => q
                        .criteria
                        .get(
                            probs
                                .iter()
                                .enumerate()
                                .max_by(|a, b| a.1.total_cmp(b.1))
                                .map(|(i, _)| i)
                                .unwrap_or(0),
                        )
                        .map(|(k, _)| k.clone()),
                    _ => None,
                },
                noul: None,
                score: match q.kind {
                    QuestionKind::Score => Some(
                        probs
                            .iter()
                            .enumerate()
                            .map(|(i, p)| i as f32 * p)
                            .sum::<f32>(),
                    ),
                    _ => None,
                },
                answer_confidence: Some(confidence),
                probabilities: Some(
                    q.criteria
                        .iter()
                        .zip(probs.iter())
                        .map(|((k, _), p)| (k.clone(), *p))
                        .collect(),
                ),
            },
            QuestionKind::Noul => DecisionAnswer {
                kind: QuestionKind::Noul,
                choice: None,
                noul: probs.get(1).copied(),
                score: None,
                answer_confidence: Some(confidence),
                probabilities: Some(
                    [
                        ("false".into(), probs.first().copied().unwrap_or(0.0)),
                        ("true".into(), probs.get(1).copied().unwrap_or(0.0)),
                    ]
                    .into_iter()
                    .collect(),
                ),
            },
        }
    }
}

#[async_trait]
impl DecisionRuntime for NativeOnnx {
    fn backend_id(&self) -> &str {
        "native-onnx"
    }

    async fn decide(
        &self,
        req: &DecisionRequest,
    ) -> Result<Vec<(String, DecisionAnswer)>, DecisionError> {
        let mut out = Vec::with_capacity(req.questions.len());
        for (qid, q) in &req.questions {
            let (ids, markers) = self.build_sequence(&req.state, q);
            if markers.is_empty() {
                return Err(DecisionError::Protocol(format!(
                    "question `{qid}` produced no in-range markers"
                )));
            }
            let rids: Vec<i64> = ids
                .iter()
                .map(|&i| *self.remap.get(i as usize).unwrap_or(&0))
                .collect();
            let batch = 1usize;
            let seq = rids.len();
            let k = markers.len();
            let attention: Vec<i64> = vec![1; seq];
            let marker_mask: Vec<bool> = vec![true; k];
            let qtype: i64 = match q.kind {
                QuestionKind::Choice => 0,
                QuestionKind::Score => 1,
                QuestionKind::Noul => 2,
            };
            let _ = batch;

            let run = || -> Result<Vec<f32>, DecisionError> {
                let mut session = self
                    .session
                    .lock()
                    .map_err(|_| DecisionError::Unavailable("native session poisoned".into()))?;
                let mk = |shape: Vec<usize>, data: Vec<i64>| {
                    ort::value::Tensor::from_array((shape, data))
                        .map_err(|e| DecisionError::Protocol(format!("tensor build: {e}")))
                };
                let mk_bool = |shape: Vec<usize>, data: Vec<bool>| {
                    ort::value::Tensor::from_array((shape, data))
                        .map_err(|e| DecisionError::Protocol(format!("tensor build: {e}")))
                };
                let outputs = session
                    .run(ort::inputs![
                        "input_ids" => mk(vec![1, seq], rids.clone())?,
                        "attention_mask" => mk(vec![1, seq], attention.clone())?,
                        "marker_pos" => mk(vec![1, k], markers.clone().into_iter().map(|m| m as i64).collect())?,
                        "marker_mask" => mk_bool(vec![1, k], marker_mask.clone())?,
                        "qtype" => mk(vec![1], vec![qtype])?,
                    ])
                    .map_err(|e| DecisionError::Protocol(format!("onnx run: {e}")))?;
                let (shape, data) = outputs["logits"]
                    .try_extract_tensor::<f32>()
                    .map_err(|e| DecisionError::Protocol(format!("onnx output: {e}")))?;
                // shape == [1, k]: flatten row 0.
                let row: Vec<f32> = data.to_vec();
                let _ = shape;
                Ok(row)
            };
            let z = run()?;
            let temp = match q.kind {
                QuestionKind::Choice => self.temps[0],
                QuestionKind::Score => self.temps[1],
                QuestionKind::Noul => self.temps[2],
            };
            let probs = Self::softmax_temp(&z, temp);
            out.push((qid.clone(), self.answer_from_probs(q, &probs)));
        }
        Ok(out)
    }
}

/// Convenience: `Arc<dyn DecisionRuntime>` from a checkpoint dir.
pub fn native_runtime(dir: &Path) -> Result<Arc<dyn DecisionRuntime>, DecisionError> {
    Ok(Arc::new(NativeOnnx::load(dir)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn softmax_temperature_matches_reference_shape() {
        let p = NativeOnnx::softmax_temp(&[2.0, 0.0], 1.0);
        assert!((p[0] - 0.88).abs() < 0.01, "{p:?}");
        // Higher temperature flattens.
        let q = NativeOnnx::softmax_temp(&[2.0, 0.0], 5.0);
        assert!(q[0] < p[0], "temperature must soften");
    }

    #[test]
    fn render_options_noul_order_is_false_then_true() {
        let q = DecisionQuestion::noul("x?");
        let o = NativeOnnx::render_options(&q);
        assert_eq!(o.len(), 2);
        assert!(o[0].starts_with("false"));
        assert!(o[1].starts_with("true"));
    }

    /// Golden cross-check against the Python reference. Requires the pruned
    /// checkpoint produced by the sandbox (TRAINING.md "词表裁剪"):
    /// `NEOMIND_NATIVE_DIR=<laya-eval>/training/native_dir` … see
    /// NATIVE_RUST.md for the exact layout; run explicitly with --ignored.
    #[tokio::test]
    #[ignore = "requires NEOMIND_NATIVE_DIR (pruned ONNX checkpoint dir)"]
    async fn native_golden() {
        let dir = match std::env::var("NEOMIND_NATIVE_DIR") {
            Ok(d) => d,
            Err(_) => return,
        };
        let rt = native_runtime(Path::new(&dir)).expect("load native backend");
        let req = DecisionRequest::new("User request: list all devices").with_question(
                "simple",
                DecisionQuestion::noul(
                    "Can this request be completed with a single direct command or lookup, without multi-step reasoning, correlation or drafting?",
                ),
            );
        let out = rt.decide(&req).await.expect("decide");
        assert_eq!(out.len(), 1);
        assert!(out[0].1.noul.is_some(), "noul answer must carry P(true)");
    }

    /// Batch port-fidelity contract: the Rust template/tokenizer/remap path
    /// must reproduce the Python reference's top-1 picks on heldout states.
    /// `golden.jsonl` (one row per heldout sample, `expect` = Python pick of
    /// the SAME pruned weights) is generated by the sandbox; agreement below
    /// 90% means the port drifted — fix the port, never the goldens.
    #[tokio::test]
    #[ignore = "requires NEOMIND_NATIVE_DIR (pruned ONNX checkpoint dir + golden.jsonl)"]
    async fn native_golden_batch() {
        let dir = match std::env::var("NEOMIND_NATIVE_DIR") {
            Ok(d) => d,
            Err(_) => return,
        };
        let rt = native_runtime(Path::new(&dir)).expect("load native backend");
        let goldens = std::fs::read_to_string(Path::new(&dir).join("golden.jsonl"))
            .expect("golden.jsonl next to the checkpoint");
        let mut agree = 0usize;
        let mut total = 0usize;
        for line in goldens.lines() {
            let row: serde_json::Value = serde_json::from_str(line).expect("golden row");
            let kind = row["kind"].as_str().unwrap_or("choice");
            let q = if kind == "noul" {
                DecisionQuestion::noul(row["ins"].as_str().unwrap_or(""))
            } else {
                let crit: Vec<(String, String)> = row["crit"]
                    .as_object()
                    .map(|o| {
                        o.iter()
                            .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
                            .collect()
                    })
                    .unwrap_or_default();
                DecisionQuestion::choice(row["ins"].as_str().unwrap_or(""), crit)
            };
            let req =
                DecisionRequest::new(row["state"].as_str().unwrap_or("")).with_question("q", q);
            let out = rt.decide(&req).await.expect("decide");
            let got: String = match kind {
                "noul" => match out[0].1.noul_bool() {
                    Some(true) => "__t__".to_owned(),
                    Some(false) => "__f__".to_owned(),
                    None => "?".to_owned(),
                },
                _ => out[0].1.picked().unwrap_or("?").to_owned(),
            };
            let expect = row["expect"].as_str().unwrap_or("?");
            agree += usize::from(got == expect);
            total += 1;
        }
        let pct = 100 * agree / total.max(1);
        println!("native vs python picks: {agree}/{total} = {pct}%");
        assert!(pct >= 90, "port fidelity below 90%: {pct}%");
    }
}
