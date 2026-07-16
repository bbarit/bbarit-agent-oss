use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::Value;

use crate::config::{AppConfig, ModelsJson};

use super::catalog::{builtin_models, builtin_providers};
use super::types::{Model, Provider, ThinkingLevel};

#[derive(Debug, Clone)]
pub struct Registry {
    providers: BTreeMap<String, Provider>,
    models: Vec<Model>,
    provider_request_configs: BTreeMap<String, ProviderRequestConfig>,
    model_request_configs: BTreeMap<(String, String), ProviderRequestConfig>,
}

#[derive(Debug, Clone)]
pub struct ResolvedModel {
    pub model: Model,
    pub thinking: Option<ThinkingLevel>,
}

#[derive(Debug, Clone, Default)]
pub struct ProviderRequestConfig {
    pub headers: BTreeMap<String, String>,
    pub auth_header: Option<bool>,
}

impl ProviderRequestConfig {
    fn merge(&mut self, next: &ProviderRequestConfig) {
        if next.auth_header.is_some() {
            self.auth_header = next.auth_header;
        }
        for (key, value) in &next.headers {
            self.headers.insert(key.clone(), value.clone());
        }
    }
}

impl Registry {
    /// Per-million-token pricing for a built-in model, if known.
    pub fn cost_for(&self, provider: &str, id: &str) -> Option<crate::providers::types::ModelCost> {
        crate::providers::costs::builtin_cost(provider, id)
    }

    pub fn load(config: &AppConfig) -> Result<Self> {
        let mut registry = Self {
            providers: builtin_providers()
                .into_iter()
                .map(|provider| (provider.id.clone(), provider))
                .collect(),
            models: builtin_models(),
            provider_request_configs: BTreeMap::new(),
            model_request_configs: BTreeMap::new(),
        };
        registry.load_custom_models(config)?;
        registry.load_extension_provider_registrations(config)?;
        registry.load_ollama_runtime_models(config);
        registry.apply_oauth_model_overrides(config)?;
        Ok(registry)
    }

    pub fn providers(&self) -> impl Iterator<Item = &Provider> {
        self.providers.values()
    }

    pub fn models_for_provider(&self, provider: &str) -> Vec<&Model> {
        self.models
            .iter()
            .filter(|model| model.provider == provider)
            .collect()
    }

    pub fn search_models(&self, query: &str) -> Vec<&Model> {
        let query = query.to_lowercase();
        self.models
            .iter()
            .filter(|model| {
                let provider_name = self
                    .providers
                    .get(&model.provider)
                    .map(|provider| provider.name.as_str())
                    .unwrap_or("");
                query.is_empty()
                    || model.id.to_lowercase().contains(&query)
                    || model.name.to_lowercase().contains(&query)
                    || model.provider.to_lowercase().contains(&query)
                    || provider_name.to_lowercase().contains(&query)
            })
            .collect()
    }

    #[allow(dead_code)]
    pub fn resolve_model(&self, provider: &str, model: Option<&str>) -> Option<Model> {
        self.resolve_model_with_thinking(provider, model)
            .map(|resolved| resolved.model)
    }

    pub fn resolve_model_with_thinking(
        &self,
        provider: &str,
        model: Option<&str>,
    ) -> Option<ResolvedModel> {
        if let Some(pattern) = model {
            let (pattern_without_thinking, thinking) = split_thinking_suffix(pattern);
            if !pattern_without_thinking.contains('/') {
                if let Some(model) =
                    self.resolve_within_provider(provider, pattern_without_thinking)
                {
                    return Some(ResolvedModel { model, thinking });
                }
                let provider_pattern = format!("{provider}/{pattern}");
                if let Some(model) = self.resolve_reference_with_thinking(&provider_pattern) {
                    return Some(model);
                }
            } else if let Some(model) = self.resolve_reference_with_thinking(pattern) {
                return Some(model);
            }
        }
        self.default_model(provider).map(|model| ResolvedModel {
            model,
            thinking: None,
        })
    }

