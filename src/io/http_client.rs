use crate::entity::HttpMethod;
use crate::usecase::send_request::{HttpClient, HttpRequest, HttpResponse};
use std::time::Duration;
use ureq::Agent;

/// Blocking HTTP client backed by `ureq`, matching the synchronous model of
/// the original `JdkHttpClient` (`java.net.http.HttpClient` used
/// synchronously). `http_status_as_error(false)` is required so a 4xx/5xx
/// response comes back as `Ok` with the real status/body instead of an
/// `Err`, since `send_request` needs to inspect the status itself.
pub struct UreqHttpClient {
    agent: Agent,
}

impl UreqHttpClient {
    pub fn new(timeout: Duration) -> Self {
        let config = Agent::config_builder()
            .timeout_global(Some(timeout))
            .http_status_as_error(false)
            .build();
        Self { agent: config.into() }
    }
}

impl Default for UreqHttpClient {
    fn default() -> Self {
        Self::new(Duration::from_secs(60))
    }
}

impl HttpClient for UreqHttpClient {
    fn execute(&self, request: HttpRequest) -> Result<HttpResponse, String> {
        let url = request.url.as_str();
        let result = match request.method {
            HttpMethod::Get => {
                let mut builder = self.agent.get(url);
                for (k, v) in &request.headers {
                    builder = builder.header(k.as_str(), v.as_str());
                }
                builder.call()
            }
            HttpMethod::Delete => {
                let mut builder = self.agent.delete(url);
                for (k, v) in &request.headers {
                    builder = builder.header(k.as_str(), v.as_str());
                }
                builder.call()
            }
            HttpMethod::Post | HttpMethod::Put | HttpMethod::Patch => {
                let mut builder = match request.method {
                    HttpMethod::Post => self.agent.post(url),
                    HttpMethod::Put => self.agent.put(url),
                    HttpMethod::Patch => self.agent.patch(url),
                    _ => unreachable!(),
                };
                for (k, v) in &request.headers {
                    builder = builder.header(k.as_str(), v.as_str());
                }
                match &request.body {
                    Some(body) => builder.send(body.as_str()),
                    None => builder.send_empty(),
                }
            }
        };

        let mut response = result.map_err(|e| e.to_string())?;
        let status = response.status().as_u16();
        let body = response
            .body_mut()
            .read_to_string()
            .map_err(|e| e.to_string())?;
        Ok(HttpResponse { status, body })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    // Hits the real network, so it's excluded from the default `cargo test`
    // run. Exists purely to catch agent/TLS misconfiguration early, before
    // it's only discoverable once wired into the Send UI (M7).
    #[test]
    #[ignore]
    fn get_against_a_real_endpoint_returns_200() {
        let client = UreqHttpClient::default();
        let response = client
            .execute(HttpRequest {
                url: "https://httpbin.org/get".to_string(),
                method: HttpMethod::Get,
                headers: HashMap::new(),
                body: None,
            })
            .unwrap();
        assert_eq!(response.status, 200);
        assert!(response.body.contains("httpbin"));
    }
}
