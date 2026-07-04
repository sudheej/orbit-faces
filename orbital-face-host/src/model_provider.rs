use crate::speech::SpeechOptions;
use crate::tools::ToolSuggestion;
use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::time::{Duration, Instant};

pub const DEFAULT_OLLAMA_BASE_URL: &str = "http://localhost:11434";
pub const DEFAULT_OLLAMA_MODEL: &str = "qwen2.5:1.5b";
pub const DEFAULT_SYSTEM_PROMPT: &str = "You are an ongoing personal companion. No fixed personality, species, or role is imposed; develop a consistent, natural relationship with the user from the configured identity and your shared history. Speak conversationally and keep most replies to 1-3 sentences unless the user asks for detail.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeModelOptions {
    pub provider: String,
    pub ollama_model: String,
    pub ollama_base_url: String,
    pub system_prompt: String,
    pub enable_hotkeys: bool,
    pub enable_model_tools: bool,
    pub enable_model_thinking: bool,
    pub db_path: Option<std::path::PathBuf>,
    pub store_session_messages: bool,
    pub store_context_items: bool,
    pub speech: SpeechOptions,
}

impl Default for RuntimeModelOptions {
    fn default() -> Self {
        Self {
            provider: "mock".into(),
            ollama_model: DEFAULT_OLLAMA_MODEL.into(),
            ollama_base_url: DEFAULT_OLLAMA_BASE_URL.into(),
            system_prompt: DEFAULT_SYSTEM_PROMPT.into(),
            enable_hotkeys: false,
            enable_model_tools: false,
            enable_model_thinking: false,
            db_path: None,
            store_session_messages: false,
            store_context_items: false,
            speech: SpeechOptions::default(),
        }
    }
}

