// Per-model metadata. See PROVENANCE.md for data provenance.

use super::types::{Model, ThinkingLevel};

#[derive(Debug, Clone, Copy, Default)]
pub struct ModelMetadata {
    pub thinking_level_map: &'static [(ThinkingLevel, Option<&'static str>)],
    pub compat: ModelCompat,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ModelCompat {
    pub supports_reasoning_effort: Option<bool>,
    pub thinking_format: Option<&'static str>,
    pub force_adaptive_thinking: Option<bool>,
}

pub fn metadata_for(model: &Model) -> ModelMetadata {
    match (model.provider.as_str(), model.id.as_str()) {
        ("amazon-bedrock", "anthropic.claude-opus-4-6-v1") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("max"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("amazon-bedrock", "anthropic.claude-opus-4-7") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("xhigh"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("amazon-bedrock", "anthropic.claude-opus-4-8") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("xhigh"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("amazon-bedrock", "au.anthropic.claude-opus-4-6-v1") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("max"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("amazon-bedrock", "au.anthropic.claude-opus-4-8") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("xhigh"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("amazon-bedrock", "eu.anthropic.claude-fable-5") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::XHigh, Some("xhigh")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("amazon-bedrock", "eu.anthropic.claude-opus-4-6-v1") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("max"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("amazon-bedrock", "eu.anthropic.claude-opus-4-7") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("xhigh"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("amazon-bedrock", "eu.anthropic.claude-opus-4-8") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("xhigh"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("amazon-bedrock", "global.anthropic.claude-fable-5") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::XHigh, Some("xhigh")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("amazon-bedrock", "global.anthropic.claude-opus-4-6-v1") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("max"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("amazon-bedrock", "global.anthropic.claude-opus-4-7") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("xhigh"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("amazon-bedrock", "global.anthropic.claude-opus-4-8") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("xhigh"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("amazon-bedrock", "jp.anthropic.claude-opus-4-7") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("xhigh"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("amazon-bedrock", "jp.anthropic.claude-opus-4-8") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("xhigh"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("amazon-bedrock", "openai.gpt-5.4") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("xhigh"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("amazon-bedrock", "openai.gpt-5.5") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("xhigh"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("amazon-bedrock", "openai.gpt-5.6-luna") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("xhigh"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("amazon-bedrock", "openai.gpt-5.6-sol") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("xhigh"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("amazon-bedrock", "openai.gpt-5.6-terra") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("xhigh"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("amazon-bedrock", "us.anthropic.claude-fable-5") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::XHigh, Some("xhigh")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("amazon-bedrock", "us.anthropic.claude-opus-4-6-v1") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("max"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("amazon-bedrock", "us.anthropic.claude-opus-4-7") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("xhigh"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("amazon-bedrock", "us.anthropic.claude-opus-4-8") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("xhigh"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("ant-ling", "Ling-2.6-1T") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: Some("ant-ling"),
                force_adaptive_thinking: None,
            },
        },
        ("ant-ling", "Ling-2.6-flash") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: Some("ant-ling"),
                force_adaptive_thinking: None,
            },
        },
        ("ant-ling", "Ring-2.6-1T") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::Minimal, None),
                (ThinkingLevel::Low, None),
                (ThinkingLevel::Medium, None),
                (ThinkingLevel::High, Some("high")),
                (ThinkingLevel::XHigh, Some("xhigh")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: Some("ant-ling"),
                force_adaptive_thinking: None,
            },
        },
        ("anthropic", "claude-fable-5") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::XHigh, Some("xhigh")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: Some(true),
            },
        },
        ("anthropic", "claude-opus-4-6") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("max"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: Some(true),
            },
        },
        ("anthropic", "claude-opus-4-7") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("xhigh"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: Some(true),
            },
        },
        ("anthropic", "claude-opus-4-8") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("xhigh"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: Some(true),
            },
        },
        ("anthropic", "claude-sonnet-4-6") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: Some(true),
            },
        },
        ("azure-openai-responses", "gpt-5") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::Off, None)],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("azure-openai-responses", "gpt-5-chat-latest") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::Off, None)],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("azure-openai-responses", "gpt-5-codex") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::Off, None)],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("azure-openai-responses", "gpt-5-mini") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::Off, None)],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("azure-openai-responses", "gpt-5-nano") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::Off, None)],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("azure-openai-responses", "gpt-5-pro") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::Off, None)],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("azure-openai-responses", "gpt-5.1") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::Off, None)],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("azure-openai-responses", "gpt-5.1-chat-latest") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::Off, None)],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("azure-openai-responses", "gpt-5.1-codex") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::Off, None)],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("azure-openai-responses", "gpt-5.1-codex-max") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::Off, None)],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("azure-openai-responses", "gpt-5.1-codex-mini") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::Off, None)],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("azure-openai-responses", "gpt-5.2") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::XHigh, Some("xhigh")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("azure-openai-responses", "gpt-5.2-chat-latest") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::XHigh, Some("xhigh")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("azure-openai-responses", "gpt-5.2-codex") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::XHigh, Some("xhigh")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("azure-openai-responses", "gpt-5.2-pro") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::XHigh, Some("xhigh")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("azure-openai-responses", "gpt-5.3-chat-latest") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::XHigh, Some("xhigh")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("azure-openai-responses", "gpt-5.3-codex") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::XHigh, Some("xhigh")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("azure-openai-responses", "gpt-5.3-codex-spark") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::XHigh, Some("xhigh")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("azure-openai-responses", "gpt-5.4") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::XHigh, Some("xhigh")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("azure-openai-responses", "gpt-5.4-mini") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::XHigh, Some("xhigh")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("azure-openai-responses", "gpt-5.4-nano") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::XHigh, Some("xhigh")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("azure-openai-responses", "gpt-5.4-pro") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::XHigh, Some("xhigh")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("azure-openai-responses", "gpt-5.5") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::XHigh, Some("xhigh")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("azure-openai-responses", "gpt-5.5-pro") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::XHigh, Some("xhigh")),
                (ThinkingLevel::Minimal, None),
                (ThinkingLevel::Low, None),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("azure-openai-responses", "gpt-5.6-luna") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::XHigh, Some("xhigh")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("azure-openai-responses", "gpt-5.6-sol") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::XHigh, Some("xhigh")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("azure-openai-responses", "gpt-5.6-terra") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::XHigh, Some("xhigh")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("cloudflare-ai-gateway", "claude-fable-5") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::XHigh, Some("xhigh")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: Some(true),
            },
        },
        ("cloudflare-ai-gateway", "claude-opus-4-6") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("max"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: Some(true),
            },
        },
        ("cloudflare-ai-gateway", "claude-opus-4-7") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("xhigh"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: Some(true),
            },
        },
        ("cloudflare-ai-gateway", "claude-opus-4-8") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("xhigh"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: Some(true),
            },
        },
        ("cloudflare-ai-gateway", "claude-sonnet-4-6") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: Some(true),
            },
        },
        ("cloudflare-ai-gateway", "gpt-5.1") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::Off, None)],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("cloudflare-ai-gateway", "gpt-5.1-codex") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::Off, None)],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("cloudflare-ai-gateway", "gpt-5.2") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::XHigh, Some("xhigh")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("cloudflare-ai-gateway", "gpt-5.2-codex") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::XHigh, Some("xhigh")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("cloudflare-ai-gateway", "gpt-5.3-codex") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::XHigh, Some("xhigh")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("cloudflare-ai-gateway", "gpt-5.4") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::XHigh, Some("xhigh")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("cloudflare-ai-gateway", "gpt-5.5") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::XHigh, Some("xhigh")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("cloudflare-ai-gateway", "gpt-5.6-luna") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::XHigh, Some("xhigh")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("cloudflare-ai-gateway", "gpt-5.6-sol") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::XHigh, Some("xhigh")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("cloudflare-ai-gateway", "gpt-5.6-terra") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::XHigh, Some("xhigh")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("cloudflare-ai-gateway", "workers-ai/@cf/moonshotai/kimi-k2.5") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("cloudflare-ai-gateway", "workers-ai/@cf/moonshotai/kimi-k2.6") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("cloudflare-ai-gateway", "workers-ai/@cf/nvidia/nemotron-3-120b-a12b") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("cloudflare-ai-gateway", "workers-ai/@cf/zai-org/glm-4.7-flash") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("deepseek", "deepseek-v4-flash") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Minimal, None),
                (ThinkingLevel::Low, None),
                (ThinkingLevel::Medium, None),
                (ThinkingLevel::High, Some("high")),
                (ThinkingLevel::XHigh, Some("max")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("deepseek"),
                force_adaptive_thinking: None,
            },
        },
        ("deepseek", "deepseek-v4-pro") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Minimal, None),
                (ThinkingLevel::Low, None),
                (ThinkingLevel::Medium, None),
                (ThinkingLevel::High, Some("high")),
                (ThinkingLevel::XHigh, Some("max")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("deepseek"),
                force_adaptive_thinking: None,
            },
        },
        ("fireworks", "accounts/fireworks/models/glm-5p2") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, Some("none")),
                (ThinkingLevel::Minimal, None),
                (ThinkingLevel::Low, Some("high")),
                (ThinkingLevel::Medium, Some("high")),
                (ThinkingLevel::XHigh, Some("max")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("github-copilot", "claude-fable-5") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("github-copilot", "claude-opus-4.6") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("max"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: Some(true),
            },
        },
        ("github-copilot", "claude-opus-4.7") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::XHigh, Some("xhigh")),
                (ThinkingLevel::Minimal, Some("low")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: Some(true),
            },
        },
        ("github-copilot", "claude-opus-4.8") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::XHigh, Some("xhigh")),
                (ThinkingLevel::Minimal, Some("low")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: Some(true),
            },
        },
        ("github-copilot", "claude-sonnet-4.6") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Minimal, Some("low")),
                (ThinkingLevel::XHigh, Some("max")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: Some(true),
            },
        },
        ("github-copilot", "gemini-2.5-pro") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("github-copilot", "gemini-3-flash-preview") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("github-copilot", "gemini-3.1-pro-preview") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("github-copilot", "gemini-3.5-flash") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("github-copilot", "gpt-4.1") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("github-copilot", "gpt-5-mini") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::Minimal, Some("low")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("github-copilot", "gpt-5.2") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::Minimal, Some("low")),
                (ThinkingLevel::XHigh, Some("xhigh")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("github-copilot", "gpt-5.2-codex") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::Minimal, Some("low")),
                (ThinkingLevel::XHigh, Some("xhigh")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("github-copilot", "gpt-5.3-codex") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::Minimal, Some("low")),
                (ThinkingLevel::XHigh, Some("xhigh")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("github-copilot", "gpt-5.4") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::Minimal, Some("low")),
                (ThinkingLevel::XHigh, Some("xhigh")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("github-copilot", "gpt-5.4-mini") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::Minimal, Some("low")),
                (ThinkingLevel::XHigh, Some("xhigh")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("github-copilot", "gpt-5.4-nano") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::Minimal, Some("low")),
                (ThinkingLevel::XHigh, Some("xhigh")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("github-copilot", "gpt-5.5") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::Minimal, Some("low")),
                (ThinkingLevel::XHigh, Some("xhigh")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("google-vertex", "gemini-3-flash-preview") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::Off, None)],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("google-vertex", "gemini-3.1-flash-lite") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::Off, None)],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("google-vertex", "gemini-3.1-pro-preview") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::Minimal, None),
                (ThinkingLevel::Low, Some("LOW")),
                (ThinkingLevel::Medium, None),
                (ThinkingLevel::High, Some("HIGH")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("google-vertex", "gemini-3.1-pro-preview-customtools") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::Minimal, None),
                (ThinkingLevel::Low, Some("LOW")),
                (ThinkingLevel::Medium, None),
                (ThinkingLevel::High, Some("HIGH")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("google-vertex", "gemini-3.5-flash") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::Off, None)],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("google-vertex", "gemini-flash-latest") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::Off, None)],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("google-vertex", "gemini-flash-lite-latest") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::Off, None)],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("google", "gemini-3-flash-preview") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::Off, None)],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("google", "gemini-3-pro-preview") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::Minimal, None),
                (ThinkingLevel::Low, Some("LOW")),
                (ThinkingLevel::Medium, None),
                (ThinkingLevel::High, Some("HIGH")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("google", "gemini-3.1-flash-lite") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::Off, None)],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("google", "gemini-3.1-flash-lite-preview") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::Off, None)],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("google", "gemini-3.1-pro-preview") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::Minimal, None),
                (ThinkingLevel::Low, Some("LOW")),
                (ThinkingLevel::Medium, None),
                (ThinkingLevel::High, Some("HIGH")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("google", "gemini-3.1-pro-preview-customtools") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::Minimal, None),
                (ThinkingLevel::Low, Some("LOW")),
                (ThinkingLevel::Medium, None),
                (ThinkingLevel::High, Some("HIGH")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("google", "gemini-3.5-flash") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::Off, None)],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("google", "gemini-flash-latest") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::Off, None)],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("google", "gemini-flash-lite-latest") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::Off, None)],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("google", "gemma-4-26b-a4b-it") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::Minimal, Some("MINIMAL")),
                (ThinkingLevel::Low, None),
                (ThinkingLevel::Medium, None),
                (ThinkingLevel::High, Some("HIGH")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("google", "gemma-4-31b-it") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::Minimal, Some("MINIMAL")),
                (ThinkingLevel::Low, None),
                (ThinkingLevel::Medium, None),
                (ThinkingLevel::High, Some("HIGH")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("groq", "qwen/qwen3-32b") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Minimal, None),
                (ThinkingLevel::Low, None),
                (ThinkingLevel::Medium, None),
                (ThinkingLevel::High, Some("default")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("moonshotai-cn", "kimi-k2-0711-preview") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: Some("deepseek"),
                force_adaptive_thinking: None,
            },
        },
        ("moonshotai-cn", "kimi-k2-0905-preview") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: Some("deepseek"),
                force_adaptive_thinking: None,
            },
        },
        ("moonshotai-cn", "kimi-k2-thinking") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: Some("deepseek"),
                force_adaptive_thinking: None,
            },
        },
        ("moonshotai-cn", "kimi-k2-thinking-turbo") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: Some("deepseek"),
                force_adaptive_thinking: None,
            },
        },
        ("moonshotai-cn", "kimi-k2-turbo-preview") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: Some("deepseek"),
                force_adaptive_thinking: None,
            },
        },
        ("moonshotai-cn", "kimi-k2.5") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: Some("deepseek"),
                force_adaptive_thinking: None,
            },
        },
        ("moonshotai-cn", "kimi-k2.6") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: Some("deepseek"),
                force_adaptive_thinking: None,
            },
        },
        ("moonshotai-cn", "kimi-k2.7-code") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::Off, None)],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: Some("deepseek"),
                force_adaptive_thinking: None,
            },
        },
        ("moonshotai-cn", "kimi-k2.7-code-highspeed") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::Off, None)],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: Some("deepseek"),
                force_adaptive_thinking: None,
            },
        },
        ("moonshotai", "kimi-k2-0711-preview") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: Some("deepseek"),
                force_adaptive_thinking: None,
            },
        },
        ("moonshotai", "kimi-k2-0905-preview") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: Some("deepseek"),
                force_adaptive_thinking: None,
            },
        },
        ("moonshotai", "kimi-k2-thinking") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: Some("deepseek"),
                force_adaptive_thinking: None,
            },
        },
        ("moonshotai", "kimi-k2-thinking-turbo") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: Some("deepseek"),
                force_adaptive_thinking: None,
            },
        },
        ("moonshotai", "kimi-k2-turbo-preview") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: Some("deepseek"),
                force_adaptive_thinking: None,
            },
        },
        ("moonshotai", "kimi-k2.5") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: Some("deepseek"),
                force_adaptive_thinking: None,
            },
        },
        ("moonshotai", "kimi-k2.6") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: Some("deepseek"),
                force_adaptive_thinking: None,
            },
        },
        ("moonshotai", "kimi-k2.7-code") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::Off, None)],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: Some("deepseek"),
                force_adaptive_thinking: None,
            },
        },
        ("moonshotai", "kimi-k2.7-code-highspeed") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::Off, None)],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: Some("deepseek"),
                force_adaptive_thinking: None,
            },
        },
        ("nvidia", "meta/llama-3.1-70b-instruct") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("nvidia", "meta/llama-3.1-8b-instruct") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("nvidia", "meta/llama-3.2-11b-vision-instruct") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("nvidia", "meta/llama-3.2-90b-vision-instruct") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("nvidia", "meta/llama-3.3-70b-instruct") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("nvidia", "mistralai/mistral-large-3-675b-instruct-2512") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("nvidia", "mistralai/mistral-small-4-119b-2603") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("nvidia", "moonshotai/kimi-k2.6") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("nvidia", "nvidia/nemotron-3-nano-30b-a3b") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("nvidia", "nvidia/nemotron-3-nano-omni-30b-a3b-reasoning") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("nvidia", "nvidia/nemotron-3-super-120b-a12b") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("nvidia", "nvidia/nemotron-3-ultra-550b-a55b") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("nvidia", "nvidia/nvidia-nemotron-nano-9b-v2") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("nvidia", "openai/gpt-oss-120b") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("nvidia", "openai/gpt-oss-20b") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("nvidia", "qwen/qwen3.5-122b-a10b") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("nvidia", "stepfun-ai/step-3.5-flash") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("nvidia", "stepfun-ai/step-3.7-flash") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("nvidia", "z-ai/glm-5.1") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("openai-codex", "gpt-5.3-codex-spark") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::XHigh, Some("xhigh")),
                (ThinkingLevel::Minimal, Some("low")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("openai-codex", "gpt-5.4") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::XHigh, Some("xhigh")),
                (ThinkingLevel::Minimal, Some("low")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("openai-codex", "gpt-5.4-mini") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::XHigh, Some("xhigh")),
                (ThinkingLevel::Minimal, Some("low")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("openai-codex", "gpt-5.5") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::XHigh, Some("xhigh")),
                (ThinkingLevel::Minimal, Some("low")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("openai-codex", "gpt-5.6-luna") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::XHigh, Some("xhigh")),
                (ThinkingLevel::Minimal, Some("low")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("openai-codex", "gpt-5.6-sol") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::XHigh, Some("xhigh")),
                (ThinkingLevel::Minimal, Some("low")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("openai-codex", "gpt-5.6-terra") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::XHigh, Some("xhigh")),
                (ThinkingLevel::Minimal, Some("low")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("openai", "gpt-5") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::Off, None)],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("openai", "gpt-5-chat-latest") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::Off, None)],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("openai", "gpt-5-codex") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::Off, None)],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("openai", "gpt-5-mini") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::Off, None)],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("openai", "gpt-5-nano") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::Off, None)],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("openai", "gpt-5-pro") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::Off, None)],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("openai", "gpt-5.1") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::Off, Some("none"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("openai", "gpt-5.1-chat-latest") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::Off, None)],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("openai", "gpt-5.1-codex") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::Off, None)],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("openai", "gpt-5.1-codex-max") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::Off, None)],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("openai", "gpt-5.1-codex-mini") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::Off, None)],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("openai", "gpt-5.2") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, Some("none")),
                (ThinkingLevel::XHigh, Some("xhigh")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("openai", "gpt-5.2-chat-latest") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::XHigh, Some("xhigh")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("openai", "gpt-5.2-codex") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::XHigh, Some("xhigh")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("openai", "gpt-5.2-pro") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::XHigh, Some("xhigh")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("openai", "gpt-5.3-chat-latest") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::XHigh, Some("xhigh")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("openai", "gpt-5.3-codex") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, Some("none")),
                (ThinkingLevel::XHigh, Some("xhigh")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("openai", "gpt-5.3-codex-spark") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::XHigh, Some("xhigh")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("openai", "gpt-5.4") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, Some("none")),
                (ThinkingLevel::XHigh, Some("xhigh")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("openai", "gpt-5.4-mini") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, Some("none")),
                (ThinkingLevel::XHigh, Some("xhigh")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("openai", "gpt-5.4-nano") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, Some("none")),
                (ThinkingLevel::XHigh, Some("xhigh")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("openai", "gpt-5.4-pro") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::XHigh, Some("xhigh")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("openai", "gpt-5.5") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, Some("none")),
                (ThinkingLevel::XHigh, Some("xhigh")),
                (ThinkingLevel::Minimal, None),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("openai", "gpt-5.5-pro") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::XHigh, Some("xhigh")),
                (ThinkingLevel::Minimal, None),
                (ThinkingLevel::Low, None),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("openai", "gpt-5.6-luna") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, Some("none")),
                (ThinkingLevel::XHigh, Some("xhigh")),
                (ThinkingLevel::Minimal, None),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("openai", "gpt-5.6-sol") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, Some("none")),
                (ThinkingLevel::XHigh, Some("xhigh")),
                (ThinkingLevel::Minimal, None),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("openai", "gpt-5.6-terra") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, Some("none")),
                (ThinkingLevel::XHigh, Some("xhigh")),
                (ThinkingLevel::Minimal, None),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("opencode-go", "deepseek-v4-flash") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Minimal, None),
                (ThinkingLevel::Low, None),
                (ThinkingLevel::Medium, None),
                (ThinkingLevel::High, Some("high")),
                (ThinkingLevel::XHigh, Some("max")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("deepseek"),
                force_adaptive_thinking: None,
            },
        },
        ("opencode-go", "deepseek-v4-pro") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Minimal, None),
                (ThinkingLevel::Low, None),
                (ThinkingLevel::Medium, None),
                (ThinkingLevel::High, Some("high")),
                (ThinkingLevel::XHigh, Some("max")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("deepseek"),
                force_adaptive_thinking: None,
            },
        },
        ("opencode-go", "glm-5.2") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::Minimal, None),
                (ThinkingLevel::Low, None),
                (ThinkingLevel::Medium, None),
                (ThinkingLevel::High, Some("high")),
                (ThinkingLevel::XHigh, Some("max")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("opencode-go", "kimi-k2.6") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Minimal, None),
                (ThinkingLevel::Low, None),
                (ThinkingLevel::Medium, None),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: Some("deepseek"),
                force_adaptive_thinking: None,
            },
        },
        ("opencode-go", "qwen3.6-plus") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("qwen"),
                force_adaptive_thinking: None,
            },
        },
        ("opencode", "claude-opus-4-6") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("max"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: Some(true),
            },
        },
        ("opencode", "claude-opus-4-7") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("xhigh"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: Some(true),
            },
        },
        ("opencode", "claude-opus-4-8") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("xhigh"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: Some(true),
            },
        },
        ("opencode", "claude-sonnet-4-6") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: Some(true),
            },
        },
        ("opencode", "deepseek-v4-flash") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Minimal, None),
                (ThinkingLevel::Low, None),
                (ThinkingLevel::Medium, None),
                (ThinkingLevel::High, Some("high")),
                (ThinkingLevel::XHigh, Some("max")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("opencode", "deepseek-v4-flash-free") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Minimal, None),
                (ThinkingLevel::Low, None),
                (ThinkingLevel::Medium, None),
                (ThinkingLevel::High, Some("high")),
                (ThinkingLevel::XHigh, Some("max")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("opencode", "deepseek-v4-pro") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Minimal, None),
                (ThinkingLevel::Low, None),
                (ThinkingLevel::Medium, None),
                (ThinkingLevel::High, Some("high")),
                (ThinkingLevel::XHigh, Some("max")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("opencode", "gemini-3-flash") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::Off, None)],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("opencode", "gemini-3.1-pro") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::Minimal, None),
                (ThinkingLevel::Low, Some("LOW")),
                (ThinkingLevel::Medium, None),
                (ThinkingLevel::High, Some("HIGH")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("opencode", "gemini-3.5-flash") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::Off, None)],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("opencode", "gpt-5") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::Off, None)],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("opencode", "gpt-5-codex") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::Off, None)],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("opencode", "gpt-5-nano") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::Off, None)],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("opencode", "gpt-5.1") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::Off, None)],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("opencode", "gpt-5.1-codex") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::Off, None)],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("opencode", "gpt-5.1-codex-max") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::Off, None)],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("opencode", "gpt-5.1-codex-mini") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::Off, None)],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("opencode", "gpt-5.2") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::XHigh, Some("xhigh")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("opencode", "gpt-5.2-codex") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::XHigh, Some("xhigh")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("opencode", "gpt-5.3-codex") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::XHigh, Some("xhigh")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("opencode", "gpt-5.4") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::XHigh, Some("xhigh")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("opencode", "gpt-5.4-mini") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::XHigh, Some("xhigh")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("opencode", "gpt-5.4-nano") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::XHigh, Some("xhigh")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("opencode", "gpt-5.4-pro") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::XHigh, Some("xhigh")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("opencode", "gpt-5.5") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::XHigh, Some("xhigh")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("opencode", "gpt-5.5-pro") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::XHigh, Some("xhigh")),
                (ThinkingLevel::Minimal, None),
                (ThinkingLevel::Low, None),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("opencode", "grok-build-0.1") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::Minimal, None),
                (ThinkingLevel::Low, None),
                (ThinkingLevel::Medium, None),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("opencode", "kimi-k2.6") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: Some("deepseek"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "ai21/jamba-large-1.7") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "amazon/nova-2-lite-v1") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "amazon/nova-lite-v1") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "amazon/nova-micro-v1") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "amazon/nova-premier-v1") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "amazon/nova-pro-v1") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "anthropic/claude-3-haiku") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "anthropic/claude-fable-5") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "anthropic/claude-haiku-4.5") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "anthropic/claude-opus-4") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "anthropic/claude-opus-4.1") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "anthropic/claude-opus-4.5") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "anthropic/claude-opus-4.6") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("max"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "anthropic/claude-opus-4.6-fast") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("max"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "anthropic/claude-opus-4.7") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("xhigh"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "anthropic/claude-opus-4.7-fast") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("xhigh"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "anthropic/claude-opus-4.8") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("xhigh"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "anthropic/claude-opus-4.8-fast") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("xhigh"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "anthropic/claude-sonnet-4") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "anthropic/claude-sonnet-4.5") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "anthropic/claude-sonnet-4.6") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "arcee-ai/trinity-large-thinking") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "arcee-ai/trinity-mini") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "arcee-ai/virtuoso-large") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "auto") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "bytedance-seed/seed-1.6") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "bytedance-seed/seed-1.6-flash") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "bytedance-seed/seed-2.0-lite") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "bytedance-seed/seed-2.0-mini") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "cohere/command-r-08-2024") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "cohere/command-r-plus-08-2024") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "cohere/north-mini-code:free") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "deepseek/deepseek-chat") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "deepseek/deepseek-chat-v3-0324") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "deepseek/deepseek-chat-v3.1") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "deepseek/deepseek-r1") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "deepseek/deepseek-r1-0528") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "deepseek/deepseek-v3.1-terminus") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "deepseek/deepseek-v3.2") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "deepseek/deepseek-v3.2-exp") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "deepseek/deepseek-v4-flash") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Minimal, None),
                (ThinkingLevel::Low, None),
                (ThinkingLevel::Medium, None),
                (ThinkingLevel::High, Some("high")),
                (ThinkingLevel::XHigh, Some("xhigh")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "deepseek/deepseek-v4-pro") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Minimal, None),
                (ThinkingLevel::Low, None),
                (ThinkingLevel::Medium, None),
                (ThinkingLevel::High, Some("high")),
                (ThinkingLevel::XHigh, Some("xhigh")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "google/gemini-2.5-flash") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "google/gemini-2.5-flash-lite") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "google/gemini-2.5-flash-lite-preview-09-2025") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "google/gemini-2.5-pro") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "google/gemini-2.5-pro-preview") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "google/gemini-2.5-pro-preview-05-06") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "google/gemini-3-flash-preview") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "google/gemini-3-pro-image") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "google/gemini-3.1-flash-lite") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "google/gemini-3.1-flash-lite-preview") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "google/gemini-3.1-pro-preview") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "google/gemini-3.1-pro-preview-customtools") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "google/gemini-3.5-flash") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "google/gemma-3-12b-it") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "google/gemma-3-27b-it") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "google/gemma-4-26b-a4b-it") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "google/gemma-4-26b-a4b-it:free") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "google/gemma-4-31b-it") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "google/gemma-4-31b-it:free") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "ibm-granite/granite-4.1-8b") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "inception/mercury-2") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::Off, None)],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "inclusionai/ling-2.6-1t") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "inclusionai/ling-2.6-flash") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "inclusionai/ring-2.6-1t") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "kwaipilot/kat-coder-pro-v2") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "liquid/lfm-2.5-1.2b-thinking:free") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "meta-llama/llama-3.1-70b-instruct") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "meta-llama/llama-3.1-8b-instruct") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "meta-llama/llama-3.3-70b-instruct") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "meta-llama/llama-3.3-70b-instruct:free") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "meta-llama/llama-4-maverick") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "meta-llama/llama-4-scout") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "minimax/minimax-m1") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "minimax/minimax-m2") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "minimax/minimax-m2.1") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "minimax/minimax-m2.5") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "minimax/minimax-m2.7") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "minimax/minimax-m3") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "mistralai/codestral-2508") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "mistralai/devstral-2512") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "mistralai/ministral-14b-2512") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "mistralai/ministral-3b-2512") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "mistralai/ministral-8b-2512") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "mistralai/mistral-large") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "mistralai/mistral-large-2407") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "mistralai/mistral-large-2512") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "mistralai/mistral-medium-3") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "mistralai/mistral-medium-3-5") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "mistralai/mistral-medium-3.1") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "mistralai/mistral-nemo") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "mistralai/mistral-saba") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "mistralai/mistral-small-2603") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "mistralai/mistral-small-3.2-24b-instruct") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "mistralai/mixtral-8x22b-instruct") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "mistralai/voxtral-small-24b-2507") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "moonshotai/kimi-k2") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "moonshotai/kimi-k2-0905") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "moonshotai/kimi-k2-thinking") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "moonshotai/kimi-k2.5") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "moonshotai/kimi-k2.6") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "moonshotai/kimi-k2.7-code") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "nvidia/llama-3.3-nemotron-super-49b-v1.5") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "nvidia/nemotron-3-nano-30b-a3b") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "nvidia/nemotron-3-nano-30b-a3b:free") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "nvidia/nemotron-3-nano-omni-30b-a3b-reasoning:free") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "nvidia/nemotron-3-super-120b-a12b") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "nvidia/nemotron-3-super-120b-a12b:free") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "nvidia/nemotron-3-ultra-550b-a55b") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "nvidia/nemotron-3-ultra-550b-a55b:free") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "nvidia/nemotron-nano-12b-v2-vl:free") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "nvidia/nemotron-nano-9b-v2:free") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "openai/gpt-3.5-turbo") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "openai/gpt-3.5-turbo-0613") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "openai/gpt-3.5-turbo-16k") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "openai/gpt-4") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "openai/gpt-4-turbo") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "openai/gpt-4-turbo-preview") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "openai/gpt-4.1") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "openai/gpt-4.1-mini") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "openai/gpt-4.1-nano") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "openai/gpt-4o") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "openai/gpt-4o-2024-05-13") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "openai/gpt-4o-2024-08-06") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "openai/gpt-4o-2024-11-20") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "openai/gpt-4o-mini") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "openai/gpt-4o-mini-2024-07-18") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "openai/gpt-5") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "openai/gpt-5-codex") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "openai/gpt-5-mini") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "openai/gpt-5-nano") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "openai/gpt-5-pro") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "openai/gpt-5.1") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "openai/gpt-5.1-chat") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "openai/gpt-5.1-codex") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "openai/gpt-5.1-codex-max") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "openai/gpt-5.1-codex-mini") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "openai/gpt-5.2") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("xhigh"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "openai/gpt-5.2-chat") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("xhigh"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "openai/gpt-5.2-codex") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("xhigh"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "openai/gpt-5.2-pro") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("xhigh"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "openai/gpt-5.3-chat") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("xhigh"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "openai/gpt-5.3-codex") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("xhigh"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "openai/gpt-5.4") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("xhigh"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "openai/gpt-5.4-mini") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("xhigh"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "openai/gpt-5.4-nano") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("xhigh"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "openai/gpt-5.4-pro") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("xhigh"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "openai/gpt-5.5") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("xhigh"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "openai/gpt-5.5-pro") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::XHigh, Some("xhigh")),
                (ThinkingLevel::Off, None),
                (ThinkingLevel::Minimal, None),
                (ThinkingLevel::Low, None),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "openai/gpt-audio") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "openai/gpt-audio-mini") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "openai/gpt-chat-latest") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "openai/gpt-oss-120b") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "openai/gpt-oss-120b:free") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "openai/gpt-oss-20b") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "openai/gpt-oss-20b:free") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "openai/gpt-oss-safeguard-20b") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "openai/o1") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "openai/o3") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "openai/o3-deep-research") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "openai/o3-mini") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "openai/o3-mini-high") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "openai/o3-pro") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "openai/o4-mini") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "openai/o4-mini-deep-research") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "openai/o4-mini-high") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "openrouter/auto") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "openrouter/free") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "openrouter/fusion") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "openrouter/owl-alpha") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "poolside/laguna-m.1") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "poolside/laguna-m.1:free") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "poolside/laguna-xs.2") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "poolside/laguna-xs.2:free") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "qwen/qwen-2.5-72b-instruct") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "qwen/qwen-2.5-7b-instruct") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "qwen/qwen-plus") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "qwen/qwen-plus-2025-07-28") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "qwen/qwen-plus-2025-07-28:thinking") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "qwen/qwen3-14b") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "qwen/qwen3-235b-a22b") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "qwen/qwen3-235b-a22b-2507") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "qwen/qwen3-235b-a22b-thinking-2507") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "qwen/qwen3-30b-a3b") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "qwen/qwen3-30b-a3b-instruct-2507") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "qwen/qwen3-30b-a3b-thinking-2507") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "qwen/qwen3-32b") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "qwen/qwen3-8b") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "qwen/qwen3-coder") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "qwen/qwen3-coder-30b-a3b-instruct") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "qwen/qwen3-coder-flash") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "qwen/qwen3-coder-next") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "qwen/qwen3-coder-plus") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "qwen/qwen3-coder:free") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "qwen/qwen3-max") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "qwen/qwen3-max-thinking") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "qwen/qwen3-next-80b-a3b-instruct") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "qwen/qwen3-next-80b-a3b-instruct:free") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "qwen/qwen3-next-80b-a3b-thinking") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "qwen/qwen3-vl-235b-a22b-instruct") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "qwen/qwen3-vl-235b-a22b-thinking") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "qwen/qwen3-vl-30b-a3b-instruct") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "qwen/qwen3-vl-30b-a3b-thinking") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "qwen/qwen3-vl-32b-instruct") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "qwen/qwen3-vl-8b-instruct") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "qwen/qwen3-vl-8b-thinking") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "qwen/qwen3.5-122b-a10b") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "qwen/qwen3.5-27b") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "qwen/qwen3.5-35b-a3b") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "qwen/qwen3.5-397b-a17b") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "qwen/qwen3.5-9b") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "qwen/qwen3.5-flash-02-23") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "qwen/qwen3.5-plus-02-15") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "qwen/qwen3.5-plus-20260420") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "qwen/qwen3.6-27b") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "qwen/qwen3.6-35b-a3b") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "qwen/qwen3.6-flash") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "qwen/qwen3.6-max-preview") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "qwen/qwen3.6-plus") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "qwen/qwen3.7-max") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "qwen/qwen3.7-plus") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "rekaai/reka-edge") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "relace/relace-search") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "sakana/fugu-ultra") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "sao10k/l3.1-euryale-70b") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "stepfun/step-3.5-flash") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "stepfun/step-3.7-flash") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "tencent/hy3-preview") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "thedrummer/unslopnemo-12b") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "upstage/solar-pro-3") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "x-ai/grok-4.20") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "x-ai/grok-4.3") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "x-ai/grok-build-0.1") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "xiaomi/mimo-v2.5") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "xiaomi/mimo-v2.5-pro") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "z-ai/glm-4.5") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "z-ai/glm-4.5-air") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "z-ai/glm-4.5v") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "z-ai/glm-4.6") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "z-ai/glm-4.6v") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "z-ai/glm-4.7") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "z-ai/glm-4.7-flash") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "z-ai/glm-5") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "z-ai/glm-5-turbo") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "z-ai/glm-5.1") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "z-ai/glm-5.2") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("xhigh"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "z-ai/glm-5v-turbo") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "~anthropic/claude-fable-latest") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "~anthropic/claude-haiku-latest") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "~anthropic/claude-opus-latest") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "~anthropic/claude-sonnet-latest") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "~google/gemini-flash-latest") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "~google/gemini-pro-latest") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "~moonshotai/kimi-latest") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "~openai/gpt-latest") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("openrouter", "~openai/gpt-mini-latest") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("openrouter"),
                force_adaptive_thinking: None,
            },
        },
        ("together", "MiniMaxAI/MiniMax-M2.7") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Off, None),
                (ThinkingLevel::Minimal, None),
                (ThinkingLevel::Low, None),
                (ThinkingLevel::Medium, None),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("together", "MiniMaxAI/MiniMax-M3") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Minimal, None),
                (ThinkingLevel::Low, None),
                (ThinkingLevel::Medium, None),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: Some("together"),
                force_adaptive_thinking: None,
            },
        },
        ("together", "Qwen/Qwen2.5-7B-Instruct-Turbo") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: Some("together"),
                force_adaptive_thinking: None,
            },
        },
        ("together", "Qwen/Qwen3-235B-A22B-Instruct-2507-tput") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: Some("together"),
                force_adaptive_thinking: None,
            },
        },
        ("together", "Qwen/Qwen3.5-397B-A17B") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Minimal, None),
                (ThinkingLevel::Low, None),
                (ThinkingLevel::Medium, None),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: Some("together"),
                force_adaptive_thinking: None,
            },
        },
        ("together", "Qwen/Qwen3.5-9B") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Minimal, None),
                (ThinkingLevel::Low, None),
                (ThinkingLevel::Medium, None),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: Some("together"),
                force_adaptive_thinking: None,
            },
        },
        ("together", "Qwen/Qwen3.6-Plus") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Minimal, None),
                (ThinkingLevel::Low, None),
                (ThinkingLevel::Medium, None),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: Some("together"),
                force_adaptive_thinking: None,
            },
        },
        ("together", "Qwen/Qwen3.7-Max") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: Some("together"),
                force_adaptive_thinking: None,
            },
        },
        ("together", "deepseek-ai/DeepSeek-V4-Pro") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Minimal, None),
                (ThinkingLevel::Low, None),
                (ThinkingLevel::Medium, None),
                (ThinkingLevel::High, Some("high")),
                (ThinkingLevel::XHigh, None),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: Some(true),
                thinking_format: Some("together"),
                force_adaptive_thinking: None,
            },
        },
        ("together", "essentialai/Rnj-1-Instruct") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: Some("together"),
                force_adaptive_thinking: None,
            },
        },
        ("together", "google/gemma-4-31B-it") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Minimal, None),
                (ThinkingLevel::Low, None),
                (ThinkingLevel::Medium, None),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: Some("together"),
                force_adaptive_thinking: None,
            },
        },
        ("together", "meta-llama/Llama-3.3-70B-Instruct-Turbo") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: Some("together"),
                force_adaptive_thinking: None,
            },
        },
        ("together", "moonshotai/Kimi-K2.6") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Minimal, None),
                (ThinkingLevel::Low, None),
                (ThinkingLevel::Medium, None),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: Some("together"),
                force_adaptive_thinking: None,
            },
        },
        ("together", "moonshotai/Kimi-K2.7-Code") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Minimal, None),
                (ThinkingLevel::Low, None),
                (ThinkingLevel::Medium, None),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: Some("together"),
                force_adaptive_thinking: None,
            },
        },
        ("together", "nvidia/nemotron-3-ultra-550b-a55b") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Minimal, None),
                (ThinkingLevel::Low, None),
                (ThinkingLevel::Medium, None),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: Some("together"),
                force_adaptive_thinking: None,
            },
        },
        ("together", "openai/gpt-oss-120b") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::Off, None), (ThinkingLevel::Minimal, None)],
            compat: ModelCompat {
                supports_reasoning_effort: Some(true),
                thinking_format: Some("openai"),
                force_adaptive_thinking: None,
            },
        },
        ("together", "openai/gpt-oss-20b") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::Off, None), (ThinkingLevel::Minimal, None)],
            compat: ModelCompat {
                supports_reasoning_effort: Some(true),
                thinking_format: Some("openai"),
                force_adaptive_thinking: None,
            },
        },
        ("together", "zai-org/GLM-5") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Minimal, None),
                (ThinkingLevel::Low, None),
                (ThinkingLevel::Medium, None),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: Some("together"),
                force_adaptive_thinking: None,
            },
        },
        ("together", "zai-org/GLM-5.1") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Minimal, None),
                (ThinkingLevel::Low, None),
                (ThinkingLevel::Medium, None),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: Some("together"),
                force_adaptive_thinking: None,
            },
        },
        ("vercel-ai-gateway", "anthropic/claude-opus-4.6") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("max"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: Some(true),
            },
        },
        ("vercel-ai-gateway", "anthropic/claude-opus-4.7") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("xhigh"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: Some(true),
            },
        },
        ("vercel-ai-gateway", "anthropic/claude-opus-4.8") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("xhigh"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: Some(true),
            },
        },
        ("vercel-ai-gateway", "anthropic/claude-sonnet-4.6") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: Some(true),
            },
        },
        ("vercel-ai-gateway", "openai/gpt-5.2") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("xhigh"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("vercel-ai-gateway", "openai/gpt-5.2-chat") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("xhigh"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("vercel-ai-gateway", "openai/gpt-5.2-codex") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("xhigh"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("vercel-ai-gateway", "openai/gpt-5.2-pro") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("xhigh"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("vercel-ai-gateway", "openai/gpt-5.3-chat") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("xhigh"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("vercel-ai-gateway", "openai/gpt-5.3-codex") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("xhigh"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("vercel-ai-gateway", "openai/gpt-5.4") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("xhigh"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("vercel-ai-gateway", "openai/gpt-5.4-mini") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("xhigh"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("vercel-ai-gateway", "openai/gpt-5.4-nano") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("xhigh"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("vercel-ai-gateway", "openai/gpt-5.4-pro") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("xhigh"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("vercel-ai-gateway", "openai/gpt-5.5") => ModelMetadata {
            thinking_level_map: &[(ThinkingLevel::XHigh, Some("xhigh"))],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("vercel-ai-gateway", "openai/gpt-5.5-pro") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::XHigh, Some("xhigh")),
                (ThinkingLevel::Off, None),
                (ThinkingLevel::Minimal, None),
                (ThinkingLevel::Low, None),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("xai", "grok-3") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("xai", "grok-3-fast") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("xai", "grok-4.20-0309-non-reasoning") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("xai", "grok-4.20-0309-reasoning") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("xai", "grok-4.3") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("xai", "grok-build-0.1") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("xai", "grok-code-fast-1") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: None,
                force_adaptive_thinking: None,
            },
        },
        ("xiaomi-token-plan-ams", "mimo-v2-omni") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("deepseek"),
                force_adaptive_thinking: None,
            },
        },
        ("xiaomi-token-plan-ams", "mimo-v2-pro") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("deepseek"),
                force_adaptive_thinking: None,
            },
        },
        ("xiaomi-token-plan-ams", "mimo-v2.5") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("deepseek"),
                force_adaptive_thinking: None,
            },
        },
        ("xiaomi-token-plan-ams", "mimo-v2.5-pro") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("deepseek"),
                force_adaptive_thinking: None,
            },
        },
        ("xiaomi-token-plan-ams", "mimo-v2.5-pro-ultraspeed") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("deepseek"),
                force_adaptive_thinking: None,
            },
        },
        ("xiaomi-token-plan-cn", "mimo-v2-omni") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("deepseek"),
                force_adaptive_thinking: None,
            },
        },
        ("xiaomi-token-plan-cn", "mimo-v2-pro") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("deepseek"),
                force_adaptive_thinking: None,
            },
        },
        ("xiaomi-token-plan-cn", "mimo-v2.5") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("deepseek"),
                force_adaptive_thinking: None,
            },
        },
        ("xiaomi-token-plan-cn", "mimo-v2.5-pro") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("deepseek"),
                force_adaptive_thinking: None,
            },
        },
        ("xiaomi-token-plan-cn", "mimo-v2.5-pro-ultraspeed") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("deepseek"),
                force_adaptive_thinking: None,
            },
        },
        ("xiaomi-token-plan-sgp", "mimo-v2-omni") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("deepseek"),
                force_adaptive_thinking: None,
            },
        },
        ("xiaomi-token-plan-sgp", "mimo-v2-pro") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("deepseek"),
                force_adaptive_thinking: None,
            },
        },
        ("xiaomi-token-plan-sgp", "mimo-v2.5") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("deepseek"),
                force_adaptive_thinking: None,
            },
        },
        ("xiaomi-token-plan-sgp", "mimo-v2.5-pro") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("deepseek"),
                force_adaptive_thinking: None,
            },
        },
        ("xiaomi-token-plan-sgp", "mimo-v2.5-pro-ultraspeed") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("deepseek"),
                force_adaptive_thinking: None,
            },
        },
        ("xiaomi", "mimo-v2-flash") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("deepseek"),
                force_adaptive_thinking: None,
            },
        },
        ("xiaomi", "mimo-v2-omni") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("deepseek"),
                force_adaptive_thinking: None,
            },
        },
        ("xiaomi", "mimo-v2-pro") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("deepseek"),
                force_adaptive_thinking: None,
            },
        },
        ("xiaomi", "mimo-v2.5") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("deepseek"),
                force_adaptive_thinking: None,
            },
        },
        ("xiaomi", "mimo-v2.5-pro") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("deepseek"),
                force_adaptive_thinking: None,
            },
        },
        ("xiaomi", "mimo-v2.5-pro-ultraspeed") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: None,
                thinking_format: Some("deepseek"),
                force_adaptive_thinking: None,
            },
        },
        ("zai-coding-cn", "glm-4.5")
        | ("zai-coding-cn", "glm-4.5-air")
        | ("zai-coding-cn", "glm-4.5-airx")
        | ("zai-coding-cn", "glm-4.5-flash")
        | ("zai-coding-cn", "glm-4.5-x")
        | ("zai-coding-cn", "glm-4.5v")
        | ("zai-coding-cn", "glm-4.6")
        | ("zai-coding-cn", "glm-4.6v")
        | ("zai-coding-cn", "glm-4.7") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: Some("zai"),
                force_adaptive_thinking: None,
            },
        },
        ("zai-coding-cn", "glm-5-turbo") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: Some("zai"),
                force_adaptive_thinking: None,
            },
        },
        ("zai-coding-cn", "glm-5.1") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: Some("zai"),
                force_adaptive_thinking: None,
            },
        },
        ("zai-coding-cn", "glm-5.2") | ("zai-coding-cn", "glm-5.2-fast") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Minimal, None),
                (ThinkingLevel::Low, Some("high")),
                (ThinkingLevel::Medium, Some("high")),
                (ThinkingLevel::High, Some("high")),
                (ThinkingLevel::XHigh, Some("max")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: Some(true),
                thinking_format: Some("zai"),
                force_adaptive_thinking: None,
            },
        },
        ("zai-coding-cn", "glm-5v-turbo") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: Some("zai"),
                force_adaptive_thinking: None,
            },
        },
        ("zai", "glm-4.5")
        | ("zai", "glm-4.5-air")
        | ("zai", "glm-4.5-airx")
        | ("zai", "glm-4.5-flash")
        | ("zai", "glm-4.5-x")
        | ("zai", "glm-4.5v")
        | ("zai", "glm-4.6")
        | ("zai", "glm-4.6v")
        | ("zai", "glm-4.7") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: Some("zai"),
                force_adaptive_thinking: None,
            },
        },
        ("zai", "glm-5-turbo") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: Some("zai"),
                force_adaptive_thinking: None,
            },
        },
        ("zai", "glm-5.1") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: Some("zai"),
                force_adaptive_thinking: None,
            },
        },
        ("zai", "glm-5.2") | ("zai", "glm-5.2-fast") => ModelMetadata {
            thinking_level_map: &[
                (ThinkingLevel::Minimal, None),
                (ThinkingLevel::Low, Some("high")),
                (ThinkingLevel::Medium, Some("high")),
                (ThinkingLevel::High, Some("high")),
                (ThinkingLevel::XHigh, Some("max")),
            ],
            compat: ModelCompat {
                supports_reasoning_effort: Some(true),
                thinking_format: Some("zai"),
                force_adaptive_thinking: None,
            },
        },
        ("zai", "glm-5v-turbo") => ModelMetadata {
            thinking_level_map: &[],
            compat: ModelCompat {
                supports_reasoning_effort: Some(false),
                thinking_format: Some("zai"),
                force_adaptive_thinking: None,
            },
        },
        _ => ModelMetadata::default(),
    }
}

pub fn mapped_thinking_value(
    model: &Model,
    level: ThinkingLevel,
    default: Option<&'static str>,
) -> Option<&'static str> {
    metadata_for(model)
        .thinking_level_map
        .iter()
        .find_map(|(mapped_level, value)| (*mapped_level == level).then_some(*value))
        .unwrap_or(default)
}

pub fn thinking_level_is_supported(model: &Model, level: ThinkingLevel) -> bool {
    if !model.reasoning {
        return level == ThinkingLevel::Off;
    }
    match metadata_for(model)
        .thinking_level_map
        .iter()
        .find_map(|(mapped_level, value)| (*mapped_level == level).then_some(*value))
    {
        Some(None) => false,
        Some(Some(_)) => true,
        None => level != ThinkingLevel::XHigh,
    }
}
