use std::time::Duration;

use goose::config::{Config, ConfigError};
use serde::{Deserialize, Serialize};
use url::{Host, Url};

pub const URL_KEY: &str = "KORY_SOURCE_URL";
pub const TOKEN_KEY: &str = "KORY_SOURCE_REVIEW_TOKEN";

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone)]
pub struct Settings {
    pub base_url: Url,
    pub token: String,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SettingsError {
    #[error("{missing} not set")]
    Missing { missing: String },
    #[error("{URL_KEY} is not a URL: {0}")]
    InvalidUrl(String),
    #[error("{URL_KEY} must use https, or http to a loopback host, so the review token is never sent in the clear: {0}")]
    InsecureUrl(String),
}

impl Settings {
    pub fn from_config(config: &Config) -> Result<Self, SettingsError> {
        let url = optional(config.get_param::<String>(URL_KEY));
        let token = optional(config.get_secret::<String>(TOKEN_KEY));
        match (url, token) {
            (Some(url), Some(token)) => Self::new(&url, token),
            (url, token) => {
                let missing = [(URL_KEY, url.is_none()), (TOKEN_KEY, token.is_none())]
                    .into_iter()
                    .filter_map(|(key, absent)| absent.then_some(key))
                    .collect::<Vec<_>>()
                    .join(" and ");
                Err(SettingsError::Missing { missing })
            }
        }
    }