impl RuntimeModelOptions {
    pub fn parse_from<I, S>(args: I) -> anyhow::Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut options = Self::default();
        let mut args = args.into_iter().map(Into::into);
        while let Some(argument) = args.next() {
            match argument.as_str() {
                "--model" => {
                    options.provider = args
                        .next()
                        .ok_or_else(|| anyhow::anyhow!("--model requires a provider name"))?;
                }
                "--ollama-model" => {
                    options.ollama_model = args
                        .next()
                        .ok_or_else(|| anyhow::anyhow!("--ollama-model requires a model name"))?;
                }
                "--ollama-base-url" => {
                    options.ollama_base_url = args
                        .next()
                        .ok_or_else(|| anyhow::anyhow!("--ollama-base-url requires a URL"))?;
                }
                "--system-prompt" => {
                    options.system_prompt = args
                        .next()
                        .ok_or_else(|| anyhow::anyhow!("--system-prompt requires text"))?;
                }
                "--enable-hotkeys" => options.enable_hotkeys = true,
                "--enable-model-tools" => options.enable_model_tools = true,
                "--enable-model-thinking" => options.enable_model_thinking = true,
                "--db-path" => {
                    options.db_path = Some(
                        args.next()
                            .map(Into::into)
                            .ok_or_else(|| anyhow::anyhow!("--db-path requires a path"))?,
                    );
                }
                "--store-session-messages" => options.store_session_messages = true,
                "--store-context-items" => options.store_context_items = true,
                "--stt" => {
                    options.speech.stt = args
                        .next()
                        .ok_or_else(|| anyhow::anyhow!("--stt requires mock or whisper"))?;
                }
                "--whisper-model-path" => {
                    options.speech.whisper_model_path =
                        Some(args.next().map(Into::into).ok_or_else(|| {
                            anyhow::anyhow!("--whisper-model-path requires a path")
                        })?);
                }
                "--whisper-bin" => {
                    options.speech.whisper_bin = args
                        .next()
                        .map(Into::into)
                        .ok_or_else(|| anyhow::anyhow!("--whisper-bin requires a path"))?;
                }
                "--tts" => {
                    options.speech.tts = args.next().ok_or_else(|| {
                        anyhow::anyhow!("--tts requires none, piper, or windows-sapi")
                    })?;
                }
                "--piper-bin" => {
                    options.speech.piper_bin = Some(
                        args.next()
                            .map(Into::into)
                            .ok_or_else(|| anyhow::anyhow!("--piper-bin requires a path"))?,
                    );
                }
                "--piper-model" => {
                    options.speech.piper_model = Some(
                        args.next()
                            .map(Into::into)
                            .ok_or_else(|| anyhow::anyhow!("--piper-model requires a path"))?,
                    );
                }
                "--piper-config" => {
                    options.speech.piper_config = Some(
                        args.next()
                            .map(Into::into)
                            .ok_or_else(|| anyhow::anyhow!("--piper-config requires a path"))?,
                    );
                }
                "--speak-responses" => options.speech.speak_responses = true,
                _ => anyhow::bail!("unknown argument {argument:?}; use --help for usage"),
            }
        }
        anyhow::ensure!(
            matches!(options.provider.as_str(), "mock" | "ollama"),
            "unknown model provider {:?}; available: mock, ollama",
            options.provider
        );
        anyhow::ensure!(
            matches!(options.speech.stt.as_str(), "mock" | "whisper"),
            "unknown STT provider {:?}; available: mock, whisper",
            options.speech.stt
        );
        anyhow::ensure!(
            matches!(
                options.speech.tts.as_str(),
                "none" | "piper" | "windows-sapi"
            ),
            "unknown TTS provider {:?}; available: none, piper, windows-sapi",
            options.speech.tts
        );
        Ok(options)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelRequest {
    pub user_input: String,
    pub prompt_context: Option<String>,
    pub context_item_count: usize,
    pub system_prompt: Option<String>,
    pub conversation_id: Option<String>,
    pub history: Vec<ModelMessage>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelResponse {
    pub text: String,
    pub provider: String,
    pub model: String,
    pub elapsed_ms: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelChunk {
    pub text_delta: String,
    pub done: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolPlanningRequest {
    pub user_input: String,
    pub tool_names: Vec<String>,
    pub tool_schemas: Vec<(String, Value)>,
    pub tool_descriptions: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderStatus {
    pub reachable: bool,
    pub model_available: Option<bool>,
    pub detail: String,
}

pub trait ModelProvider {
    fn name(&self) -> &'static str;
    fn model_name(&self) -> &str;
    fn base_url(&self) -> Option<&str> {
        None
    }
    fn status(&self) -> ProviderStatus;
    fn suggest_tool(
        &self,
        _request: &ToolPlanningRequest,
    ) -> anyhow::Result<Option<ToolSuggestion>> {
        Ok(None)
    }
    fn generate(&self, request: ModelRequest) -> anyhow::Result<ModelResponse> {
        self.generate_streaming(request, &mut |_| {})
    }
    fn generate_streaming(
        &self,
        request: ModelRequest,
        on_chunk: &mut dyn FnMut(ModelChunk),
    ) -> anyhow::Result<ModelResponse>;
}

#[derive(Debug, Default)]
pub struct MockModelProvider;

impl ModelProvider for MockModelProvider {
    fn name(&self) -> &'static str {
        "mock"
    }

    fn model_name(&self) -> &str {
        "orbital-mock-v0"
    }

    fn status(&self) -> ProviderStatus {
        ProviderStatus {
            reachable: true,
            model_available: Some(true),
            detail: "deterministic in-process provider".into(),
        }
    }

    fn suggest_tool(
        &self,
        request: &ToolPlanningRequest,
    ) -> anyhow::Result<Option<ToolSuggestion>> {
        let normalized = request.user_input.to_ascii_lowercase();
        if normalized.contains("search memory") {
            let query = request
                .user_input
                .split_once("for ")
                .map(|(_, query)| query)
                .map(|query| query.split_once(" and ").map_or(query, |(query, _)| query))
                .map(|query| query.trim().trim_end_matches(['.', '?']))
                .filter(|query| !query.is_empty())
                .unwrap_or(request.user_input.trim());
            return Ok(Some(ToolSuggestion {
                tool: "memory.search".into(),
                arguments: serde_json::json!({"query":query}),
            }));
        }
        Ok(None)
    }

    fn generate_streaming(
        &self,
        request: ModelRequest,
        on_chunk: &mut dyn FnMut(ModelChunk),
    ) -> anyhow::Result<ModelResponse> {
        let started = Instant::now();
        let normalized = request.user_input.to_ascii_lowercase();
        let context_count = request.context_item_count;
        let selected_count = request
            .prompt_context
            .as_deref()
            .map(|context| context.matches("Type: selected_text").count())
            .unwrap_or(0);
        let text = if selected_count > 0 {
            let preview = selected_text_preview(request.prompt_context.as_deref().unwrap_or(""));
            format!(
                "I used {selected_count} selected_text context item(s). The selected text appears to be related to: {preview}"
            )
        } else if context_count > 0 {
            format!(
                "I used {context_count} attached context item(s) to answer: {}",
                request.user_input
            )
        } else if normalized.contains("summarize this") || normalized.contains("explain this") {
            "I do not have context yet. Use /clipboard, /attach-text, or /attach-file.".to_owned()
        } else if normalized.contains("error") {
            "I need the error text, but I can help once you paste the log.".to_owned()
        } else if normalized.starts_with("hello") || normalized.starts_with("hi") {
            "Hello. The local Orbital runtime is connected and ready.".to_owned()
        } else if normalized.contains("status") {
            "The runtime is active. Use slash status for bridge details.".to_owned()
        } else {
            "I received your message. This mock provider proves the runtime flow without an external model.".to_owned()
        };

        for delta in word_chunks(&text, 4) {
            on_chunk(ModelChunk {
                text_delta: delta,
                done: false,
            });
        }
        on_chunk(ModelChunk {
            text_delta: String::new(),
            done: true,
        });

        Ok(ModelResponse {
            text,
            provider: self.name().into(),
            model: self.model_name().into(),
            elapsed_ms: started.elapsed().as_millis() as u64,
            error: None,
        })
    }
}

fn selected_text_preview(context: &str) -> String {
    let content = context
        .split("Type: selected_text")
        .nth(1)
        .and_then(|section| section.split("Content:\n").nth(1))
        .unwrap_or("selected content");
    let preview = content
        .split_whitespace()
        .take(8)
        .collect::<Vec<_>>()
        .join(" ");
    if preview.is_empty() {
        "selected content".into()
    } else {
        preview
    }
}

#[derive(Debug, Clone)]
pub struct OllamaModelProvider {
    base_url: String,
    model: String,
    agent: ureq::Agent,
    health_agent: ureq::Agent,
    enable_thinking: bool,
}

impl OllamaModelProvider {
    pub fn new(base_url: impl Into<String>, model: impl Into<String>) -> anyhow::Result<Self> {
        let base_url = base_url.into().trim_end_matches('/').to_owned();
        anyhow::ensure!(
            base_url.starts_with("http://"),
            "Ollama Local Model Provider v0 requires an http:// base URL"
        );
        let model = model.into();
        anyhow::ensure!(!model.trim().is_empty(), "Ollama model must not be empty");
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(300)))
            .build()
            .into();
        let health_agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(2)))
            .build()
            .into();
        Ok(Self {
            base_url,
            model,
            agent,
            health_agent,
            enable_thinking: false,
        })
    }

