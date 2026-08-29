use crate::auth::AuthProvider;
use crate::common::ResponseStream;
use crate::common::ResponsesApiRequest;
use crate::endpoint::session::EndpointSession;
use crate::error::ApiError;
use crate::provider::Provider;
use crate::requests::responses::merge_extra_body;
use crate::sse::spawn_anthropic_response_stream;
use crate::telemetry::SseTelemetry;
use codex_client::HttpTransport;
use http::HeaderMap;
use http::HeaderValue;
use http::Method;
use serde_json::Value;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::instrument;

pub struct AnthropicMessagesClient<T: HttpTransport, A: AuthProvider> {
    session: EndpointSession<T, A>,
    sse_telemetry: Option<Arc<dyn SseTelemetry>>,
}

impl<T: HttpTransport, A: AuthProvider> AnthropicMessagesClient<T, A> {
    pub fn new(transport: T, provider: Provider, auth: A) -> Self {
        Self {
            session: EndpointSession::new(transport, provider, auth),
            sse_telemetry: None,
        }
    }

    pub fn with_telemetry(
        self,
        request: Option<Arc<dyn codex_client::RequestTelemetry>>,
        sse: Option<Arc<dyn SseTelemetry>>,
    ) -> Self {
        Self {
            session: self.session.with_request_telemetry(request),
            sse_telemetry: sse,
        }
    }

