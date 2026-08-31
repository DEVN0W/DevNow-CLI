use super::*;
use pretty_assertions::assert_eq;

#[test]
fn default_stream_retry_budget_is_two_retries() {
    let provider: ModelProviderInfo = toml::from_str(
        r#"
name = "Test"
base_url = "https://example.com"
"#,
    )
    .unwrap();

    assert_eq!(provider.stream_max_retries(), 2);
}

#[test]
fn test_deserialize_ollama_model_provider_toml() {
    let azure_provider_toml = r#"
name = "Ollama"
base_url = "http://localhost:11434/v1"
        "#;
    let expected_provider = ModelProviderInfo {
        name: "Ollama".into(),
        base_url: Some("http://localhost:11434/v1".into()),
        env_key: None,
        env_key_instructions: None,
        experimental_bearer_token: None,
        wire_api: WireApi::Responses,
        query_params: None,
        http_headers: None,
        env_http_headers: None,
        request_max_retries: None,
        stream_max_retries: None,
        stream_idle_timeout_ms: None,
        websocket_connect_timeout_ms: None,
        requires_openai_auth: false,
        supports_websockets: false,
        extra_body: None,
    };

    let provider: ModelProviderInfo = toml::from_str(azure_provider_toml).unwrap();
    assert_eq!(expected_provider, provider);
}

#[test]
fn test_deserialize_azure_model_provider_toml() {
    let azure_provider_toml = r#"
name = "Azure"
base_url = "https://xxxxx.openai.azure.com/openai"
env_key = "AZURE_OPENAI_API_KEY"
query_params = { api-version = "2025-04-01-preview" }
        "#;
    let expected_provider = ModelProviderInfo {
        name: "Azure".into(),
        base_url: Some("https://xxxxx.openai.azure.com/openai".into()),
        env_key: Some("AZURE_OPENAI_API_KEY".into()),
        env_key_instructions: None,
        experimental_bearer_token: None,
        wire_api: WireApi::Responses,
        query_params: Some(maplit::hashmap! {
            "api-version".to_string() => "2025-04-01-preview".to_string(),
        }),
        http_headers: None,
        env_http_headers: None,
        request_max_retries: None,
        stream_max_retries: None,
        stream_idle_timeout_ms: None,
        websocket_connect_timeout_ms: None,
        requires_openai_auth: false,
        supports_websockets: false,
        extra_body: None,
    };

    let provider: ModelProviderInfo = toml::from_str(azure_provider_toml).unwrap();
    assert_eq!(expected_provider, provider);
}

#[test]
fn test_deserialize_example_model_provider_toml() {
    let azure_provider_toml = r#"
name = "Example"
base_url = "https://example.com"
env_key = "API_KEY"
http_headers = { "X-Example-Header" = "example-value" }
env_http_headers = { "X-Example-Env-Header" = "EXAMPLE_ENV_VAR" }
        "#;
    let expected_provider = ModelProviderInfo {
        name: "Example".into(),
        base_url: Some("https://example.com".into()),
        env_key: Some("API_KEY".into()),
        env_key_instructions: None,
        experimental_bearer_token: None,
        wire_api: WireApi::Responses,
        query_params: None,
        http_headers: Some(maplit::hashmap! {
            "X-Example-Header".to_string() => "example-value".to_string(),
        }),
        env_http_headers: Some(maplit::hashmap! {
            "X-Example-Env-Header".to_string() => "EXAMPLE_ENV_VAR".to_string(),
        }),
        request_max_retries: None,
        stream_max_retries: None,
        stream_idle_timeout_ms: None,
        websocket_connect_timeout_ms: None,
        requires_openai_auth: false,
        supports_websockets: false,
        extra_body: None,
    };

    let provider: ModelProviderInfo = toml::from_str(azure_provider_toml).unwrap();
    assert_eq!(expected_provider, provider);
}

#[test]
fn test_deserialize_provider_extra_body_toml() {
    let provider_toml = r#"
name = "Foundry Agent 1"
base_url = "https://example.services.ai.azure.com/api/projects/demo"
extra_body = { agent_reference = { name = "emea-swd-multi-agent-ai", version = "2", type = "agent_reference" } }
        "#;

    let provider: ModelProviderInfo = toml::from_str(provider_toml).unwrap();
    assert_eq!(
        provider.extra_body,
        Some(serde_json::json!({
            "agent_reference": {
                "name": "emea-swd-multi-agent-ai",
                "version": "2",
                "type": "agent_reference",
            }
        }))
    );
}

#[test]
fn test_deserialize_chat_wire_api_shows_helpful_error() {
    let provider_toml = r#"
name = "OpenAI using Chat Completions"
base_url = "https://api.openai.com/v1"
env_key = "OPENAI_API_KEY"
wire_api = "chat"
        "#;

    let err = toml::from_str::<ModelProviderInfo>(provider_toml).unwrap_err();
    assert!(err.to_string().contains(CHAT_WIRE_API_REMOVED_ERROR));
}

#[test]
fn test_deserialize_anthropic_messages_wire_api() {
    let provider_toml = r#"
name = "Claude on Foundry"
base_url = "https://example.services.ai.azure.com/anthropic"
env_key = "AZURE_FOUNDRY_API_KEY"
wire_api = "anthropic_messages"
        "#;

    let provider: ModelProviderInfo = toml::from_str(provider_toml).unwrap();
    assert_eq!(provider.wire_api, WireApi::AnthropicMessages);
}

#[test]
fn test_deserialize_anthropicmessages_wire_api_alias() {
    let provider_toml = r#"
name = "Claude on Foundry"
base_url = "https://example.services.ai.azure.com/anthropic"
env_key = "AZURE_FOUNDRY_API_KEY"
wire_api = "anthropicmessages"
        "#;

    let provider: ModelProviderInfo = toml::from_str(provider_toml).unwrap();
    assert_eq!(provider.wire_api, WireApi::AnthropicMessages);
}

#[test]
fn test_deserialize_managed_devnow_gateway_wire_api() {
    let provider_toml = r#"
name = "DevNow Gateway"
wire_api = "managed_devnow_gateway"
        "#;

    let provider: ModelProviderInfo = toml::from_str(provider_toml).unwrap();
    assert_eq!(provider.wire_api, WireApi::ManagedDevNowGateway);
}

#[test]
fn test_anthropic_provider_uses_retry_429_backoff() {
    let provider = ModelProviderInfo {
        name: "Claude on Foundry".into(),
        base_url: Some("https://example.services.ai.azure.com/anthropic".into()),
        env_key: Some("AZURE_FOUNDRY_API_KEY".into()),
        env_key_instructions: None,
        experimental_bearer_token: None,
        wire_api: WireApi::AnthropicMessages,
        query_params: None,
        extra_body: None,
        http_headers: None,
        env_http_headers: None,
        request_max_retries: None,
        stream_max_retries: None,
        stream_idle_timeout_ms: None,
        websocket_connect_timeout_ms: None,
        requires_openai_auth: false,
        supports_websockets: false,
    };

    let api_provider = provider.to_api_provider(None).unwrap();
    assert!(api_provider.retry.retry_429);
    assert_eq!(
        api_provider.retry.base_delay,
        std::time::Duration::from_millis(750)
    );
}

#[test]
fn test_deserialize_websocket_connect_timeout() {
    let provider_toml = r#"
name = "OpenAI"
base_url = "https://api.openai.com/v1"
websocket_connect_timeout_ms = 15000
supports_websockets = true
        "#;

    let provider: ModelProviderInfo = toml::from_str(provider_toml).unwrap();
    assert_eq!(provider.websocket_connect_timeout_ms, Some(15_000));
}