    pub fn with_thinking(mut self, enabled: bool) -> Self {
        self.enable_thinking = enabled;
        self
    }

    fn endpoint(&self, path: &str) -> String {
        format!("{}/api/{path}", self.base_url)
    }
}

impl ModelProvider for OllamaModelProvider {
    fn name(&self) -> &'static str {
        "ollama"
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    fn base_url(&self) -> Option<&str> {
        Some(&self.base_url)
    }

    fn status(&self) -> ProviderStatus {
        let result = self.health_agent.get(self.endpoint("tags")).call();
        match result {
            Ok(mut response) => {
                let tags = response.body_mut().read_json::<OllamaTagsResponse>();
                match tags {
                    Ok(tags) => {
                        let available = tags.models.iter().any(|model| {
                            model.name == self.model || model.model.as_deref() == Some(&self.model)
                        });
                        ProviderStatus {
                            reachable: true,
                            model_available: Some(available),
                            detail: if available {
                                "Ollama is reachable and the model is installed".into()
                            } else {
                                format!(
                                    "Ollama is reachable but model {:?} was not listed",
                                    self.model
                                )
                            },
                        }
                    }
                    Err(error) => ProviderStatus {
                        reachable: true,
                        model_available: None,
                        detail: format!("Ollama replied but model list was invalid: {error}"),
                    },
                }
            }
            Err(error) => ProviderStatus {
                reachable: false,
                model_available: None,
                detail: format!("Ollama is not reachable at {}: {error}", self.base_url),
            },
        }
    }

