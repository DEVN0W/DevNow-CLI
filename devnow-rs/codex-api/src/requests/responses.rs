use crate::provider::Provider;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use serde_json::Value;
use tracing::info;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Compression {
    #[default]
    None,
    Zstd,
}

pub(crate) fn attach_item_ids(payload_json: &mut Value, original_items: &[ResponseItem]) {
    let Some(input_value) = payload_json.get_mut("input") else {
        return;
    };
    let Value::Array(items) = input_value else {
        return;
    };

    for (value, item) in items.iter_mut().zip(original_items.iter()) {
        if let ResponseItem::Reasoning { id, .. }
        | ResponseItem::Message { id: Some(id), .. }
        | ResponseItem::WebSearchCall { id: Some(id), .. }
        | ResponseItem::FunctionCall { id: Some(id), .. }
        | ResponseItem::ToolSearchCall { id: Some(id), .. }
        | ResponseItem::LocalShellCall { id: Some(id), .. }
        | ResponseItem::CustomToolCall { id: Some(id), .. } = item
        {
            if id.is_empty() {
                continue;
            }

            if let Some(obj) = value.as_object_mut() {
                obj.insert("id".to_string(), Value::String(id.clone()));
            }
        }
    }
}

pub(crate) fn merge_extra_body(payload_json: &mut Value, extra_body: &Value) -> Result<(), String> {
    let Some(payload_object) = payload_json.as_object_mut() else {
        return Err("responses request body must serialize to a JSON object".to_string());
    };

    let Some(extra_object) = extra_body.as_object() else {
        return Err("provider extra_body must be a JSON object".to_string());
    };

    for (key, value) in extra_object {
        payload_object.insert(key.clone(), value.clone());
    }

    Ok(())
}

fn has_agent_reference(provider_extra_body: Option<&Value>) -> bool {
    provider_extra_body
        .and_then(|extra_body| extra_body.get("agent_reference"))
        .is_some()
}

pub(crate) fn strip_agent_reference_incompatible_fields(
    payload_json: &mut Value,
    provider_extra_body: Option<&Value>,
) {
    if !has_agent_reference(provider_extra_body) {
        return;
    }

    let Some(payload_object) = payload_json.as_object_mut() else {
        return;
    };

    let instructions = payload_object
        .get("instructions")
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|value| !value.trim().is_empty());
    if let Some(instructions) = instructions {
        let developer_message = serde_json::to_value(ResponseItem::Message {
            id: None,
            role: "developer".to_string(),
            content: vec![ContentItem::InputText { text: instructions }],
            end_turn: None,
            phase: None,
        })
        .expect("developer instruction message should serialize");

        match payload_object.get_mut("input") {
            Some(Value::Array(items)) => items.insert(0, developer_message),
            _ => {
                payload_object.insert("input".to_string(), Value::Array(vec![developer_message]));
            }
        }
    }

    payload_object.remove("instructions");
    payload_object.remove("tools");
    payload_object.remove("tool_choice");
    payload_object.remove("parallel_tool_calls");
}

pub(crate) fn log_responses_payload(
    transport: &'static str,
    provider: &Provider,
    request_json: &Value,
    payload_json: &Value,
    provider_extra_body: Option<&Value>,
) {
    let request_text = serde_json::to_string_pretty(request_json)
        .unwrap_or_else(|err| format!("<failed to serialize request body: {err}>"));
    let payload_text = serde_json::to_string_pretty(payload_json)
        .unwrap_or_else(|err| format!("<failed to serialize payload body: {err}>"));
    let extra_body_text = provider_extra_body
        .map(|extra_body| {
            serde_json::to_string_pretty(extra_body)
                .unwrap_or_else(|err| format!("<failed to serialize provider extra_body: {err}>"))
        })
        .unwrap_or_else(|| "<none>".to_string());

    info!(
        target: "codex_api::payload",
        transport,
        provider = %provider.name,
        url = %provider.url_for_path("responses"),
        request_json = %request_text,
        payload_json = %payload_text,
        provider_extra_body = %extra_body_text,
        "sending responses request"
    );
}
