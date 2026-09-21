use std::env;
use std::time::Duration;

use serde_json::Value;
use ureq::Agent;

use super::{ApiResponse, PickError};

const API_KEY_ENV: &str = "TYPESAFE_API_KEY";
const BASE_URL_ENV: &str = "TYPESAFE_BASE_URL";
const MODEL_ENV: &str = "TYPESAFE_DEFAULT_MODEL";
const DEFAULT_BASE_URL: &str = "https://api.typesafe.ai";
const DEFAULT_MODEL: &str = "jev-latest";
const TIMEOUT: Duration = Duration::from_secs(10);

pub(super) struct Config {
    api_key: String,
    endpoint: String,
    pub(super) model: String,
}

impl Config {
    pub(super) fn from_env() -> Result<Self, PickError> {
        let api_key = required_env(API_KEY_ENV, PickError::MissingApiKey)?;
        let base_url = optional_env(BASE_URL_ENV, DEFAULT_BASE_URL)?;
        let model = optional_env(MODEL_ENV, DEFAULT_MODEL)?;
        Ok(Self {
            api_key,
            endpoint: format!("{}/v1/systemone", base_url.trim_end_matches('/')),
            model,
        })
    }
}

fn required_env(name: &'static str, missing: PickError) -> Result<String, PickError> {
    match env::var(name) {
        Ok(value) if !value.trim().is_empty() => Ok(value),
        Ok(_) | Err(env::VarError::NotPresent) => Err(missing),
        Err(env::VarError::NotUnicode(_)) => Err(PickError::InvalidConfiguration(name)),
    }
}

fn optional_env(name: &'static str, fallback: &str) -> Result<String, PickError> {
    match env::var(name) {
        Ok(value) if !value.trim().is_empty() => Ok(value),
        Ok(_) => Err(PickError::InvalidConfiguration(name)),
        Err(env::VarError::NotPresent) => Ok(fallback.to_owned()),
        Err(env::VarError::NotUnicode(_)) => Err(PickError::InvalidConfiguration(name)),
    }
}

pub(super) fn send(config: &Config, request: &Value) -> Result<ApiResponse, PickError> {
    let agent: Agent = Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .http_status_as_error(false)
        .build()
        .into();
    let authorization = format!("Bearer {}", config.api_key);
    let mut response = agent
        .post(&config.endpoint)
        .header("Authorization", &authorization)
        .send_json(request)
        .map_err(|error| PickError::Transport(error.to_string()))?;
    let status = response.status();
    if !status.is_success() {
        return Err(PickError::Api {
            status: status.as_u16(),
        });
    }
    response
        .body_mut()
        .read_json::<ApiResponse>()
        .map_err(|error| PickError::Decode(error.to_string()))
}