    pub fn new(url: &str, token: String) -> Result<Self, SettingsError> {
        let base_url =
            Url::parse(url.trim()).map_err(|_| SettingsError::InvalidUrl(url.to_string()))?;
        let loopback = match base_url.host() {
            Some(Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
            Some(Host::Ipv4(ip)) => ip.is_loopback(),
            Some(Host::Ipv6(ip)) => ip.is_loopback(),
            None => return Err(SettingsError::InvalidUrl(url.to_string())),
        };
        match base_url.scheme() {
            "https" => {}
            "http" if loopback => {}
            _ => return Err(SettingsError::InsecureUrl(url.to_string())),
        }
        Ok(Self { base_url, token })
    }
}

fn optional(value: Result<String, ConfigError>) -> Option<String> {
    value.ok().filter(|value| !value.trim().is_empty())
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Job {
    pub id: String,
    pub state: String,
    pub stage: Option<String>,
    pub repo: String,
    pub pr_number: u64,
    pub requested_head_sha: Option<String>,
    pub reviewed_head_sha: Option<String>,
    pub base_sha: Option<String>,
    pub deadline_at: Option<f64>,
    pub deduplicated: bool,
    pub findings: Option<Vec<Finding>>,
    pub summary: Option<String>,
    pub coverage: Option<Coverage>,
    pub models: Models,
    pub error: Option<JobError>,
    pub pending_work: Vec<WorkItem>,
    pub report_url: Option<String>,
}

impl Job {
    pub fn is_terminal(&self) -> bool {
        matches!(self.state.as_str(), "succeeded" | "failed" | "interrupted")
    }
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Finding {
    pub id: String,
    pub path: Option<String>,
    pub line: Option<u64>,
    pub end_line: Option<u64>,
    pub severity: String,
    pub category: Option<String>,
    pub title: String,
    pub body: String,
    pub suggestion: Option<String>,
    pub registry_ids: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Coverage {
    pub changed_files: u64,
    pub reviewed_files: u64,
    pub skipped: Vec<SkippedFile>,
    pub note: String,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct SkippedFile {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Models {
    pub requested: Vec<String>,
    pub actual: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct JobError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct WorkItem {
    pub work_id: String,
    pub prompt: String,
    pub model: String,
    pub timeout_seconds: f64,
}

/// The two bodies `POST /api/reviews/{id}/work/{work_id}` accepts.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum WorkAnswer {
    Text {
        text: String,
        actual_model: String,
    },
    Error {
        error: WorkErrorCode,
        #[serde(skip_serializing_if = "Option::is_none")]
        actual_model: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkErrorCode {
    SeatExhausted,
    WorkstationOutage,
    ModelMismatch,
    Timeout,
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum ApiError {
    #[error("{0}")]
    Unreachable(String),
    #[error("HTTP {status}: {message}")]
    Http {
        status: u16,
        code: Option<String>,
        message: String,
        current_head_sha: Option<String>,
    },
}

impl ApiError {
    pub fn is_auth(&self) -> bool {
        matches!(
            self,
            ApiError::Http {
                status: 401 | 403,
                ..
            }
        )
    }

    /// Whether resending the same request later can succeed.
    pub fn is_transient(&self) -> bool {
        match self {
            ApiError::Unreachable(_) => true,
            ApiError::Http { status, .. } => matches!(status, 408 | 429 | 500..=599),
        }
    }
}

#[derive(Deserialize)]
struct ErrorEnvelope {
    error: ErrorBody,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ErrorBody {
    Plain(String),
    Coded {
        code: String,
        #[serde(default)]
        message: String,
        current_head_sha: Option<String>,
    },
}

#[derive(Serialize)]
struct SubmitBody<'a> {
    repo: &'a str,
    pr_number: u64,
    head_sha: &'a str,
}

pub struct Client {
    settings: Settings,
    http: reqwest::Client,
}

impl Client {
    pub fn new(settings: Settings) -> anyhow::Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()?;
        Ok(Self { settings, http })
    }

    pub fn base_url(&self) -> &Url {
        &self.settings.base_url
    }

    pub async fn submit(
        &self,
        repo: &str,
        pr_number: u64,
        head_sha: &str,
    ) -> Result<Job, ApiError> {
        let body = SubmitBody {
            repo,
            pr_number,
            head_sha,
        };
        self.send(self.http.post(self.url(&["api", "reviews"])), Some(&body))
            .await
    }

    pub async fn status(&self, job_id: &str) -> Result<Job, ApiError> {
        self.send::<(), _>(self.http.get(self.url(&["api", "reviews", job_id])), None)
            .await
    }

    pub async fn answer(
        &self,
        job_id: &str,
        work_id: &str,
        answer: &WorkAnswer,
    ) -> Result<(), ApiError> {
        let url = self.url(&["api", "reviews", job_id, "work", work_id]);
        self.send::<_, serde_json::Value>(self.http.post(url), Some(answer))
            .await
            .map(drop)
    }

    fn url(&self, segments: &[&str]) -> Url {
        let mut url = self.settings.base_url.clone();
        url.path_segments_mut()
            .expect("an http(s) URL has path segments")
            .pop_if_empty()
            .extend(segments);
        url
    }

    async fn send<B: Serialize, T: for<'de> Deserialize<'de>>(
        &self,
        request: reqwest::RequestBuilder,
        body: Option<&B>,
    ) -> Result<T, ApiError> {
        let mut request = request.bearer_auth(&self.settings.token);
        if let Some(body) = body {
            let json = serde_json::to_vec(body).expect("request bodies serialize");
            request = request
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(json);
        }
        let response = request
            .send()
            .await
            .map_err(|error| ApiError::Unreachable(error.without_url().to_string()))?;
        let status = response.status();
        let bytes = response
            .bytes()
            .await
            .map_err(|error| ApiError::Unreachable(error.without_url().to_string()))?;
        if status.is_success() {
            return serde_json::from_slice(&bytes).map_err(|error| ApiError::Http {
                status: status.as_u16(),
                code: None,
                message: format!("unreadable response: {error}"),
                current_head_sha: None,
            });
        }
        let (code, message, current_head_sha) =
            match serde_json::from_slice::<ErrorEnvelope>(&bytes).map(|e| e.error) {
                Ok(ErrorBody::Plain(message)) => (None, message, None),
                Ok(ErrorBody::Coded {
                    code,
                    message,
                    current_head_sha,
                }) => (Some(code), message, current_head_sha),
                Err(_) => (
                    None,
                    String::from_utf8_lossy(&bytes).chars().take(300).collect(),
                    None,
                ),
            };
        Err(ApiError::Http {
            status: status.as_u16(),
            code,
            message,
            current_head_sha,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_token_only_travels_over_https_or_to_loopback() {
        let token = || "kst_x".to_string();
        assert!(Settings::new("https://kory-source.example.com", token()).is_ok());
        assert!(Settings::new("http://127.0.0.1:8080", token()).is_ok());
        assert!(Settings::new("http://localhost:8080/", token()).is_ok());
        assert!(Settings::new("http://[::1]:8080", token()).is_ok());
        assert!(matches!(
            Settings::new("http://kory-source.example.com", token()),
            Err(SettingsError::InsecureUrl(_))
        ));
        assert!(matches!(
            Settings::new("kory-source", token()),
            Err(SettingsError::InvalidUrl(_))
        ));
    }

    #[test]
    fn work_answers_serialize_to_exactly_the_accepted_bodies() {
        let text = WorkAnswer::Text {
            text: "{}".into(),
            actual_model: "grok/grok-4.6".into(),
        };
        assert_eq!(
            serde_json::to_value(&text).unwrap(),
            serde_json::json!({"text": "{}", "actual_model": "grok/grok-4.6"})
        );
        let error = WorkAnswer::Error {
            error: WorkErrorCode::SeatExhausted,
            actual_model: None,
        };
        assert_eq!(
            serde_json::to_value(&error).unwrap(),
            serde_json::json!({"error": "seat_exhausted"})
        );
    }
}