    fn suggest_tool(
        &self,
        request: &ToolPlanningRequest,
    ) -> anyhow::Result<Option<ToolSuggestion>> {
        let planner_prompt = "Call at most one tool only when it must run to satisfy the request. Do not call tools for conversation, capability questions, tool-history questions, unclear requests, or unsupported actions. Never claim execution. Tool results require runtime approval."
            .to_owned();
        let messages = vec![
            ModelMessage {
                role: "system".into(),
                content: planner_prompt,
            },
            ModelMessage {
                role: "user".into(),
                content: request.user_input.clone(),
            },
        ];
        let tools = request
            .tool_schemas
            .iter()
            .map(|(name, parameters)| OllamaTool {
                r#type: "function",
                function: OllamaToolFunction {
                    name: name.clone(),
                    description: request
                        .tool_descriptions
                        .iter()
                        .find(|(candidate, _)| candidate == name)
                        .map(|(_, description)| description.clone())
                        .unwrap_or_else(|| name.clone()),
                    parameters: parameters.clone(),
                },
            })
            .collect::<Vec<_>>();
        let body = OllamaToolChatRequest {
            model: &self.model,
            messages: &messages,
            tools: &tools,
            stream: false,
            think: false,
            options: OllamaOptions {
                num_predict: Some(128),
                temperature: Some(0.0),
            },
        };
        let mut response = self
            .agent
            .post(self.endpoint("chat"))
            .send_json(&body)
            .context("Ollama tool planner request failed")?;
        let response = response
            .body_mut()
            .read_json::<OllamaChatChunk>()
            .context("invalid Ollama tool planner response")?;
        if let Some(error) = response.error {
            anyhow::bail!("Ollama tool planner error: {error}");
        }
        if response.message.tool_calls.is_empty() {
            return Ok(None);
        }
        anyhow::ensure!(
            response.message.tool_calls.len() == 1,
            "Ollama proposed more than one tool; v0 allows one"
        );
        let call = &response.message.tool_calls[0].function;
        anyhow::ensure!(
            request.tool_names.iter().any(|name| name == &call.name),
            "Ollama tool planner selected an unknown tool"
        );
        Ok(Some(ToolSuggestion {
            tool: call.name.clone(),
            arguments: call.arguments.clone(),
        }))
    }

