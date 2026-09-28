//! Private synchronous TypeSafe System One client.

use std::collections::BTreeMap;
use std::env;
use std::fmt;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ureq::Agent;

const API_KEY_ENV: &str = "TYPESAFE_API_KEY";
const BASE_URL_ENV: &str = "TYPESAFE_BASE_URL";
const MODEL_ENV: &str = "TYPESAFE_DEFAULT_MODEL";
const DEFAULT_BASE_URL: &str = "https://api.typesafe.ai";
const DEFAULT_MODEL: &str = "jev-latest";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// Time for all requests of one run. The API is fast, so a run that needs
/// more than this means something went wrong.
const RUN_BUDGET: Duration = Duration::from_secs(20);
const IN_FLIGHT: usize = 4;

/// Why some or all checks could not run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum VerifyError {
    MissingApiKey,
    InvalidConfiguration(&'static str),
    Transport(String),
    Api { status: u16 },
    Decode(String),
    InvalidResponse(String),
    BudgetExceeded(Duration),
}

impl VerifyError {
    /// Every later request would be rejected the same way.
    fn is_credential_rejection(&self) -> bool {
        matches!(self, Self::Api { status: 401 | 403 })
    }
}

impl fmt::Display for VerifyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingApiKey => write!(formatter, "{API_KEY_ENV} is not set or is empty"),
            Self::InvalidConfiguration(name) => {
                write!(formatter, "{name} is set but empty or not valid UTF-8")
            }
            Self::Transport(message) => write!(formatter, "TypeSafe request failed: {message}"),
            Self::Api { status } => write!(formatter, "TypeSafe API returned HTTP {status}"),
            Self::Decode(message) => {
                write!(
                    formatter,
                    "could not decode the TypeSafe response: {message}"
                )
            }
            Self::InvalidResponse(message) => {
                write!(formatter, "invalid TypeSafe Choice response: {message}")
            }
            Self::BudgetExceeded(budget) => {
                write!(
                    formatter,
                    "time budget of {} s exceeded",
                    budget.as_secs_f64()
                )
            }
        }
    }
}

/// Where requests go and which model they ask for. Reading these does not
/// need the API key, so cached answers can be looked up without it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Endpoint {
    pub(super) url: String,
    pub(super) model: String,
}

impl Endpoint {
    pub(super) fn from_env() -> Result<Self, VerifyError> {
        let base_url = optional_env(BASE_URL_ENV, DEFAULT_BASE_URL)?;
        let model = optional_env(MODEL_ENV, DEFAULT_MODEL)?;
        Ok(Self {
            url: format!("{}/v1/systemone", base_url.trim_end_matches('/')),
            model,
        })
    }
}

pub(super) fn api_key_from_env() -> Result<String, VerifyError> {
    match env::var(API_KEY_ENV) {
        // A trailing newline from `$(cat key-file)` would otherwise make the
        // Authorization header invalid.
        Ok(value) if !value.trim().is_empty() => Ok(value.trim().to_owned()),
        Ok(_) | Err(env::VarError::NotPresent) => Err(VerifyError::MissingApiKey),
        Err(env::VarError::NotUnicode(_)) => Err(VerifyError::InvalidConfiguration(API_KEY_ENV)),
    }
}

fn optional_env(name: &'static str, fallback: &str) -> Result<String, VerifyError> {
    match env::var(name) {
        Ok(value) if !value.trim().is_empty() => Ok(value.trim().to_owned()),
        Ok(_) => Err(VerifyError::InvalidConfiguration(name)),
        Err(env::VarError::NotPresent) => Ok(fallback.to_owned()),
        Err(env::VarError::NotUnicode(_)) => Err(VerifyError::InvalidConfiguration(name)),
    }
}

/// Validated answers to one request: the resolved model and, per question
/// key, the raw `yes`/`no` probabilities. This is also what the cache stores.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Answers {
    pub(super) model: String,
    pub(super) probabilities: BTreeMap<String, BTreeMap<String, f64>>,
}

impl Answers {
    /// The probability that the question's target has a problem.
    pub(super) fn yes(&self, key: &str) -> f64 {
        self.probabilities[key]["yes"]
    }