    #[instrument(
        name = "anthropic_messages.stream_request",
        level = "info",
        skip_all,
        fields(
            transport = "anthropic_messages_http",
            http.method = "POST",
            api.path = "v1/messages"
        )
    )]
    pub async fn stream_request(
        &self,
        request: ResponsesApiRequest,
        extra_headers: HeaderMap,
    ) -> Result<ResponseStream, ApiError> {
        let body = build_anthropic_body(&request)?;

        let stream_response = self
            .session
            .stream_with(
                Method::POST,
                "v1/messages",
                extra_headers,
                Some(body),
                |req| {
                    req.headers.insert(
                        http::header::ACCEPT,
                        HeaderValue::from_static("text/event-stream"),
                    );
                },
            )
            .await?;

        Ok(spawn_anthropic_response_stream(
            stream_response,
            self.session.provider().stream_idle_timeout,
            self.sse_telemetry.clone(),
            tool_kind_map(&request.tools),
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AnthropicToolKind {
    FunctionLike,
    Custom,
}

fn build_anthropic_body(request: &ResponsesApiRequest) -> Result<Value, ApiError> {
    let (system, messages) = translate_input(request);
    let tools = translate_tools(&request.tools);

    let mut body = json!({
        "model": request.model,
        "messages": messages,
        "max_tokens": 8192,
        "stream": true,
    });

    if !system.is_empty() {
        body["system"] = Value::String(system);
    }
    if !tools.is_empty() {
        body["tools"] = Value::Array(tools);
    }

    if let Some(extra_body) = request.extra_body.as_ref() {
        merge_extra_body(&mut body, extra_body).map_err(ApiError::Stream)?;
    }

    Ok(body)
}

fn translate_input(request: &ResponsesApiRequest) -> (String, Vec<Value>) {
    let mut system_parts = Vec::new();
    if !request.instructions.trim().is_empty() {
        system_parts.push(request.instructions.trim().to_string());
    }

    let mut messages = Vec::<Value>::new();

    for item in &request.input {
        match item {
            codex_protocol::models::ResponseItem::Message { role, content, .. } => {
                if role == "system" || role == "developer" {
                    let text = content_to_plain_text(content);
                    if !text.trim().is_empty() {
                        system_parts.push(text);
                    }
                    continue;
                }

                let blocks = content
                    .iter()
                    .map(|entry| match entry {
                        codex_protocol::models::ContentItem::InputText { text }
                        | codex_protocol::models::ContentItem::OutputText { text } => {
                            json!({"type": "text", "text": text})
                        }
                        codex_protocol::models::ContentItem::InputImage { image_url } => {
                            json!({"type": "text", "text": format!("[image omitted: {image_url}]")})
                        }
                    })
                    .collect::<Vec<_>>();
                push_message(&mut messages, role, blocks);
            }
            codex_protocol::models::ResponseItem::FunctionCall {
                name,
                arguments,
                call_id,
                ..
            } => {
                let input = serde_json::from_str::<Value>(arguments)
                    .unwrap_or_else(|_| json!({ "input": arguments }));
                push_message(
                    &mut messages,
                    "assistant",
                    vec![json!({
                        "type": "tool_use",
                        "id": call_id,
                        "name": name,
                        "input": input,
                    })],
                );
            }
            codex_protocol::models::ResponseItem::CustomToolCall {
                name,
                input,
                call_id,
                ..
            } => {
                push_message(
                    &mut messages,
                    "assistant",
                    vec![json!({
                        "type": "tool_use",
                        "id": call_id,
                        "name": name,
                        "input": { "input": input },
                    })],
                );
            }
            codex_protocol::models::ResponseItem::LocalShellCall {
                id,
                call_id,
                action,
                ..
            } => {
                let call_id = call_id
                    .clone()
                    .or_else(|| id.clone())
                    .unwrap_or_else(|| "local_shell".to_string());
                let input = match action {
                    codex_protocol::models::LocalShellAction::Exec(exec) => json!({
                        "command": exec.command,
                        "workdir": exec.working_directory,
                        "timeout_ms": exec.timeout_ms,
                    }),
                };
                push_message(
                    &mut messages,
                    "assistant",
                    vec![json!({
                        "type": "tool_use",
                        "id": call_id,
                        "name": "local_shell",
                        "input": input,
                    })],
                );
            }
            codex_protocol::models::ResponseItem::ToolSearchCall {
                call_id, arguments, ..
            } => {
                push_message(
                    &mut messages,
                    "assistant",
                    vec![json!({
                        "type": "tool_use",
                        "id": call_id.clone().unwrap_or_else(|| "tool_search".to_string()),
                        "name": "tool_search",
                        "input": arguments,
                    })],
                );
            }
            codex_protocol::models::ResponseItem::FunctionCallOutput { call_id, output } => {
                push_message(
                    &mut messages,
                    "user",
                    vec![json!({
                        "type": "tool_result",
                        "tool_use_id": call_id,
                        "content": output.text_content().unwrap_or_default(),
                    })],
                );
            }
            codex_protocol::models::ResponseItem::CustomToolCallOutput {
                call_id, output, ..
            } => {
                push_message(
                    &mut messages,
                    "user",
                    vec![json!({
                        "type": "tool_result",
                        "tool_use_id": call_id,
                        "content": output.text_content().unwrap_or_default(),
                    })],
                );
            }
            codex_protocol::models::ResponseItem::Reasoning { .. }
            | codex_protocol::models::ResponseItem::ToolSearchOutput { .. }
            | codex_protocol::models::ResponseItem::WebSearchCall { .. }
            | codex_protocol::models::ResponseItem::ImageGenerationCall { .. }
            | codex_protocol::models::ResponseItem::GhostSnapshot { .. }
            | codex_protocol::models::ResponseItem::Compaction { .. }
            | codex_protocol::models::ResponseItem::Other => {}
        }
    }

    (system_parts.join("\n\n"), messages)
}

fn content_to_plain_text(content: &[codex_protocol::models::ContentItem]) -> String {
    content
        .iter()
        .map(|entry| match entry {
            codex_protocol::models::ContentItem::InputText { text }
            | codex_protocol::models::ContentItem::OutputText { text } => text.clone(),
            codex_protocol::models::ContentItem::InputImage { image_url } => {
                format!("[image omitted: {image_url}]")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn push_message(messages: &mut Vec<Value>, role: &str, mut blocks: Vec<Value>) {
    if blocks.is_empty() {
        return;
    }

    if let Some(last) = messages.last_mut()
        && last.get("role").and_then(Value::as_str) == Some(role)
        && let Some(content) = last.get_mut("content").and_then(Value::as_array_mut)
    {
        content.append(&mut blocks);
        return;
    }

    messages.push(json!({
        "role": role,
        "content": blocks,
    }));
}

pub(crate) fn tool_kind_map(tools: &[Value]) -> HashMap<String, AnthropicToolKind> {
    let mut map = HashMap::new();

    for tool in tools {
        let Some(tool_type) = tool.get("type").and_then(Value::as_str) else {
            continue;
        };

        let name = match tool_type {
            "function" | "custom" => tool.get("name").and_then(Value::as_str),
            "local_shell" => Some("local_shell"),
            "tool_search" => Some("tool_search"),
            "image_generation" => Some("image_generation"),
            "web_search" => Some("web_search"),
            _ => None,
        };

        let Some(name) = name else {
            continue;
        };
        let kind = if tool_type == "custom" {
            AnthropicToolKind::Custom
        } else {
            AnthropicToolKind::FunctionLike
        };
        map.insert(name.to_string(), kind);
    }

    map
}

fn translate_tools(tools: &[Value]) -> Vec<Value> {
    tools.iter().filter_map(translate_tool).collect()
}

fn translate_tool(tool: &Value) -> Option<Value> {
    let tool_type = tool.get("type")?.as_str()?;
    match tool_type {
        "function" => Some(json!({
            "name": tool.get("name")?,
            "description": tool.get("description").cloned().unwrap_or_else(|| Value::String(String::new())),
            "input_schema": tool.get("parameters").cloned().unwrap_or_else(|| json!({"type": "object"})),
        })),
        "custom" => Some(json!({
            "name": tool.get("name")?,
            "description": tool.get("description").cloned().unwrap_or_else(|| Value::String(String::new())),
            "input_schema": {
                "type": "object",
                "properties": {
                    "input": {
                        "type": "string",
                        "description": tool.pointer("/format/definition").cloned().unwrap_or_else(|| Value::String("Freeform tool input.".to_string())),
                    }
                },
                "required": ["input"],
                "additionalProperties": false,
            },
        })),
        "tool_search" => Some(json!({
            "name": "tool_search",
            "description": tool.get("description").cloned().unwrap_or_else(|| Value::String("Search installed tools.".to_string())),
            "input_schema": tool.get("parameters").cloned().unwrap_or_else(|| json!({"type": "object"})),
        })),
        "local_shell" => Some(json!({
            "name": "local_shell",
            "description": "Run a shell command in the local workspace.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "command": { "type": "array", "items": { "type": "string" } },
                    "workdir": { "type": ["string", "null"] },
                    "timeout_ms": { "type": ["integer", "null"] }
                },
                "required": ["command"],
                "additionalProperties": true,
            },
        })),
        "image_generation" => Some(json!({
            "name": "image_generation",
            "description": "Generate or edit an image.",
            "input_schema": { "type": "object", "additionalProperties": true },
        })),
        "web_search" => Some(json!({
            "name": "web_search",
            "description": "Search the web and inspect web pages.",
            "input_schema": { "type": "object", "additionalProperties": true },
        })),
        _ => None,
    }
}

pub(crate) fn serialize_tool_input(input: &Value) -> String {
    match input {
        Value::Object(_) | Value::Array(_) => {
            serde_json::to_string(input).unwrap_or_else(|_| "{}".to_string())
        }
        Value::String(text) => json!({ "input": text }).to_string(),
        Value::Null => "{}".to_string(),
        _ => json!({ "input": input }).to_string(),
    }
}