    fn generate_streaming(
        &self,
        request: ModelRequest,
        on_chunk: &mut dyn FnMut(ModelChunk),
    ) -> anyhow::Result<ModelResponse> {
        let started = Instant::now();
        let user_content = assemble_user_content(&request);
        let mut messages = Vec::new();
        if let Some(system_prompt) = request.system_prompt.filter(|prompt| !prompt.is_empty()) {
            messages.push(ModelMessage {
                role: "system".into(),
                content: system_prompt,
            });
        }
        messages.extend(request.history);
        messages.push(ModelMessage {
            role: "user".into(),
            content: user_content,
        });

        let body = OllamaChatRequest {
            model: &self.model,
            messages: &messages,
            stream: true,
            think: self.enable_thinking,
            options: OllamaOptions {
                num_predict: request.max_tokens,
                temperature: request.temperature,
            },
        };
        let response = self
            .agent
            .post(self.endpoint("chat"))
            .send_json(&body)
            .with_context(|| {
                format!(
                    "Ollama request failed at {}; start Ollama and run `ollama pull {}`",
                    self.base_url, self.model
                )
            })?;

        let mut full_text = String::new();
        let reader = BufReader::new(response.into_body().into_reader());
        for line in reader.lines() {
            let line = line.context("failed reading Ollama streaming response")?;
            if line.trim().is_empty() {
                continue;
            }
            let chunk: OllamaChatChunk =
                serde_json::from_str(&line).context("invalid Ollama streaming response")?;
            if let Some(error) = chunk.error {
                anyhow::bail!("Ollama model error: {error}");
            }
            if !chunk.message.content.is_empty() {
                full_text.push_str(&chunk.message.content);
                on_chunk(ModelChunk {
                    text_delta: chunk.message.content,
                    done: false,
                });
            }
            if chunk.done {
                on_chunk(ModelChunk {
                    text_delta: String::new(),
                    done: true,
                });
            }
        }
        anyhow::ensure!(
            !full_text.trim().is_empty(),
            "Ollama returned an empty response"
        );

        Ok(ModelResponse {
            text: full_text,
            provider: self.name().into(),
            model: self.model.clone(),
            elapsed_ms: started.elapsed().as_millis() as u64,
            error: None,
        })
    }
}

pub fn assemble_user_content(request: &ModelRequest) -> String {
    match request
        .prompt_context
        .as_deref()
        .filter(|context| !context.trim().is_empty())
    {
        Some(context) => format!("{context}\n[User Request]\n{}", request.user_input),
        None => request.user_input.clone(),
    }
}

fn word_chunks(text: &str, words_per_chunk: usize) -> Vec<String> {
    let words: Vec<_> = text.split_whitespace().collect();
    words
        .chunks(words_per_chunk)
        .map(|chunk| format!("{} ", chunk.join(" ")))
        .collect()
}

#[derive(Debug, Serialize)]
struct OllamaChatRequest<'a> {
    model: &'a str,
    messages: &'a [ModelMessage],
    stream: bool,
    think: bool,
    options: OllamaOptions,
}

#[derive(Debug, Serialize)]
struct OllamaToolChatRequest<'a> {
    model: &'a str,
    messages: &'a [ModelMessage],
    tools: &'a [OllamaTool],
    stream: bool,
    think: bool,
    options: OllamaOptions,
}

#[derive(Debug, Serialize)]
struct OllamaTool {
    r#type: &'static str,
    function: OllamaToolFunction,
}

#[derive(Debug, Serialize)]
struct OllamaToolFunction {
    name: String,
    description: String,
    parameters: Value,
}

#[derive(Debug, Serialize)]
struct OllamaOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    num_predict: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
}

#[derive(Debug, Deserialize)]
struct OllamaChatChunk {
    #[serde(default)]
    message: OllamaMessage,
    #[serde(default)]
    done: bool,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct OllamaMessage {
    #[serde(default)]
    content: String,
    #[serde(default)]
    tool_calls: Vec<OllamaToolCall>,
}

#[derive(Debug, Deserialize)]
struct OllamaToolCall {
    function: OllamaToolCallFunction,
}

#[derive(Debug, Deserialize)]
struct OllamaToolCallFunction {
    name: String,
    #[serde(default)]
    arguments: Value,
}

#[derive(Debug, Deserialize)]
struct OllamaTagsResponse {
    #[serde(default)]
    models: Vec<OllamaTag>,
}

#[derive(Debug, Deserialize)]
struct OllamaTag {
    name: String,
    #[serde(default)]
    model: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(input: &str) -> ModelRequest {
        ModelRequest {
            user_input: input.into(),
            prompt_context: None,
            context_item_count: 0,
            system_prompt: Some(DEFAULT_SYSTEM_PROMPT.into()),
            conversation_id: None,
            history: Vec::new(),
            max_tokens: Some(128),
            temperature: Some(0.2),
        }
    }

