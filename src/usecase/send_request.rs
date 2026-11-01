use super::json_path;
use super::template::{self, TemplateContext};
use crate::entity::{ApiPreset, HttpMethod};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub url: String,
    pub method: HttpMethod,
    pub headers: HashMap<String, String>,
    pub body: Option<String>,
}

#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

/// Boundary the transport implementation sits behind, so `send_request` can
/// be unit-tested with a fake without any real networking (mirrors the
/// Kotlin `HttpClient` port). The real adapter (`ureq`-backed) lands in the
/// I/O layer.
pub trait HttpClient {
    fn execute(&self, request: HttpRequest) -> Result<HttpResponse, String>;
}

pub struct SendRequestInput<'a> {
    pub preset: &'a ApiPreset,
    pub selection: &'a str,
    pub filename: &'a str,
    pub tokens: &'a HashMap<String, String>,
}

pub enum SendOutput {
    Success {
        raw_body: String,
        extracted: String,
        status: u16,
    },
    Failure {
        message: String,
        status: Option<u16>,
        raw_body: Option<String>,
    },
}

pub fn send_request(http: &dyn HttpClient, input: SendRequestInput) -> SendOutput {
    let ctx = TemplateContext {
        selection: input.selection,
        filename: input.filename,
        tokens: input.tokens,
    };
    let url = template::render(&input.preset.url, &ctx);
    let body = if input.preset.body_template.is_empty() {
        None
    } else {
        Some(template::render(&input.preset.body_template, &ctx))
    };
    let headers: HashMap<String, String> = input
        .preset
        .headers
        .iter()
        .map(|(k, v)| (k.clone(), template::render(v, &ctx)))
        .collect();

    let request = HttpRequest {
        url,
        method: input.preset.method,
        headers,
        body,
    };
    let response = match http.execute(request) {
        Ok(r) => r,
        Err(e) => {
            return SendOutput::Failure {
                message: format!("HTTP error: {e}"),
                status: None,
                raw_body: None,
            }
        }
    };

    if !(200..300).contains(&response.status) {
        return SendOutput::Failure {
            message: format!("HTTP {}", response.status),
            status: Some(response.status),
            raw_body: Some(response.body),
        };
    }

    let extracted = match input.preset.response_json_path.as_deref() {
        None | Some("") => response.body.clone(),
        Some(p) if p.trim().is_empty() => response.body.clone(),
        Some(p) => match json_path::extract(&response.body, p) {
            Some(v) => v,
            None => {
                return SendOutput::Failure {
                    message: format!("JSON path '{p}' did not match"),
                    status: Some(response.status),
                    raw_body: Some(response.body),
                }
            }
        },
    };

    SendOutput::Success {
        raw_body: response.body,
        extracted,
        status: response.status,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::ResponseTarget;
    use std::cell::RefCell;

    struct FakeHttp {
        response: HttpResponse,
        captured: RefCell<Option<HttpRequest>>,
    }

    impl FakeHttp {
        fn new(status: u16, body: &str) -> Self {
            Self {
                response: HttpResponse {
                    status,
                    body: body.to_string(),
                },
                captured: RefCell::new(None),
            }
        }
    }

    impl HttpClient for FakeHttp {
        fn execute(&self, request: HttpRequest) -> Result<HttpResponse, String> {
            *self.captured.borrow_mut() = Some(request);
            Ok(self.response.clone())
        }
    }

    #[test]
    fn expands_template_calls_http_extracts_json_path() {
        let fake = FakeHttp::new(200, r#"{"choices":[{"message":{"content":"OK"}}]}"#);
        let mut headers = HashMap::new();
        headers.insert("Authorization".to_string(), "Bearer {{tokens.t}}".to_string());
        let preset = ApiPreset {
            name: "echo".to_string(),
            url: "https://example.com/api".to_string(),
            method: HttpMethod::Post,
            headers,
            body_template: r#"{"q":"{{selectionJson}}"}"#.to_string(),
            response_json_path: Some("choices.0.message.content".to_string()),
            response_target: ResponseTarget::NewTab,
        };
        let mut tokens = HashMap::new();
        tokens.insert("t".to_string(), "secret".to_string());

        let out = send_request(
            &fake,
            SendRequestInput {
                preset: &preset,
                selection: "hi\nthere",
                filename: "x.txt",
                tokens: &tokens,
            },
        );

        match out {
            SendOutput::Success { extracted, .. } => assert_eq!(extracted, "OK"),
            SendOutput::Failure { message, .. } => panic!("expected success, got failure: {message}"),
        }
        let captured = fake.captured.borrow();
        let captured = captured.as_ref().unwrap();
        assert_eq!(captured.headers.get("Authorization").unwrap(), "Bearer secret");
        assert_eq!(captured.body.as_deref(), Some(r#"{"q":"hi\nthere"}"#));
    }

    #[test]
    fn returns_failure_on_non_2xx_status() {
        let fake = FakeHttp::new(500, "boom");
        let preset = ApiPreset {
            name: "t".to_string(),
            url: "https://example/".to_string(),
            ..Default::default()
        };
        let tokens = HashMap::new();
        let out = send_request(
            &fake,
            SendRequestInput {
                preset: &preset,
                selection: "",
                filename: "",
                tokens: &tokens,
            },
        );
        match out {
            SendOutput::Failure { status, .. } => assert_eq!(status, Some(500)),
            SendOutput::Success { .. } => panic!("expected failure"),
        }
    }

    #[test]
    fn returns_failure_when_json_path_misses() {
        let fake = FakeHttp::new(200, r#"{"a":1}"#);
        let preset = ApiPreset {
            name: "t".to_string(),
            url: "https://example/".to_string(),
            response_json_path: Some("b.c".to_string()),
            ..Default::default()
        };
        let tokens = HashMap::new();
        let out = send_request(
            &fake,
            SendRequestInput {
                preset: &preset,
                selection: "",
                filename: "",
                tokens: &tokens,
            },
        );
        match out {
            SendOutput::Failure { message, .. } => assert!(message.contains("JSON path")),
            SendOutput::Success { .. } => panic!("expected failure"),
        }
    }

    #[test]
    fn no_json_path_returns_raw_body() {
        let fake = FakeHttp::new(200, "plain text");
        let preset = ApiPreset {
            name: "t".to_string(),
            url: "https://example/".to_string(),
            ..Default::default()
        };
        let tokens = HashMap::new();
        let out = send_request(
            &fake,
            SendRequestInput {
                preset: &preset,
                selection: "",
                filename: "",
                tokens: &tokens,
            },
        );
        match out {
            SendOutput::Success { extracted, .. } => assert_eq!(extracted, "plain text"),
            SendOutput::Failure { message, .. } => panic!("expected success, got failure: {message}"),
        }
    }
}
