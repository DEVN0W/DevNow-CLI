use crate::common::ResponseEvent;
use crate::common::ResponseStream;
use crate::endpoint::anthropic::AnthropicToolKind;
use crate::endpoint::anthropic::serialize_tool_input;
use crate::error::ApiError;
use crate::telemetry::SseTelemetry;
use codex_client::StreamResponse;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ReasoningItemContent;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::TokenUsage;
use eventsource_stream::Eventsource;
use futures::StreamExt;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::Instant;
use tokio::time::timeout;
use tracing::debug;
use tracing::trace;

#[derive(Debug, Deserialize)]
struct AnthropicStreamEvent {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    message: Option<AnthropicMessageEnvelope>,
    #[serde(default)]
    content_block: Option<AnthropicContentBlock>,
    #[serde(default)]
    delta: Option<AnthropicDelta>,
    #[serde(default)]
    index: Option<i64>,
    #[serde(default)]
    usage: Option<AnthropicUsage>,
    #[serde(default)]
    error: Option<AnthropicError>,
}

#[derive(Debug, Deserialize)]
struct AnthropicMessageEnvelope {
    id: Option<String>,
    usage: Option<AnthropicUsage>,
}

#[derive(Debug, Deserialize, Default, Clone)]
struct AnthropicUsage {
    #[serde(default)]
    input_tokens: Option<i64>,
    #[serde(default)]
    output_tokens: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct AnthropicError {
    #[serde(default)]
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AnthropicContentBlock {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    input: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct AnthropicDelta {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    thinking: Option<String>,
    #[serde(default)]
    partial_json: Option<String>,
}

enum BlockState {
    Text(String),
    Thinking(String),
    ToolUse {
        id: String,
        name: String,
        input_json: String,
    },
}

struct AnthropicState {
    response_id: Option<String>,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    blocks: HashMap<i64, BlockState>,
}

impl AnthropicState {
    fn new() -> Self {
        Self {
            response_id: None,
            input_tokens: None,
            output_tokens: None,
            blocks: HashMap::new(),
        }
    }

    fn token_usage(&self) -> Option<TokenUsage> {
        let input_tokens = self.input_tokens?;
        let output_tokens = self.output_tokens.unwrap_or(0);
        Some(TokenUsage {
            input_tokens,
            cached_input_tokens: 0,
            output_tokens,
            reasoning_output_tokens: 0,
            total_tokens: input_tokens + output_tokens,
        })
    }
}

pub(crate) fn spawn_anthropic_response_stream(
    stream_response: StreamResponse,
    idle_timeout: Duration,
    telemetry: Option<Arc<dyn SseTelemetry>>,
    tool_kinds: HashMap<String, AnthropicToolKind>,
) -> ResponseStream {
    let (tx_event, rx_event) = mpsc::channel::<Result<ResponseEvent, ApiError>>(1600);
    tokio::spawn(process_anthropic_sse(
        stream_response.bytes,
        tx_event,
        idle_timeout,
        telemetry,
        tool_kinds,
    ));
    ResponseStream { rx_event }
}

async fn process_anthropic_sse(
    stream: codex_client::ByteStream,
    tx_event: mpsc::Sender<Result<ResponseEvent, ApiError>>,
    idle_timeout: Duration,
    telemetry: Option<Arc<dyn SseTelemetry>>,
    tool_kinds: HashMap<String, AnthropicToolKind>,
) {
    let mut stream = stream.eventsource();
    let mut state = AnthropicState::new();

    loop {
        let start = Instant::now();
        let response = timeout(idle_timeout, stream.next()).await;
        if let Some(t) = telemetry.as_ref() {
            t.on_sse_poll(&response, start.elapsed());
        }

        let sse = match response {
            Ok(Some(Ok(sse))) => sse,
            Ok(Some(Err(e))) => {
                let _ = tx_event.send(Err(ApiError::Stream(e.to_string()))).await;
                return;
            }
            Ok(None) => {
                let _ = tx_event
                    .send(Err(ApiError::Stream(
                        "stream closed before message_stop".into(),
                    )))
                    .await;
                return;
            }
            Err(_) => {
                let _ = tx_event
                    .send(Err(ApiError::Stream("idle timeout waiting for SSE".into())))
                    .await;
                return;
            }
        };

        trace!("Anthropic SSE event {}: {}", sse.event, sse.data);

        let event: AnthropicStreamEvent = match serde_json::from_str(&sse.data) {
            Ok(event) => event,
            Err(err) => {
                debug!(
                    "Failed to parse Anthropic SSE event: {err}, data: {}",
                    sse.data
                );
                continue;
            }
        };

        if handle_event(&mut state, &tool_kinds, event, &tx_event)
            .await
            .is_err()
        {
            return;
        }
    }
}

async fn handle_event(
    state: &mut AnthropicState,
    tool_kinds: &HashMap<String, AnthropicToolKind>,
    event: AnthropicStreamEvent,
    tx_event: &mpsc::Sender<Result<ResponseEvent, ApiError>>,
) -> Result<(), ()> {
    match event.kind.as_str() {
        "message_start" => {
            if let Some(message) = event.message {
                state.response_id = message.id;
                if let Some(usage) = message.usage {
                    state.input_tokens = usage.input_tokens;
                    state.output_tokens = usage.output_tokens;
                }
            }
            tx_event
                .send(Ok(ResponseEvent::Created))
                .await
                .map_err(|_| ())?;
        }
        "content_block_start" => {
            let index = event.index.unwrap_or(0);
            if let Some(block) = event.content_block {
                let state_block = match block.kind.as_str() {
                    "text" => Some(BlockState::Text(String::new())),
                    "thinking" => Some(BlockState::Thinking(String::new())),
                    "tool_use" => Some(BlockState::ToolUse {
                        id: block.id.unwrap_or_else(|| format!("tool_{index}")),
                        name: block.name.unwrap_or_else(|| "tool".to_string()),
                        input_json: block
                            .input
                            .map(|input| serialize_tool_input(&input))
                            .unwrap_or_else(|| "{}".to_string()),
                    }),
                    _ => None,
                };
                if let Some(block_state) = state_block {
                    if matches!(block_state, BlockState::Text(_) | BlockState::Thinking(_)) {
                        let item = match &block_state {
                            BlockState::Text(_) => ResponseItem::Message {
                                id: None,
                                role: "assistant".to_string(),
                                content: vec![ContentItem::OutputText {
                                    text: String::new(),
                                }],
                                end_turn: None,
                                phase: None,
                            },
                            BlockState::Thinking(_) => ResponseItem::Reasoning {
                                id: String::new(),
                                summary: Vec::new(),
                                content: Some(vec![ReasoningItemContent::ReasoningText {
                                    text: String::new(),
                                }]),
                                encrypted_content: None,
                            },
                            BlockState::ToolUse { .. } => unreachable!(),
                        };
                        tx_event
                            .send(Ok(ResponseEvent::OutputItemAdded(item)))
                            .await
                            .map_err(|_| ())?;
                    }
                    state.blocks.insert(index, block_state);
                }
            }
        }
        "content_block_delta" => {
            let index = event.index.unwrap_or(0);
            if let Some(delta) = event.delta {
                match (state.blocks.get_mut(&index), delta.kind.as_str()) {
                    (Some(BlockState::Text(text)), "text_delta") => {
                        let piece = delta.text.unwrap_or_default();
                        text.push_str(&piece);
                        tx_event
                            .send(Ok(ResponseEvent::OutputTextDelta(piece)))
                            .await
                            .map_err(|_| ())?;
                    }
                    (Some(BlockState::Thinking(text)), "thinking_delta") => {
                        let piece = delta.thinking.unwrap_or_default();
                        text.push_str(&piece);
                        tx_event
                            .send(Ok(ResponseEvent::ReasoningContentDelta {
                                delta: piece,
                                content_index: index,
                            }))
                            .await
                            .map_err(|_| ())?;
                    }
                    (Some(BlockState::ToolUse { input_json, .. }), "input_json_delta") => {
                        input_json.push_str(delta.partial_json.as_deref().unwrap_or_default());
                    }
                    _ => {}
                }
            }
        }
        "content_block_stop" => {
            let index = event.index.unwrap_or(0);
            if let Some(block) = state.blocks.remove(&index) {
                let item = match block {
                    BlockState::Text(text) => Some(ResponseItem::Message {
                        id: None,
                        role: "assistant".to_string(),
                        content: vec![ContentItem::OutputText { text }],
                        end_turn: None,
                        phase: None,
                    }),
                    BlockState::Thinking(text) => Some(ResponseItem::Reasoning {
                        id: String::new(),
                        summary: Vec::new(),
                        content: Some(vec![ReasoningItemContent::ReasoningText { text }]),
                        encrypted_content: None,
                    }),
                    BlockState::ToolUse {
                        id,
                        name,
                        input_json,
                    } => {
                        let kind = tool_kinds
                            .get(&name)
                            .copied()
                            .unwrap_or(AnthropicToolKind::FunctionLike);
                        match kind {
                            AnthropicToolKind::Custom => {
                                let input = serde_json::from_str::<Value>(&input_json)
                                    .ok()
                                    .and_then(|value| {
                                        value
                                            .get("input")
                                            .and_then(Value::as_str)
                                            .map(str::to_string)
                                    })
                                    .unwrap_or(input_json);
                                Some(ResponseItem::CustomToolCall {
                                    id: None,
                                    status: None,
                                    call_id: id,
                                    name,
                                    input,
                                })
                            }
                            AnthropicToolKind::FunctionLike => Some(ResponseItem::FunctionCall {
                                id: None,
                                name,
                                namespace: None,
                                arguments: input_json,
                                call_id: id,
                            }),
                        }
                    }
                };

                if let Some(item) = item {
                    tx_event
                        .send(Ok(ResponseEvent::OutputItemDone(item)))
                        .await
                        .map_err(|_| ())?;
                }
            }
        }
        "message_delta" => {
            if let Some(usage) = event.usage {
                if state.input_tokens.is_none() {
                    state.input_tokens = usage.input_tokens;
                }
                state.output_tokens = usage.output_tokens.or(state.output_tokens);
            }
        }
        "message_stop" => {
            let response_id = state
                .response_id
                .clone()
                .unwrap_or_else(|| "anthropic-message".to_string());
            tx_event
                .send(Ok(ResponseEvent::Completed {
                    response_id,
                    token_usage: state.token_usage(),
                }))
                .await
                .map_err(|_| ())?;
            return Err(());
        }
        "error" => {
            let message = event
                .error
                .and_then(|error| error.message)
                .unwrap_or_else(|| "anthropic stream error".to_string());
            tx_event
                .send(Err(ApiError::Stream(message)))
                .await
                .map_err(|_| ())?;
            return Err(());
        }
        "ping" => {}
        _ => {
            trace!("unhandled anthropic event: {}", event.kind);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use codex_client::TransportError;
    use futures::stream;
    use pretty_assertions::assert_eq;

    #[tokio::test]
    async fn anthropic_sse_maps_text_and_tool_use_into_response_events() {
        let body = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"usage\":{\"input_tokens\":11}}}\n\n",
            "event: content_block_start\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Working:\"}}\n\n",
            "event: content_block_stop\n",
            "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "event: content_block_start\n",
            "data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"tool_1\",\"name\":\"shell_command\",\"input\":{}}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"command\\\":\\\"pwd\\\"}\"}}\n\n",
            "event: content_block_stop\n",
            "data: {\"type\":\"content_block_stop\",\"index\":1}\n\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\",\"usage\":{\"output_tokens\":7}}\n\n",
            "event: message_stop\n",
            "data: {\"type\":\"message_stop\"}\n\n"
        );

        let stream = stream::iter(vec![Ok::<Bytes, TransportError>(Bytes::from(body))]).boxed();
        let (tx, mut rx) = mpsc::channel::<Result<ResponseEvent, ApiError>>(32);
        let mut tool_kinds = HashMap::new();
        tool_kinds.insert("shell_command".to_string(), AnthropicToolKind::FunctionLike);

        tokio::spawn(process_anthropic_sse(
            stream,
            tx,
            Duration::from_secs(5),
            None,
            tool_kinds,
        ));

        let mut events = Vec::new();
        while let Some(event) = rx.recv().await {
            events.push(event.expect("stream event"));
        }

        assert!(matches!(events[0], ResponseEvent::Created));
        assert!(matches!(
            &events[1],
            ResponseEvent::OutputItemAdded(ResponseItem::Message { role, content, .. })
                if role == "assistant" && content.len() == 1
        ));
        assert_eq!(
            events[2],
            ResponseEvent::OutputTextDelta("Working:".to_string())
        );
        assert!(matches!(
            &events[3],
            ResponseEvent::OutputItemDone(ResponseItem::Message { role, .. }) if role == "assistant"
        ));
        assert!(matches!(
            &events[4],
            ResponseEvent::OutputItemDone(ResponseItem::FunctionCall { name, call_id, arguments, .. })
                if name == "shell_command" && call_id == "tool_1" && arguments.contains("pwd")
        ));
        assert!(matches!(
            &events[5],
            ResponseEvent::Completed { response_id, token_usage: Some(usage) }
                if response_id == "msg_1" && usage.input_tokens == 11 && usage.output_tokens == 7
        ));
    }
}