    #[test]
    fn model_types_round_trip_as_json() {
        let request = request("hello");
        let json = serde_json::to_string(&request).unwrap();
        assert_eq!(
            serde_json::from_str::<ModelRequest>(&json).unwrap(),
            request
        );

        let response = ModelResponse {
            text: "hi".into(),
            provider: "mock".into(),
            model: "orbital-mock-v0".into(),
            elapsed_ms: 1,
            error: None,
        };
        let json = serde_json::to_string(&response).unwrap();
        assert_eq!(
            serde_json::from_str::<ModelResponse>(&json).unwrap(),
            response
        );
    }

    #[test]
    fn ollama_tool_channel_keeps_calls_out_of_content() {
        let messages = vec![ModelMessage {
            role: "user".into(),
            content: "What time is it?".into(),
        }];
        let tools = vec![OllamaTool {
            r#type: "function",
            function: OllamaToolFunction {
                name: "time.now".into(),
                description: "Return local time".into(),
                parameters: serde_json::json!({"type":"object"}),
            },
        }];
        let request = OllamaToolChatRequest {
            model: "test",
            messages: &messages,
            tools: &tools,
            stream: false,
            think: false,
            options: OllamaOptions {
                num_predict: Some(128),
                temperature: Some(0.0),
            },
        };
        let json = serde_json::to_value(request).unwrap();
        assert_eq!(json["tools"][0]["function"]["name"], "time.now");

        let response: OllamaChatChunk = serde_json::from_value(serde_json::json!({
            "message": {"content":"", "tool_calls":[{"function":{"name":"time.now","arguments":{}}}]},
            "done": true
        }))
        .unwrap();
        assert!(response.message.content.is_empty());
        assert_eq!(response.message.tool_calls[0].function.name, "time.now");
    }

    #[test]
    fn mock_generation_reports_provider_and_model() {
        let response = MockModelProvider.generate(request("hello")).unwrap();
        assert_eq!(response.provider, "mock");
        assert_eq!(response.model, "orbital-mock-v0");
        assert!(response.error.is_none());
    }

    #[test]
    fn mock_streaming_reconstructs_response() {
        let mut streamed = String::new();
        let response = MockModelProvider
            .generate_streaming(request("hello"), &mut |chunk| {
                streamed.push_str(&chunk.text_delta)
            })
            .unwrap();
        assert_eq!(streamed.trim(), response.text);
    }

    #[test]
    fn mock_provider_reports_attached_context() {
        let mut request = request("explain this");
        request.prompt_context =
            Some("[Orbital Context]\nContext item 1:\nType: attached_text\nContent:\nerror".into());
        request.context_item_count = 1;
        let response = MockModelProvider.generate(request).unwrap();
        assert!(response.text.contains("1 attached context item"));
    }

    #[test]
    fn model_request_assembles_context_before_user_request() {
        let mut request = request("summarize this");
        request.prompt_context = Some("[Orbital Context]\nexample".into());
        let assembled = assemble_user_content(&request);
        assert!(assembled.starts_with("[Orbital Context]"));
        assert!(assembled.ends_with("[User Request]\nsummarize this"));
    }

    #[test]
    fn default_ollama_model_is_qwen_1_5b() {
        assert_eq!(DEFAULT_OLLAMA_MODEL, "qwen2.5:1.5b");
    }

