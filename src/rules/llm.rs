use crate::config::Config;
use crate::rules::{CleanupRule, cache_rule};
use crate::scanner::entry::{Category, SafetyLevel, ScannedEntry};
use crate::scanner::walker;

// -- Ollama --
// Models stored in ~/.ollama/models (blobs can be many GB each)

pub struct OllamaModelsRule;

impl CleanupRule for OllamaModelsRule {
    fn name(&self) -> &'static str {
        "Ollama models"
    }

    fn category(&self) -> Category {
        Category::LlmModels
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let home = dirs::home_dir().unwrap_or_default();
        let ollama_dir = home.join(".ollama/models");

        if !ollama_dir.exists() {
            return Vec::new();
        }

        let mut entries = Vec::new();

        // Report the blobs directory (where the actual model weights live)
        let blobs_dir = ollama_dir.join("blobs");
        if blobs_dir.exists() {
            let size = walker::dir_size(&blobs_dir);
            if size > 0 {
                entries.push(ScannedEntry {
                    path: blobs_dir,
                    size,
                    category: Category::LlmModels,
                    safety: SafetyLevel::Caution,
                    description: "Ollama model blobs".to_owned(),
                    item_count: None,
                });
            }
        }

        // Report manifests separately (small, but shows what's installed)
        let manifests_dir = ollama_dir.join("manifests");
        if manifests_dir.exists() {
            let size = walker::dir_size(&manifests_dir);
            if size > 0 {
                entries.push(ScannedEntry {
                    path: manifests_dir,
                    size,
                    category: Category::LlmModels,
                    safety: SafetyLevel::Caution,
                    description: "Ollama manifests".to_owned(),
                    item_count: None,
                });
            }
        }

        entries
    }
}

// -- HuggingFace Hub cache --
// ~/.cache/huggingface/hub stores downloaded models and datasets

cache_rule!(
    HuggingFaceCacheRule,
    "HuggingFace cache",
    Category::LlmModels,
    SafetyLevel::Caution,
    ".cache/huggingface"
);

// -- LM Studio --
// Models in ~/.cache/lm-studio/models and app data in ~/Library/Application Support/LM Studio

pub struct LmStudioModelsRule;

impl CleanupRule for LmStudioModelsRule {
    fn name(&self) -> &'static str {
        "LM Studio models"
    }

    fn category(&self) -> Category {
        Category::LlmModels
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let home = dirs::home_dir().unwrap_or_default();
        let mut entries = Vec::new();

        // Primary model storage: ~/.cache/lm-studio
        let cache_dir = home.join(".cache/lm-studio");
        if cache_dir.exists() {
            let size = walker::dir_size(&cache_dir);
            if size > 0 {
                entries.push(ScannedEntry {
                    path: cache_dir,
                    size,
                    category: Category::LlmModels,
                    safety: SafetyLevel::Caution,
                    description: "LM Studio models cache".to_owned(),
                    item_count: None,
                });
            }
        }

        // Alternative: ~/.lmstudio/models
        let lmstudio_dir = home.join(".lmstudio/models");
        if lmstudio_dir.exists() {
            let size = walker::dir_size(&lmstudio_dir);
            if size > 0 {
                entries.push(ScannedEntry {
                    path: lmstudio_dir,
                    size,
                    category: Category::LlmModels,
                    safety: SafetyLevel::Caution,
                    description: "LM Studio models".to_owned(),
                    item_count: None,
                });
            }
        }

        // App support data (electron cache, logs, etc.)
        let app_support = home.join("Library/Application Support/LM Studio");
        if app_support.exists() {
            let size = walker::dir_size(&app_support);
            if size > 0 {
                entries.push(ScannedEntry {
                    path: app_support,
                    size,
                    category: Category::LlmModels,
                    safety: SafetyLevel::Caution,
                    description: "LM Studio app data".to_owned(),
                    item_count: None,
                });
            }
        }

        entries
    }
}

// -- GPT4All --
// ~/.cache/gpt4all or ~/Library/Application Support/nomic.ai

pub struct Gpt4AllRule;

impl CleanupRule for Gpt4AllRule {
    fn name(&self) -> &'static str {
        "GPT4All models"
    }

    fn category(&self) -> Category {
        Category::LlmModels
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let home = dirs::home_dir().unwrap_or_default();
        let mut entries = Vec::new();

        let paths = [
            (home.join(".cache/gpt4all"), "GPT4All cache"),
            (
                home.join("Library/Application Support/nomic.ai"),
                "GPT4All app data",
            ),
            (home.join(".local/share/nomic.ai"), "GPT4All local data"),
        ];

        for (path, desc) in paths {
            if path.exists() {
                let size = walker::dir_size(&path);
                if size > 0 {
                    entries.push(ScannedEntry {
                        path,
                        size,
                        category: Category::LlmModels,
                        safety: SafetyLevel::Caution,
                        description: desc.to_owned(),
                        item_count: None,
                    });
                }
            }
        }

        entries
    }
}

// -- Jan AI --
// ~/jan/models or ~/.jan/models or ~/Library/Application Support/Jan

pub struct JanAiRule;

impl CleanupRule for JanAiRule {
    fn name(&self) -> &'static str {
        "Jan AI models"
    }

    fn category(&self) -> Category {
        Category::LlmModels
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let home = dirs::home_dir().unwrap_or_default();
        let mut entries = Vec::new();

        let paths = [
            (home.join("jan/models"), "Jan AI models"),
            (home.join(".jan/models"), "Jan AI models"),
            (
                home.join("Library/Application Support/Jan"),
                "Jan AI app data",
            ),
        ];

        for (path, desc) in paths {
            if path.exists() {
                let size = walker::dir_size(&path);
                if size > 0 {
                    entries.push(ScannedEntry {
                        path,
                        size,
                        category: Category::LlmModels,
                        safety: SafetyLevel::Caution,
                        description: desc.to_owned(),
                        item_count: None,
                    });
                }
            }
        }

        entries
    }
}

// -- LocalAI / llama.cpp caches --

cache_rule!(
    LlamaCppCacheRule,
    "llama.cpp cache",
    Category::LlmModels,
    SafetyLevel::Safe,
    "Library/Caches/llama.cpp"
);

pub fn rules() -> Vec<Box<dyn CleanupRule>> {
    vec![
        Box::new(OllamaModelsRule),
        Box::new(HuggingFaceCacheRule),
        Box::new(LmStudioModelsRule),
        Box::new(Gpt4AllRule),
        Box::new(JanAiRule),
        Box::new(LlamaCppCacheRule),
    ]
}