    fn resolve_within_provider(&self, provider: &str, pattern: &str) -> Option<Model> {
        let provider_models = self
            .models
            .iter()
            .filter(|model| model.provider == provider)
            .collect::<Vec<_>>();
        if pattern.contains('*') || pattern.contains('?') {
            return self.best_match(
                provider_models
                    .into_iter()
                    .filter(|model| wildcard_match(pattern, &model.id))
                    .collect(),
            );
        }
        let normalized = pattern.to_lowercase();
        if let Some(exact) = provider_models
            .iter()
            .find(|model| model.id.to_lowercase() == normalized)
        {
            return Some((*exact).clone().clone());
        }
        self.best_match(
            provider_models
                .into_iter()
                .filter(|model| {
                    model.id.to_lowercase().contains(&normalized)
                        || model.name.to_lowercase().contains(&normalized)
                })
                .collect(),
        )
    }

    #[allow(dead_code)]
    pub fn resolve_reference(&self, reference: &str) -> Option<Model> {
        self.resolve_reference_with_thinking(reference)
            .map(|resolved| resolved.model)
    }

    pub fn resolve_reference_with_thinking(&self, reference: &str) -> Option<ResolvedModel> {
        let (pattern, thinking) = split_thinking_suffix(reference);
        let pattern = pattern.trim();
        if pattern.is_empty() {
            return None;
        }

        if pattern.contains('*') || pattern.contains('?') {
            return self
                .best_match(
                    self.models
                        .iter()
                        .filter(|model| {
                            wildcard_match(pattern, &format!("{}/{}", model.provider, model.id))
                                || wildcard_match(pattern, &model.id)
                        })
                        .collect(),
                )
                .map(|model| ResolvedModel { model, thinking });
        }

        let normalized = pattern.to_lowercase();
        if let Some((provider, id)) = pattern.split_once('/') {
            let provider = provider.to_lowercase();
            let id = id.to_lowercase();
            if let Some(exact) = self.models.iter().find(|model| {
                model.provider.to_lowercase() == provider && model.id.to_lowercase() == id
            }) {
                return Some(ResolvedModel {
                    model: exact.clone(),
                    thinking,
                });
            }
        }

        let exact_id_matches = self
            .models
            .iter()
            .filter(|model| model.id.to_lowercase() == normalized)
            .collect::<Vec<_>>();
        if exact_id_matches.len() == 1 {
            return exact_id_matches.first().map(|model| ResolvedModel {
                model: (*model).clone().clone(),
                thinking,
            });
        }

        // A bare provider id means "that provider's default model" — the
        // lexical best_match below would pick whichever id sorts last
        // (gpt-5.6-terra beats gpt-5.6-sol) or even another provider whose
        // ids merely contain the name (bedrock "openai.gpt-*").
        if self.providers.contains_key(normalized.as_str())
            && let Some(model) = self.default_model(&normalized)
        {
            return Some(ResolvedModel { model, thinking });
        }

        self.best_match(
            self.models
                .iter()
                .filter(|model| {
                    model.id.to_lowercase().contains(&normalized)
                        || model.name.to_lowercase().contains(&normalized)
                        || format!("{}/{}", model.provider, model.id)
                            .to_lowercase()
                            .contains(&normalized)
                })
                .collect(),
        )
        .map(|model| ResolvedModel { model, thinking })
    }

    fn best_match(&self, matches: Vec<&Model>) -> Option<Model> {
        if matches.is_empty() {
            return None;
        }
        let mut aliases = matches
            .iter()
            .filter(|model| is_alias(&model.id))
            .copied()
            .collect::<Vec<_>>();
        if !aliases.is_empty() {
            aliases.sort_by(|a, b| b.id.cmp(&a.id));
            return aliases.first().map(|model| (*model).clone());
        }
        let mut dated = matches;
        dated.sort_by(|a, b| b.id.cmp(&a.id));
        dated.first().map(|model| (*model).clone())
    }