    #[test]
    fn ollama_config_normalizes_trailing_slash() {
        let provider = OllamaModelProvider::new("http://localhost:11434/", "qwen2.5:1.5b").unwrap();
        assert_eq!(provider.base_url(), Some("http://localhost:11434"));
        assert_eq!(provider.model_name(), "qwen2.5:1.5b");
    }

    #[test]
    fn unavailable_ollama_is_reported_without_panicking() {
        let provider = OllamaModelProvider::new("http://127.0.0.1:1", "missing").unwrap();
        let status = provider.status();
        assert!(!status.reachable);
    }

    #[test]
    fn cli_defaults_to_mock_and_recommended_ollama_model() {
        let options = RuntimeModelOptions::parse_from(Vec::<String>::new()).unwrap();
        assert_eq!(options.provider, "mock");
        assert_eq!(options.ollama_model, "qwen2.5:1.5b");
        assert!(!options.enable_hotkeys);
        assert!(!options.enable_model_tools);
        assert!(!options.enable_model_thinking);
        assert!(options.db_path.is_none());
        assert!(!options.store_session_messages);
        assert_eq!(options.speech.stt, "mock");
        assert_eq!(options.speech.tts, "none");
    }

    #[test]
    fn cli_parses_ollama_provider_configuration() {
        let options = RuntimeModelOptions::parse_from([
            "--model",
            "ollama",
            "--ollama-model",
            "qwen2.5-coder:1.5b",
            "--ollama-base-url",
            "http://127.0.0.1:11434",
            "--system-prompt",
            "Be brief.",
            "--enable-hotkeys",
            "--enable-model-tools",
            "--enable-model-thinking",
            "--db-path",
            "./orbital.db",
            "--store-session-messages",
            "--store-context-items",
            "--stt",
            "whisper",
            "--whisper-model-path",
            "./tiny.bin",
            "--tts",
            "piper",
            "--piper-bin",
            "./piper",
            "--piper-model",
            "./voice.onnx",
            "--speak-responses",
        ])
        .unwrap();
        assert_eq!(options.provider, "ollama");
        assert_eq!(options.ollama_model, "qwen2.5-coder:1.5b");
        assert_eq!(options.ollama_base_url, "http://127.0.0.1:11434");
        assert_eq!(options.system_prompt, "Be brief.");
        assert!(options.enable_hotkeys);
        assert!(options.enable_model_tools);
        assert!(options.enable_model_thinking);
        assert_eq!(
            options.db_path.as_deref(),
            Some(std::path::Path::new("./orbital.db"))
        );
        assert!(options.store_session_messages);
        assert!(options.store_context_items);
        assert_eq!(options.speech.stt, "whisper");
        assert_eq!(
            options.speech.whisper_model_path.as_deref(),
            Some(std::path::Path::new("./tiny.bin"))
        );
        assert_eq!(options.speech.tts, "piper");
        assert!(options.speech.speak_responses);
    }

    #[test]
    fn mock_provider_identifies_selected_text_context() {
        let mut request = request("explain this");
        request.prompt_context = Some(
            "[Orbital Context]\nContext item 1:\nType: selected_text\nTitle: Selected Text\nContent:\nmissing class Foo"
                .into(),
        );
        request.context_item_count = 1;
        let response = MockModelProvider.generate(request).unwrap();
        assert!(response.text.contains("1 selected_text context item"));
        assert!(response.text.contains("missing class Foo"));
    }

    #[test]
    fn mock_provider_can_emit_explicit_tool_test_hook() {
        let suggestion = MockModelProvider
            .suggest_tool(&ToolPlanningRequest {
                user_input: "Search memory for Orbital and summarize it.".into(),
                tool_names: vec!["memory.search".into()],
                tool_schemas: vec![("memory.search".into(), serde_json::json!({"type":"object"}))],
                tool_descriptions: vec![("memory.search".into(), "Search memory".into())],
            })
            .unwrap()
            .unwrap();
        assert_eq!(suggestion.tool, "memory.search");
        assert_eq!(suggestion.arguments["query"], "Orbital");
    }
}
