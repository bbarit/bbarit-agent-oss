use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub api_key_env: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Model {
    pub id: String,
    pub name: String,
    pub api: String,
    pub provider: String,
    pub base_url: Option<String>,
    pub reasoning: bool,
    pub context_window: Option<u32>,
    pub max_tokens: Option<u32>,
}

/// Per-million-token USD pricing for a model.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ModelCost {
    pub input: f64,
    pub output: f64,
    #[serde(default, rename = "cacheRead")]
    pub cache_read: f64,
    #[serde(default, rename = "cacheWrite")]
    pub cache_write: f64,
}

impl ModelCost {
    /// USD cost for the given token counts (token counts are absolute, pricing
    /// is per million tokens).
    pub fn cost_for(
        &self,
        input: usize,
        output: usize,
        cache_read: usize,
        cache_write: usize,
    ) -> f64 {
        let per_million = |tokens: usize, price: f64| (tokens as f64) * price / 1_000_000.0;
        per_million(input, self.input)
            + per_million(output, self.output)
            + per_million(cache_read, self.cache_read)
            + per_million(cache_write, self.cache_write)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThinkingLevel {
    Off,
    Minimal,
    Low,
    Medium,
    High,
    XHigh,
}

impl ThinkingLevel {
    pub fn parse(value: &str) -> Result<Self> {
        match value.trim().to_lowercase().as_str() {
            "off" | "none" => Ok(Self::Off),
            "minimal" | "min" => Ok(Self::Minimal),
            "low" => Ok(Self::Low),
            "medium" | "med" => Ok(Self::Medium),
            "high" => Ok(Self::High),
            "xhigh" | "x-high" | "max" => Ok(Self::XHigh),
            other => bail!(
                "invalid thinking level '{other}'. Use off, minimal, low, medium, high, or xhigh"
            ),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Minimal => "minimal",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::XHigh => "xhigh",
        }
    }

    pub fn is_enabled(self) -> bool {
        self != Self::Off
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiKind {
    OpenAiResponses,
    AzureOpenAiResponses,
    OpenAiCodexResponses,
    OpenAiCompletions,
    AnthropicMessages,
    GoogleGenerativeAi,
    MistralConversations,
    BedrockConverse,
    GoogleVertex,
    Unsupported,
}

impl ApiKind {
    pub fn from_api(api: &str) -> Self {
        match api {
            "openai-responses" => Self::OpenAiResponses,
            "azure-openai-responses" => Self::AzureOpenAiResponses,
            "openai-codex-responses" => Self::OpenAiCodexResponses,
            "openai-completions" => Self::OpenAiCompletions,
            "anthropic-messages" => Self::AnthropicMessages,
            "google-generative-ai" => Self::GoogleGenerativeAi,
            "mistral-conversations" => Self::MistralConversations,
            "bedrock-converse-stream" => Self::BedrockConverse,
            "google-vertex" => Self::GoogleVertex,
            _ => Self::Unsupported,
        }
    }
}