    pub fn default_model(&self, provider: &str) -> Option<Model> {
        let preferred: &[&str] = match provider {
            "amazon-bedrock" => &["us.anthropic.claude-opus-4-6-v1"],
            "ant-ling" => &["Ring-2.6-1T"],
            // Fable 5 first (Anthropic's strongest generally-available model);
            // Opus 4.8 as the fallback when the account/catalog lacks it.
            "anthropic" => &["claude-fable-5", "claude-opus-4-8"],
            "openai" => &["gpt-5.6-sol", "gpt-5.5"],
            "azure-openai-responses" => &["gpt-5.4"],
            "openai-codex" => &["gpt-5.6-sol", "gpt-5.5"],
            "ollama" => &[
                "gpt-oss:20b",
                "qwen2.5-coder:7b",
                "llama3.1:8b",
                "llama3.2:3b",
            ],
            "nvidia" => &["nvidia/nemotron-3-super-120b-a12b"],
            "dashscope" => &["qwen3-coder-plus", "qwen3-max"],
            "deepseek" => &["deepseek-v4-pro"],
            "google" | "google-vertex" => &["gemini-3.1-pro-preview"],
            "github-copilot" => &["gpt-5.4"],
            "openrouter" => &["moonshotai/kimi-k2.6"],
            "vercel-ai-gateway" => &["zai/glm-5.2-fast", "zai/glm-5.2"],
            "xai" => &["grok-4.20-0309-reasoning"],
            "groq" => &["openai/gpt-oss-120b"],
            "cerebras" => &["zai-glm-4.7"],
            "zai" | "zai-coding-cn" => &["glm-5.2-fast", "glm-5.2"],
            "mistral" => &["devstral-medium-latest"],
            "minimax" | "minimax-cn" => &["MiniMax-M2.7"],
            "moonshotai" | "moonshotai-cn" => &["kimi-k2.6"],
            "huggingface" => &["moonshotai/Kimi-K2.6"],
            "fireworks" => &["accounts/fireworks/models/kimi-k2p6"],
            "together" => &["moonshotai/Kimi-K2.6"],
            "opencode" | "opencode-go" => &["kimi-k2.6"],
            "kimi-coding" => &["kimi-for-coding"],
            "cloudflare-workers-ai" => &["@cf/moonshotai/kimi-k2.6"],
            "cloudflare-ai-gateway" => &["workers-ai/@cf/moonshotai/kimi-k2.6"],
            "xiaomi"
            | "xiaomi-token-plan-cn"
            | "xiaomi-token-plan-ams"
            | "xiaomi-token-plan-sgp" => &["mimo-v2.5-pro"],
            _ => &[],
        };
        for id in preferred {
            if let Some(model) = self
                .models
                .iter()
                .find(|model| model.provider == provider && model.id == *id)
            {
                return Some(model.clone());
            }
        }
        self.models
            .iter()
            .find(|model| model.provider == provider)
            .cloned()
    }

    pub fn provider(&self, provider: &str) -> Option<&Provider> {
        self.providers.get(provider)
    }

    pub fn request_config(&self, provider: &str, model_id: &str) -> ProviderRequestConfig {
        let mut config = self
            .provider_request_configs
            .get(provider)
            .cloned()
            .unwrap_or_default();
        if let Some(model_config) = self
            .model_request_configs
            .get(&(provider.to_string(), model_id.to_string()))
        {
            config.merge(model_config);
        }
        config
    }

