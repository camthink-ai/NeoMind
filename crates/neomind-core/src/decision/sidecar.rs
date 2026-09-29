//! HTTP client for the Jev-compatible `/v1/systemone` protocol.
//!
//! One client, two deployments: a local [laya](https://github.com/NandhaKishorM/laya)
//! sidecar (`laya-serve`, default) or a cloud Jev endpoint — the wire shape
//! is identical, so a deployment can swap cost/latency/privacy trade-offs
//! without touching callers.
//!
//! Wire contract (verified against `laya/serve.py`):
//! ```text
//! POST {base}/v1/systemone
//!   { "state": "...", "questions": {qid: {"type": "choice"|"score"|"noul",
//!        "instructions": "...", "criteria": {label: desc} | [desc, ...]}},
//!     "model": "convaiinnovations/laya-multilingual" (optional) }
//! 200 { "model": ..., "answers": {qid: {"type": ..., "choice": label |
//!        "noul": p_true | "score": v, "probabilities": {...},
//!        "answer_confidence": 0..1, ...}}, "usage": ..., "routing": ... }
//! ```
//! Parsing is tolerant by design: every answer field except the pick itself
//! is optional, because the fallback story ("treat missing confidence as
//! ungated") depends on `Option` rather than defaults.

use std::time::Duration;

use async_trait::async_trait;

use super::runtime::DecisionRuntime;
use super::types::{
    DecisionAnswer, DecisionError, DecisionQuestion, DecisionRequest, QuestionKind,
};

/// A `/v1/systemone` endpoint client.
pub struct LayaSidecar {
    base_url: String,
    model: Option<String>,
    client: reqwest::Client,
    timeout: Duration,
}

impl LayaSidecar {
    /// Point at a server root (e.g. `http://127.0.0.1:8000`).
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            model: None,
            client: reqwest::Client::new(),
            timeout: Duration::from_millis(300),
        }
    }

    /// Pin a checkpoint (e.g. `convaiinnovations/laya-multilingual`).
    /// Unset means "let the server's router choose".
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Per-decision budget. The rule-engine path wants ~200 ms before it
    /// treats a decision as "no match"; chat routing tolerates more.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Render one question into its wire JSON. `criteria` is an object for
    /// choice (label → description), an array for score (ladder), and omitted
    /// for noul — the exact shapes the engines were trained against.
    fn question_wire(q: &DecisionQuestion) -> serde_json::Value {
        let mut obj = serde_json::Map::new();
        let kind = match q.kind {
            QuestionKind::Choice => "choice",
            QuestionKind::Score => "score",
            QuestionKind::Noul => "noul",
        };
        obj.insert("type".into(), serde_json::Value::String(kind.into()));
        obj.insert(
            "instructions".into(),
            serde_json::Value::String(q.instructions.clone()),
        );
        match q.kind {
            QuestionKind::Choice => {
                let mut crit = serde_json::Map::new();
                for (label, desc) in &q.criteria {
                    crit.insert(label.clone(), serde_json::Value::String(desc.clone()));
                }
                obj.insert("criteria".into(), serde_json::Value::Object(crit));
            }
            QuestionKind::Score => {
                let ladder: Vec<serde_json::Value> = q
                    .criteria
                    .iter()
                    .map(|(_, desc)| serde_json::Value::String(desc.clone()))
                    .collect();
                obj.insert("criteria".into(), serde_json::Value::Array(ladder));
            }
            QuestionKind::Noul => {}
        }
        serde_json::Value::Object(obj)
    }

    fn build_body(req: &DecisionRequest, model: &Option<String>) -> serde_json::Value {
        let mut questions = serde_json::Map::new();
        for (id, q) in &req.questions {
            questions.insert(id.clone(), Self::question_wire(q));
        }
        let mut body = serde_json::Map::new();
        body.insert("state".into(), serde_json::Value::String(req.state.clone()));
        body.insert("questions".into(), serde_json::Value::Object(questions));
        if let Some(m) = model {
            body.insert("model".into(), serde_json::Value::String(m.clone()));
        }
        serde_json::Value::Object(body)
    }

    fn parse_answer(
        kind: QuestionKind,
        v: &serde_json::Value,
    ) -> Result<DecisionAnswer, DecisionError> {
        let get_f32 =
            |key: &str| -> Option<f32> { v.get(key).and_then(|x| x.as_f64()).map(|x| x as f32) };
        let probabilities = v
            .get("probabilities")
            .and_then(|p| p.as_object())
            .map(|obj| {
                obj.iter()
                    .filter_map(|(k, x)| x.as_f64().map(|f| (k.clone(), f as f32)))
                    .collect()
            });
        Ok(DecisionAnswer {
            kind,
            choice: v.get("choice").and_then(|c| c.as_str()).map(str::to_string),
            noul: get_f32("noul"),
            score: get_f32("score"),
            answer_confidence: get_f32("answer_confidence"),
            probabilities,
        })
    }
}

#[async_trait]
impl DecisionRuntime for LayaSidecar {
    fn backend_id(&self) -> &str {
        "laya-sidecar"
    }

    async fn is_available(&self) -> bool {
        self.client
            .get(format!("{}/health", self.base_url))
            .timeout(self.timeout)
            .send()
            .await
            .map(|r| r.status().is_success() || r.status().as_u16() == 401)
            .unwrap_or(false)
    }

    async fn warmup(&self) -> Result<(), DecisionError> {
        let req = DecisionRequest::new("warmup")
            .with_question("warmup", DecisionQuestion::noul("Is this a warmup probe?"));
        self.decide(&req).await.map(|_| ())
    }