    /// Check a response against the questions that were asked.
    pub(super) fn validate(response: Value, keys: &[&str]) -> Result<Self, VerifyError> {
        let response: ApiResponse = serde_json::from_value(response)
            .map_err(|error| VerifyError::Decode(error.to_string()))?;
        let mut probabilities = BTreeMap::new();
        let mut answers = response.answers;
        for key in keys {
            let answer = answers
                .remove(*key)
                .ok_or_else(|| VerifyError::InvalidResponse(format!("missing `{key}` answer")))?;
            if answer.answer_type != "choice" {
                return Err(VerifyError::InvalidResponse(format!(
                    "`{key}` answer had type {:?}, expected \"choice\"",
                    answer.answer_type
                )));
            }
            for option in ["yes", "no"] {
                let probability = answer.probabilities.get(option).ok_or_else(|| {
                    VerifyError::InvalidResponse(format!(
                        "`{key}` has no probability for `{option}`"
                    ))
                })?;
                if !(0.0..=1.0).contains(probability) {
                    return Err(VerifyError::InvalidResponse(format!(
                        "`{key}` probability for `{option}` must be between 0 and 1"
                    )));
                }
            }
            let sum = answer.probabilities.values().sum::<f64>();
            if (sum - 1.0).abs() > 0.001 {
                return Err(VerifyError::InvalidResponse(format!(
                    "`{key}` probabilities sum to {sum}, expected 1"
                )));
            }
            probabilities.insert((*key).to_owned(), answer.probabilities);
        }
        Ok(Self {
            model: response.model,
            probabilities,
        })
    }
}

#[derive(Debug, Deserialize)]
struct ApiResponse {
    model: String,
    answers: BTreeMap<String, ChoiceAnswer>,
}

#[derive(Debug, Deserialize)]
struct ChoiceAnswer {
    #[serde(rename = "type")]
    answer_type: String,
    probabilities: BTreeMap<String, f64>,
}

pub(super) struct Client {
    agent: Agent,
    url: String,
    authorization: String,
}

impl Client {
    pub(super) fn new(endpoint: &Endpoint, api_key: &str) -> Self {
        Self {
            agent: Agent::config_builder()
                .http_status_as_error(false)
                .build()
                .into(),
            url: endpoint.url.clone(),
            authorization: format!("Bearer {api_key}"),
        }
    }

    fn send(&self, body: &Value, timeout: Duration) -> Result<Value, VerifyError> {
        let mut response = self
            .agent
            .post(&self.url)
            .config()
            .timeout_global(Some(timeout))
            .build()
            .header("Authorization", &self.authorization)
            .send_json(body)
            .map_err(|error| VerifyError::Transport(error.to_string()))?;
        let status = response.status();
        if !status.is_success() {
            return Err(VerifyError::Api {
                status: status.as_u16(),
            });
        }
        response
            .body_mut()
            .read_json::<Value>()
            .map_err(|error| VerifyError::Decode(error.to_string()))
    }

    /// Send every body with at most `IN_FLIGHT` at once and return the raw
    /// responses in request order. The agent's connection pool is shared by
    /// the worker threads. A 401 or 403 stops the remaining requests, which
    /// get the same error instead of a doomed call. All requests share
    /// `RUN_BUDGET`: none starts after it ends, and a running one gets only the
    /// time left.
    pub(super) fn send_all(&self, bodies: &[Value]) -> Vec<Result<Value, VerifyError>> {
        self.send_all_within(bodies, RUN_BUDGET)
    }