    fn load_custom_models(&mut self, config: &AppConfig) -> Result<()> {
        for path in config.models_json_paths() {
            if !path.exists() {
                continue;
            }
            let text = fs::read_to_string(&path)
                .with_context(|| format!("failed to read {}", path.display()))?;
            let custom: ModelsJson = serde_json::from_str(text.trim_start_matches('\u{feff}'))
                .with_context(|| format!("failed to parse {}", path.display()))?;
            for (id, provider) in custom.providers {
                if let Some(existing) = self.providers.get_mut(&id) {
                    // Merge into a known provider: the /models refresh cache
                    // stores only model lists, so replacing the entry would
                    // wipe the builtin name/base_url/api_key_env.
                    if let Some(name) = provider.name.clone() {
                        existing.name = name;
                    }
                    if let Some(base_url) = provider.base_url.clone() {
                        existing.base_url = Some(base_url);
                    }
                    if let Some(api_key) = provider.api_key.clone() {
                        existing.api_key = Some(api_key);
                    }
                    if let Some(api_key_env) = provider.api_key_env.clone() {
                        existing.api_key_env = vec![api_key_env];
                    }
                } else {
                    let api_key_env = provider
                        .api_key_env
                        .clone()
                        .map(|env| vec![env])
                        .or_else(|| {
                            provider.api_key.as_ref().map(|_| {
                                vec![format!(
                                    "BBARIT_PROVIDER_{}_API_KEY",
                                    id.to_uppercase().replace('-', "_")
                                )]
                            })
                        })
                        .unwrap_or_default();
                    self.providers.insert(
                        id.clone(),
                        Provider {
                            id: id.clone(),
                            name: provider.name.clone().unwrap_or_else(|| id.clone()),
                            base_url: provider.base_url.clone(),
                            api_key: provider.api_key.clone(),
                            api_key_env,
                        },
                    );
                }
                // Cache docs carry no api/baseUrl either; inherit from the
                // provider's existing models (then the provider itself) so
                // e.g. cached anthropic models don't default to
                // openai-completions with no endpoint.
                let inherited = self
                    .models
                    .iter()
                    .find(|model| model.provider == id)
                    .map(|model| (model.api.clone(), model.base_url.clone()));
                let provider_api = provider
                    .api
                    .clone()
                    .or_else(|| inherited.as_ref().map(|(api, _)| api.clone()))
                    .unwrap_or_else(|| "openai-completions".to_string());
                let base_url = provider
                    .base_url
                    .clone()
                    .or_else(|| {
                        inherited
                            .as_ref()
                            .and_then(|(_, base_url)| base_url.clone())
                    })
                    .or_else(|| {
                        self.providers
                            .get(&id)
                            .and_then(|existing| existing.base_url.clone())
                    });
                for (model_id, override_value) in provider.model_overrides {
                    for model in self
                        .models
                        .iter_mut()
                        .filter(|model| model.provider == id && model.id == model_id)
                    {
                        apply_model_override(model, &override_value);
                    }
                }
                for model in provider.models {
                    // Don't duplicate a model already known (builtin or earlier
                    // file); custom files only *add* models they introduce.
                    if self
                        .models
                        .iter()
                        .any(|existing| existing.provider == id && existing.id == model.id)
                    {
                        continue;
                    }
                    self.models.push(Model {
                        provider: id.clone(),
                        id: model.id.clone(),
                        name: model.name.unwrap_or(model.id),
                        api: model.api.unwrap_or_else(|| provider_api.clone()),
                        base_url: model.base_url.or_else(|| base_url.clone()),
                        reasoning: false,
                        context_window: model.context_window,
                        max_tokens: model.max_tokens,
                    });
                }
            }
        }
        Ok(())
    }

    fn load_extension_provider_registrations(&mut self, config: &AppConfig) -> Result<()> {
        for change in crate::extensions::load_extension_provider_changes(config)? {
            match change {
                crate::extensions::ExtensionProviderChange::Unregister(provider_id) => {
                    self.providers.remove(&provider_id);
                    self.models.retain(|model| model.provider != provider_id);
                    self.provider_request_configs.remove(&provider_id);
                    self.model_request_configs
                        .retain(|(provider, _), _| provider != &provider_id);
                }
                crate::extensions::ExtensionProviderChange::Register(registration) => {
                    let provider_id = registration.provider.id.clone();
                    if let Some(existing) = self.providers.get_mut(&provider_id) {
                        existing.name = registration.provider.name.clone();
                        if registration.provider.base_url.is_some() {
                            existing.base_url = registration.provider.base_url.clone();
                        }
                        if registration.provider.api_key.is_some() {
                            existing.api_key = registration.provider.api_key.clone();
                        }
                        if !registration.provider.api_key_env.is_empty() {
                            existing.api_key_env = registration.provider.api_key_env.clone();
                        }
                    } else {
                        self.providers
                            .insert(provider_id.clone(), registration.provider.clone());
                    }

                    if registration.replace_models {
                        self.models.retain(|model| model.provider != provider_id);
                        self.model_request_configs
                            .retain(|(provider, _), _| provider != &provider_id);
                        self.models.extend(registration.models);
                    } else if registration.provider.base_url.is_some() {
                        for model in self
                            .models
                            .iter_mut()
                            .filter(|model| model.provider == provider_id)
                        {
                            model.base_url = registration.provider.base_url.clone();
                            if model.api.is_empty() {
                                model.api = registration.provider_api.clone();
                            }
                        }
                    }
                    self.provider_request_configs.insert(
                        provider_id.clone(),
                        ProviderRequestConfig {
                            headers: registration.headers.clone(),
                            auth_header: registration.auth_header,
                        },
                    );
                    for (model_id, model_config) in registration.model_request_configs {
                        self.model_request_configs
                            .insert((provider_id.clone(), model_id), model_config);
                    }
                }
            }
        }
        Ok(())
    }