    async fn decide(
        &self,
        req: &DecisionRequest,
    ) -> Result<Vec<(String, DecisionAnswer)>, DecisionError> {
        let url = format!("{}/v1/systemone", self.base_url);
        let resp = self
            .client
            .post(url)
            .timeout(self.timeout)
            .json(&Self::build_body(req, &self.model))
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    DecisionError::Timeout
                } else if e.is_connect() {
                    DecisionError::Unavailable(e.to_string())
                } else {
                    DecisionError::Transport(e.to_string())
                }
            })?;

        let status = resp.status();
        if !status.is_success() {
            // 422 names the offending question (server contract); surface it
            // verbatim — a reworded/oversized question is a programming fix,
            // not a runtime condition to swallow.
            let detail = resp.text().await.unwrap_or_default();
            return Err(DecisionError::Protocol(format!("HTTP {status}: {detail}")));
        }

        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| DecisionError::Protocol(e.to_string()))?;
        let answers = body
            .get("answers")
            .and_then(|a| a.as_object())
            .ok_or_else(|| DecisionError::Protocol("response missing `answers` object".into()))?;

        let mut out = Vec::with_capacity(req.questions.len());
        for (id, q) in &req.questions {
            let raw = answers.get(id).ok_or_else(|| {
                DecisionError::Protocol(format!("response missing answer for question `{id}`"))
            })?;
            out.push((id.clone(), Self::parse_answer(q.kind, raw)?));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_body_matches_engine_contract() {
        let req = DecisionRequest::new("User request: list devices")
            .with_question(
                "tool",
                DecisionQuestion::choice(
                    "Which tool?",
                    vec![
                        ("shell".into(), "CLI ops".into()),
                        ("vision".into(), "images".into()),
                    ],
                ),
            )
            .with_question("simple", DecisionQuestion::noul("Single command?"));
        let body = LayaSidecar::build_body(&req, &Some("m".into()));

        assert_eq!(body["state"], "User request: list devices");
        assert_eq!(body["model"], "m");
        assert_eq!(body["questions"]["tool"]["type"], "choice");
        assert_eq!(body["questions"]["tool"]["criteria"]["shell"], "CLI ops");
        // criteria order must survive into the object (trained option order)
        let crit = body["questions"]["tool"]["criteria"].as_object().unwrap();
        assert_eq!(crit.keys().collect::<Vec<_>>(), vec!["shell", "vision"]);
        assert_eq!(body["questions"]["simple"]["type"], "noul");
        assert!(body["questions"]["simple"].get("criteria").is_none());
    }

    #[test]
    fn parse_answer_tolerates_missing_fields() {
        let v: serde_json::Value =
            serde_json::from_str(r#"{"type":"choice","choice":"shell"}"#).unwrap();
        let a = LayaSidecar::parse_answer(QuestionKind::Choice, &v).unwrap();
        assert_eq!(a.picked(), Some("shell"));
        assert!(a.answer_confidence.is_none());
        assert!(!a.passes_gate(0.7));
    }

    #[test]
    fn parse_noul_answer() {
        let v: serde_json::Value = serde_json::from_str(
            r#"{"type":"noul","noul":0.91,"answer_confidence":0.88,"probabilities":{"false":0.09,"true":0.91}}"#,
        )
        .unwrap();
        let a = LayaSidecar::parse_answer(QuestionKind::Noul, &v).unwrap();
        assert_eq!(a.noul_bool(), Some(true));
        assert!(a.passes_gate(0.8));
        assert_eq!(a.probabilities.as_ref().unwrap()["true"], 0.91);
    }

    /// End-to-end against a real sidecar: `LAYA_SIDECAR_URL=http://127.0.0.1:8123`
    /// (sandbox: `LAYA_PORT=8123 LAYA_MODELS=multilingual python -m laya.serve`).
    /// Ignored by default — it needs a live server and is run explicitly:
    /// `cargo test -p neomind-core --lib decision -- --ignored`.
    #[tokio::test]
    #[ignore = "requires a live sidecar at LAYA_SIDECAR_URL"]
    async fn e2e_live_sidecar() {
        let url = match std::env::var("LAYA_SIDECAR_URL") {
            Ok(u) => u,
            Err(_) => return, // no server configured — nothing to assert
        };
        let sidecar = LayaSidecar::new(url)
            .with_model("convaiinnovations/laya-multilingual")
            .with_timeout(std::time::Duration::from_secs(20));
        assert!(sidecar.is_available().await, "sidecar must be healthy");

        let req = DecisionRequest::new("User request: reboot sensor roof-03")
            .with_question(
                "tool",
                DecisionQuestion::choice(
                    "Which tool should serve this user request?",
                    vec![
                        ("shell".into(), "ALL neomind CLI platform operations, INCLUDING device control commands".into()),
                        ("vision".into(), "understand or read the content of an image".into()),
                        ("none".into(), "pure conversation".into()),
                    ],
                ),
            )
            .with_question(
                "simple",
                DecisionQuestion::noul(
                    "Can this request be completed with a single direct command, without multi-step reasoning?",
                ),
            );
        let answers = sidecar
            .decide(&req)
            .await
            .expect("live decide must succeed");
        assert_eq!(answers.len(), 2, "every question gets an answer");
        for (id, a) in &answers {
            assert!(
                a.answer_confidence.is_some(),
                "`{id}` must report calibrated confidence"
            );
        }
    }
}