    fn send_all_within(
        &self,
        bodies: &[Value],
        budget: Duration,
    ) -> Vec<Result<Value, VerifyError>> {
        let deadline = Instant::now() + budget;
        let next = AtomicUsize::new(0);
        let rejected = OnceLock::<VerifyError>::new();
        let results = bodies.iter().map(|_| OnceLock::new()).collect::<Vec<_>>();
        thread::scope(|scope| {
            for _ in 0..IN_FLIGHT.min(bodies.len()) {
                scope.spawn(|| {
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some(body) = bodies.get(index) else {
                            break;
                        };
                        let left = deadline.saturating_duration_since(Instant::now());
                        let result = match rejected.get() {
                            Some(error) => Err(error.clone()),
                            None if left.is_zero() => Err(VerifyError::BudgetExceeded(budget)),
                            None => match self.send(body, left.min(REQUEST_TIMEOUT)) {
                                // Cut short by the run's deadline, not by the request timeout.
                                Err(VerifyError::Transport(_)) if Instant::now() >= deadline => {
                                    Err(VerifyError::BudgetExceeded(budget))
                                }
                                result => result,
                            },
                        };
                        if let Err(error) = &result
                            && error.is_credential_rejection()
                        {
                            let _ = rejected.set(error.clone());
                        }
                        let _ = results[index].set(result);
                    }
                });
            }
        });
        results
            .into_iter()
            .map(|cell| cell.into_inner().expect("every request index was claimed"))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn answer(yes: f64, no: f64) -> Value {
        json!({"type": "choice", "choice": "no", "confidence": no, "probabilities": {"yes": yes, "no": no}})
    }

    #[test]
    fn validation_keeps_asked_questions_and_the_resolved_model() {
        let response = json!({
            "model": "jev-1.13.0",
            "answers": {"a": answer(0.9, 0.1), "b": answer(0.2, 0.8), "extra": answer(0.5, 0.5)},
            "usage": {"input_tokens": 10, "output_tokens": 2}
        });
        let answers = Answers::validate(response, &["a", "b"]).unwrap();
        assert_eq!(answers.model, "jev-1.13.0");
        assert_eq!(answers.yes("a"), 0.9);
        assert_eq!(answers.probabilities.len(), 2);
    }

    #[test]
    fn validation_rejects_missing_or_malformed_answers() {
        let cases = [
            json!({"model": "m", "answers": {}}),
            json!({"model": "m", "answers": {"a": {"type": "number", "probabilities": {"yes": 1.0, "no": 0.0}}}}),
            json!({"model": "m", "answers": {"a": {"type": "choice", "probabilities": {"yes": 1.0}}}}),
            json!({"model": "m", "answers": {"a": answer(1.5, -0.5)}}),
            json!({"model": "m", "answers": {"a": answer(0.5, 0.2)}}),
        ];
        for response in cases {
            assert!(
                matches!(
                    Answers::validate(response.clone(), &["a"]),
                    Err(VerifyError::InvalidResponse(_))
                ),
                "{response}"
            );
        }
        assert!(matches!(
            Answers::validate(json!({"answers": {}}), &["a"]),
            Err(VerifyError::Decode(_))
        ));
    }

    #[test]
    fn the_run_budget_stops_slow_requests_and_skips_unstarted_ones() {
        use std::io::Read;
        use std::net::TcpListener;

        // Accepts connections and never answers.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        thread::spawn(move || {
            let mut open = Vec::new();
            for mut stream in listener.incoming().flatten() {
                let mut buffer = [0; 1024];
                let _ = stream.read(&mut buffer);
                open.push(stream);
            }
        });
        let client = Client::new(
            &Endpoint {
                url,
                model: "m".into(),
            },
            "key",
        );
        let bodies = vec![json!({}); IN_FLIGHT + 2];
        let started = Instant::now();
        let budget = Duration::from_millis(300);
        let results = client.send_all_within(&bodies, budget);
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "{:?}",
            started.elapsed()
        );
        assert_eq!(results.len(), bodies.len());
        for result in results {
            assert_eq!(result, Err(VerifyError::BudgetExceeded(budget)));
        }
        assert_eq!(
            VerifyError::BudgetExceeded(RUN_BUDGET).to_string(),
            "time budget of 20 s exceeded"
        );
    }

    #[test]
    fn only_401_and_403_stop_the_remaining_requests() {
        assert!(VerifyError::Api { status: 401 }.is_credential_rejection());
        assert!(VerifyError::Api { status: 403 }.is_credential_rejection());
        assert!(!VerifyError::Api { status: 503 }.is_credential_rejection());
        assert!(!VerifyError::Transport("timeout".into()).is_credential_rejection());
    }
}