    fn load_ollama_runtime_models(&mut self, config: &AppConfig) {
        let Some(root) = ollama_root_url(config) else {
            return;
        };
        // Discovery timeout: generous enough that a momentarily busy local Ollama
        // (mid-generation on another request, or a cold daemon) still lists its
        // models, instead of silently registering none. A refused connection
        // (Ollama not running) fails fast regardless, so this only costs time
        // when Ollama is up but slow.
        let Ok(client) = reqwest::blocking::Client::builder()
            .timeout(Duration::from_millis(3000))
            .build()
        else {
            return;
        };
        let Ok(response) = client
            .get(format!("{}/api/tags", root.trim_end_matches('/')))
            .send()
        else {
            return;
        };
        if !response.status().is_success() {
            return;
        }
        let Ok(body) = response.json::<Value>() else {
            return;
        };
        let base_url = format!("{}/v1", root.trim_end_matches('/'));
        let Some(models) = body["models"].as_array() else {
            return;
        };
        for item in models {
            let id = item["model"]
                .as_str()
                .or_else(|| item["name"].as_str())
                .unwrap_or_default()
                .trim();
            if id.is_empty()
                || self
                    .models
                    .iter()
                    .any(|model| model.provider == "ollama" && model.id == id)
            {
                continue;
            }
            self.models.push(Model {
                id: id.to_string(),
                name: format!("{} (Ollama)", id),
                api: "openai-completions".to_string(),
                provider: "ollama".to_string(),
                base_url: Some(base_url.clone()),
                reasoning: ollama_model_is_reasoning(id),
                context_window: None,
                max_tokens: Some(8192),
            });
        }
    }

    fn apply_oauth_model_overrides(&mut self, config: &AppConfig) -> Result<()> {
        let Some(copilot) = crate::auth::stored_github_copilot_model_config(config)? else {
            return Ok(());
        };
        if let Some(available_model_ids) = copilot.available_model_ids {
            let available = available_model_ids.into_iter().collect::<BTreeSet<_>>();
            if !available.is_empty() {
                self.models.retain(|model| {
                    model.provider != "github-copilot" || available.contains(&model.id)
                });
            }
        }
        for model in self
            .models
            .iter_mut()
            .filter(|model| model.provider == "github-copilot")
        {
            model.base_url = Some(copilot.base_url.clone());
        }
        Ok(())
    }
}

fn apply_model_override(model: &mut Model, override_value: &crate::config::ModelOverride) {
    if let Some(name) = &override_value.name {
        model.name = name.clone();
    }
    if let Some(api) = &override_value.api {
        model.api = api.clone();
    }
    if let Some(base_url) = &override_value.base_url {
        model.base_url = Some(base_url.clone());
    }
    if let Some(reasoning) = override_value.reasoning {
        model.reasoning = reasoning;
    }
    if let Some(context_window) = override_value.context_window {
        model.context_window = Some(context_window);
    }
    if let Some(max_tokens) = override_value.max_tokens {
        model.max_tokens = Some(max_tokens);
    }
}

pub fn split_thinking_suffix(pattern: &str) -> (&str, Option<ThinkingLevel>) {
    let Some((prefix, suffix)) = pattern.rsplit_once(':') else {
        return (pattern, None);
    };
    match ThinkingLevel::parse(suffix) {
        Ok(level) => (prefix, Some(level)),
        Err(_) => (pattern, None),
    }
}

fn is_alias(id: &str) -> bool {
    id.ends_with("-latest") || !id.chars().rev().take(8).all(|char| char.is_ascii_digit())
}

fn ollama_root_url(config: &AppConfig) -> Option<String> {
    let stored_env = crate::auth::stored_provider_env(config, "ollama").ok();
    let configured = stored_env
        .as_ref()
        .and_then(|env| env.get("OLLAMA_BASE_URL").cloned())
        .or_else(|| {
            stored_env
                .as_ref()
                .and_then(|env| env.get("OLLAMA_HOST").cloned())
        })
        .or_else(|| std::env::var("OLLAMA_BASE_URL").ok())
        .or_else(|| std::env::var("OLLAMA_HOST").ok())
        .unwrap_or_else(|| "http://localhost:11434".to_string());
    let mut root = configured.trim().trim_end_matches('/').to_string();
    if root.is_empty() {
        return None;
    }
    if !root.contains("://") {
        root = format!("http://{root}");
    }
    if let Some(stripped) = root.strip_suffix("/v1") {
        root = stripped.to_string();
    }
    Some(root)
}

