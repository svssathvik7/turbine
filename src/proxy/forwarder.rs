use crate::config::EndpointAuth;
use reqwest::Client;
use std::time::Duration;

pub struct Forwarder {
    client: Client,
}

impl Forwarder {
    pub fn new() -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to build HTTP client");

        Self { client }
    }

    pub async fn forward(
        &self,
        endpoint: &str,
        body: &[u8],
        auth: Option<&EndpointAuth>,
    ) -> Result<(u16, bytes::Bytes), ForwardError> {
        let mut req = self
            .client
            .post(endpoint)
            .header("Content-Type", "application/json")
            .body(body.to_vec());

        if let Some(auth) = auth {
            req = match auth {
                EndpointAuth::Basic { username, password } => {
                    req.basic_auth(username, Some(password))
                }
                EndpointAuth::Bearer(token) => req.bearer_auth(token),
                EndpointAuth::Header { name, value } => req.header(name, value),
            };
        }

        let response = req.send().await.map_err(|e| {
            if e.is_timeout() {
                ForwardError::Timeout
            } else if e.is_connect() {
                ForwardError::ConnectionFailed(e.to_string())
            } else {
                ForwardError::RequestFailed(e.to_string())
            }
        })?;

        let status = response.status().as_u16();

        if status == 429 {
            return Err(ForwardError::RateLimited);
        }

        if status >= 500 {
            return Err(ForwardError::ServerError(status));
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| ForwardError::RequestFailed(e.to_string()))?;

        Ok((status, bytes))
    }
}

#[derive(Debug)]
pub enum ForwardError {
    Timeout,
    ConnectionFailed(String),
    RateLimited,
    ServerError(u16),
    RequestFailed(String),
}

impl std::fmt::Display for ForwardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ForwardError::Timeout => write!(f, "Request timed out"),
            ForwardError::ConnectionFailed(e) => write!(f, "Connection failed: {}", e),
            ForwardError::RateLimited => write!(f, "Rate limited (429)"),
            ForwardError::ServerError(code) => write!(f, "Server error ({})", code),
            ForwardError::RequestFailed(e) => write!(f, "Request failed: {}", e),
        }
    }
}
