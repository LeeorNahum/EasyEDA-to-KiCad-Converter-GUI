//! The EasyEDA web API: a part's CAD data and its 3D model files.
//!
//! The endpoints and request headers are the ones easyeda2kicad 1.0.1 uses.
//! EasyEDA's CDN refuses requests without a browser-like User-Agent.

use std::io::Read;
use std::time::Duration;

use reqwest::StatusCode;
use serde_json::Value;

const COMPONENT_URL: &str = "https://easyeda.com/api/products/{id}/components";
const OBJ_URL: &str = "https://modules.easyeda.com/3dmodel/{uuid}";
const STEP_URL: &str = "https://modules.easyeda.com/qAxj6KHrDKw4blvCG8QJPs7Y/{uuid}";
const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

/// Why a request to EasyEDA did not return what was asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchError {
    /// No connection, a DNS failure, or a timeout.
    Unreachable(String),
    /// EasyEDA answered 403 or 429, which it does when it rate-limits.
    Refused(u16),
    /// EasyEDA has no part with this number.
    PartNotFound,
    /// Any other HTTP failure status.
    Status(u16),
    /// A response that is not the JSON or text expected.
    BadResponse(String),
}

pub struct Client {
    http: reqwest::Client,
}

impl Client {
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(30))
            .build()
            .expect("the HTTP client configuration is static and valid");
        Self { http }
    }

    /// The `result` object of the component endpoint: the part's symbol,
    /// footprint, and metadata.
    pub async fn component(&self, lcsc_id: &str) -> Result<Value, FetchError> {
        let url = COMPONENT_URL.replace("{id}", lcsc_id);
        let request = self
            .http
            .get(url)
            .header("Accept", "application/json, text/javascript, */*; q=0.01")
            .header("Content-Type", "application/x-www-form-urlencoded; charset=UTF-8")
            .header("Referer", "https://easyeda.com/");
        let body = match self.send(request).await? {
            Some(body) => body,
            // The endpoint answers an unknown number with a JSON 404 body, and
            // a bare 404 status means the same.
            None => return Err(FetchError::PartNotFound),
        };
        let text = decode_text(&body)?;
        let response: Value = serde_json::from_str(&text)
            .map_err(|error| FetchError::BadResponse(format!("invalid JSON: {error}")))?;
        if response.get("success") == Some(&Value::Bool(false)) {
            if response.get("code").and_then(Value::as_i64) == Some(404) {
                return Err(FetchError::PartNotFound);
            }
            let message = response.get("message").and_then(Value::as_str).unwrap_or("no message");
            return Err(FetchError::BadResponse(format!("EasyEDA reported: {message}")));
        }
        match response.get("result") {
            Some(result) if result.is_object() => Ok(result.clone()),
            _ => Err(FetchError::BadResponse("the response has no part data".to_string())),
        }
    }

    /// The OBJ model text, or `None` when EasyEDA has no model under this id.
    pub async fn obj_model(&self, uuid: &str) -> Result<Option<String>, FetchError> {
        let request = self.http.get(OBJ_URL.replace("{uuid}", uuid));
        match self.send(request).await? {
            Some(body) => decode_text(&body).map(Some),
            None => Ok(None),
        }
    }

    /// The STEP model, or `None` when EasyEDA has no STEP file under this id.
    pub async fn step_model(&self, uuid: &str) -> Result<Option<Vec<u8>>, FetchError> {
        let request = self.http.get(STEP_URL.replace("{uuid}", uuid));
        self.send(request).await
    }

    /// Sends a request and returns its body, or `None` on a 404.
    async fn send(&self, request: reqwest::RequestBuilder) -> Result<Option<Vec<u8>>, FetchError> {
        let response = request.send().await.map_err(unreachable)?;
        let status = response.status();
        if status == StatusCode::NOT_FOUND {
            // The component endpoint's 404 carries a JSON body worth reading.
            let body = response.bytes().await.map_err(unreachable)?;
            if let Ok(text) = decode_text(&body)
                && serde_json::from_str::<Value>(&text).is_ok()
            {
                return Ok(Some(body.to_vec()));
            }
            return Ok(None);
        }
        if status == StatusCode::FORBIDDEN || status == StatusCode::TOO_MANY_REQUESTS {
            return Err(FetchError::Refused(status.as_u16()));
        }
        if !status.is_success() {
            return Err(FetchError::Status(status.as_u16()));
        }
        let body = response.bytes().await.map_err(unreachable)?;
        Ok(Some(body.to_vec()))
    }
}

impl Default for Client {
    fn default() -> Self {
        Self::new()
    }
}

fn unreachable(error: reqwest::Error) -> FetchError {
    let mut message = error.to_string();
    let mut source = std::error::Error::source(&error);
    while let Some(inner) = source {
        message.push_str(": ");
        message.push_str(&inner.to_string());
        source = inner.source();
    }
    FetchError::Unreachable(message)
}

/// A response body as text. A gzip body sent without a gzip header is
/// unpacked first, as easyeda2kicad does.
fn decode_text(body: &[u8]) -> Result<String, FetchError> {
    if body.starts_with(&[0x1f, 0x8b]) {
        let mut text = String::new();
        flate2::read::GzDecoder::new(body)
            .read_to_string(&mut text)
            .map_err(|error| FetchError::BadResponse(format!("gzip body could not be read: {error}")))?;
        return Ok(text);
    }
    Ok(String::from_utf8_lossy(body).into_owned())
}