fn ollama_model_is_reasoning(id: &str) -> bool {
    let id = id.to_lowercase();
    id.contains("gpt-oss")
        || id.contains("deepseek-r1")
        || id.contains("qwen3")
        || id.contains("qwq")
        || id.contains("reason")
}

fn wildcard_match(pattern: &str, value: &str) -> bool {
    wildcard_match_inner(
        pattern.to_lowercase().as_bytes(),
        value.to_lowercase().as_bytes(),
    )
}

fn wildcard_match_inner(pattern: &[u8], value: &[u8]) -> bool {
    if pattern.is_empty() {
        return value.is_empty();
    }
    match pattern[0] {
        b'*' => {
            wildcard_match_inner(&pattern[1..], value)
                || (!value.is_empty() && wildcard_match_inner(pattern, &value[1..]))
        }
        b'?' => !value.is_empty() && wildcard_match_inner(&pattern[1..], &value[1..]),
        char => {
            !value.is_empty()
                && char == value[0]
                && wildcard_match_inner(&pattern[1..], &value[1..])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;

    #[test]
    fn kimi_coding_provider_models_and_costs_are_wired() {
        let config = AppConfig::for_test(std::env::temp_dir().join("bbarit-kimi-registry"));
        let registry = Registry::load(&config).unwrap();

        let provider = registry
            .provider("kimi-coding")
            .expect("kimi-coding provider should be built in");
        assert_eq!(provider.name, "Kimi For Coding");
        assert_eq!(provider.api_key_env, vec!["KIMI_API_KEY".to_string()]);

        let models = registry.models_for_provider("kimi-coding");
        assert!(!models.is_empty(), "kimi-coding should ship models");
        for model in &models {
            assert_eq!(model.api, "anthropic-messages");
            assert!(
                registry.cost_for("kimi-coding", &model.id).is_some(),
                "missing cost entry for kimi-coding/{}",
                model.id
            );
        }

        // Picking the provider after /login kimi-coding must resolve a model.
        let default = registry
            .default_model("kimi-coding")
            .expect("kimi-coding should have a default model");
        assert_eq!(default.id, "kimi-for-coding");
    }

    #[test]
    fn bare_provider_reference_resolves_to_provider_default() {
        let config = AppConfig::for_test(std::env::temp_dir().join("bbarit-registry-provider-ref"));
        let registry = Registry::load(&config).unwrap();

        let resolved = registry
            .resolve_reference("openai-codex")
            .expect("bare provider name should resolve to its default model");
        assert_eq!(resolved.provider, "openai-codex");
        assert_eq!(resolved.id, "gpt-5.6-sol");

        // Explicit tier selection must keep winning over the provider default.
        let terra = registry
            .resolve_reference("openai-codex/gpt-5.6-terra")
            .expect("explicit tier reference should resolve");
        assert_eq!(terra.id, "gpt-5.6-terra");
    }

    #[test]
    fn models_dev_cache_merges_into_builtin_provider() {
        let dir = std::env::temp_dir().join("bbarit-registry-models-dev-merge");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let config = AppConfig::for_test(dir.clone());
        // /models refresh cache doc: model list only, no identity fields.
        fs::write(
            config.models_dev_cache_path(),
            r#"{"providers":{"anthropic":{"models":[{"id":"claude-cache-only"}]}}}"#,
        )
        .unwrap();

        let registry = Registry::load(&config).unwrap();

        let provider = registry
            .provider("anthropic")
            .expect("anthropic provider should survive the cache doc");
        assert_eq!(provider.name, "Anthropic");
        assert!(
            provider
                .api_key_env
                .contains(&"ANTHROPIC_API_KEY".to_string()),
            "builtin api_key_env lost: {:?}",
            provider.api_key_env
        );

        let model = registry
            .models_for_provider("anthropic")
            .into_iter()
            .find(|model| model.id == "claude-cache-only")
            .expect("cached model should be registered")
            .clone();
        assert_eq!(model.api, "anthropic-messages");
        assert_eq!(model.base_url.as_deref(), Some("https://api.anthropic.com"));

        let _ = fs::remove_dir_all(dir);
    }
}
