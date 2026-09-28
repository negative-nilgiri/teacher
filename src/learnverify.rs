//! Optional, network-backed semantic lesson checks used only by the
//! `learnverify` binary.
//!
//! The module depends one way on the core library: it compiles the lesson
//! exactly like `learnc check` and reuses lint's source spans and diagnostic
//! shape. No compiler, runtime, source, artifact, repository, or lint module
//! depends on it, so an API failure can never affect `check`, `build`, `lint`,
//! or `learn`.

mod cache;
mod checks;
mod client;
mod config;
mod grade;

use std::path::Path;

use serde_json::Value;

use crate::compiler::CompileOptions;
use crate::diagnostics::Diagnostic;
use crate::lint;

use cache::Cache;
use checks::Job;
use client::{Answers, Client, Endpoint, VerifyError};

pub use config::VerifyConfig;
pub use grade::{VerifyDiagnostic, VerifyReport};

/// Findings for one lesson, plus operational warnings for stderr (such as a
/// disabled cache).
#[derive(Clone, Debug, PartialEq)]
pub struct Verification {
    pub findings: Vec<VerifyDiagnostic>,
    pub warnings: Vec<String>,
}

/// Compile `lesson_path` as `learnc check` does, ask TypeSafe about each quiz
/// and highlighted code block, and grade the answers. A lesson that fails to
/// compile is an error and makes no request; API trouble becomes a
/// `verify.unavailable` finding instead.
pub fn verify_file(
    lesson_path: &Path,
    options: &CompileOptions,
    config: &VerifyConfig,
    use_cache: bool,
) -> Result<Verification, Vec<Diagnostic>> {
    let lesson = lint::load_lesson(lesson_path, options, "learnverify")?;
    let jobs = checks::plan(&lesson, config.max_context_chars);
    let mut warnings = Vec::new();
    let outcomes = if jobs.is_empty() {
        Vec::new()
    } else {
        answer(&jobs, use_cache, &mut warnings)
    };
    Ok(Verification {
        findings: grade::findings(&lesson, &jobs, &outcomes, config),
        warnings,
    })
}

/// Answers for every job, in job order: cached where possible, otherwise
/// requested. The API key is only needed for cache misses.
fn answer(
    jobs: &[Job],
    use_cache: bool,
    warnings: &mut Vec<String>,
) -> Vec<Result<Answers, VerifyError>> {
    let endpoint = match Endpoint::from_env() {
        Ok(endpoint) => endpoint,
        Err(error) => return jobs.iter().map(|_| Err(error.clone())).collect(),
    };
    let cache = if use_cache {
        Cache::open()
            .map_err(|reason| warnings.push(format!("answer cache disabled: {reason}")))
            .ok()
    } else {
        None
    };
    let bodies = jobs
        .iter()
        .map(|job| job.request(&endpoint.model))
        .collect::<Vec<Value>>();
    let keys = bodies
        .iter()
        .map(|body| cache::key(&endpoint.url, body))
        .collect::<Vec<_>>();
    let mut outcomes = jobs
        .iter()
        .zip(&keys)
        .map(|(job, key)| {
            cache
                .as_ref()
                .and_then(|cache| cache.get(key))
                .filter(|answers| answers_every_question(answers, job))
                .map(Ok)
        })
        .collect::<Vec<_>>();

    let misses = (0..jobs.len())
        .filter(|index| outcomes[*index].is_none())
        .collect::<Vec<_>>();
    if !misses.is_empty() {
        match client::api_key_from_env() {
            Err(error) => {
                for index in &misses {
                    outcomes[*index] = Some(Err(error.clone()));
                }
            }
            Ok(api_key) => {
                let client = Client::new(&endpoint, &api_key);
                let miss_bodies = misses
                    .iter()
                    .map(|index| bodies[*index].clone())
                    .collect::<Vec<_>>();
                for (index, response) in misses.iter().zip(client.send_all(&miss_bodies)) {
                    let job = &jobs[*index];
                    let keys_asked = job
                        .questions
                        .iter()
                        .map(|question| question.key.as_str())
                        .collect::<Vec<_>>();
                    let result = response.and_then(|value| Answers::validate(value, &keys_asked));
                    if let (Ok(answers), Some(cache)) = (&result, &cache) {
                        cache.put(&keys[*index], answers);
                    }
                    outcomes[*index] = Some(result);
                }
            }
        }
    }
    outcomes
        .into_iter()
        .map(|outcome| outcome.expect("every job has a cached, requested, or failed outcome"))
        .collect()
}

/// A cache entry is only usable if it still answers every question.
fn answers_every_question(answers: &Answers, job: &Job) -> bool {
    job.questions.iter().all(|question| {
        answers
            .probabilities
            .get(&question.key)
            .is_some_and(|options| options.contains_key("yes"))
    })
}
