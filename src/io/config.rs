use crate::entity::AppConfig;
use std::fs;
use std::path::{Path, PathBuf};

const DIR_NAME: &str = ".kotomemo";
const CONFIG_FILE: &str = "config";

fn home_dir() -> PathBuf {
    #[cfg(windows)]
    let home = std::env::var("USERPROFILE");
    #[cfg(not(windows))]
    let home = std::env::var("HOME");
    PathBuf::from(home.unwrap_or_default())
}

pub fn config_dir() -> PathBuf {
    home_dir().join(DIR_NAME)
}

pub fn config_file() -> PathBuf {
    config_dir().join(CONFIG_FILE)
}

/// Missing file or a parse failure both silently fall back to
/// `AppConfig::default()` — no error is surfaced to the user on a corrupt
/// config, matching the Kotlin `JsonConfigRepository.load()` behaviour.
/// `#[serde(default)]` on every `AppConfig`/`ApiPreset` field gives the same
/// tolerance for old/foreign JSON keys as kotlinx.serialization's
/// `ignoreUnknownKeys = true`, so a config file written by the Kotlin build
/// keeps loading here.
pub fn load(path: &Path) -> AppConfig {
    let Ok(text) = fs::read_to_string(path) else {
        return AppConfig::default();
    };
    if text.trim().is_empty() {
        return AppConfig::default();
    }
    serde_json::from_str(&text).unwrap_or_default()
}

pub fn save(path: &Path, config: &AppConfig) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)?;
        }
    }
    let text = serde_json::to_string_pretty(config).expect("AppConfig always serializes");
    fs::write(path, text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{ApiPreset, HttpMethod, ResponseTarget};
    use std::collections::HashMap;
    use std::env;

    fn temp_path(name: &str) -> PathBuf {
        let mut p = env::temp_dir();
        p.push(format!(
            "kotomemo-config-test-{}-{name}",
            std::process::id()
        ));
        p
    }

    #[test]
    fn missing_file_loads_default() {
        let tmp = temp_path("missing/config");
        assert!(!tmp.exists());
        let cfg = load(&tmp);
        assert!(cfg.presets.is_empty());
        assert_eq!(cfg.attachments_folder, "attachments");
    }

    #[test]
    fn save_then_load_round_trips_presets_and_tokens() {
        let tmp = temp_path("roundtrip/config");
        let mut headers = HashMap::new();
        headers.insert("Authorization".to_string(), "Bearer {{tokens.t}}".to_string());
        let mut tokens = HashMap::new();
        tokens.insert("t".to_string(), "secret".to_string());
        let cfg = AppConfig {
            presets: vec![ApiPreset {
                name: "echo".to_string(),
                url: "https://example.com".to_string(),
                method: HttpMethod::Post,
                headers,
                body_template: "{{selectionJson}}".to_string(),
                response_json_path: Some("choices.0.text".to_string()),
                response_target: ResponseTarget::AfterSelection,
            }],
            tokens,
            attachments_folder: "img".to_string(),
            request_timeout_secs: 30,
            ..AppConfig::default()
        };

        save(&tmp, &cfg).unwrap();
        let reloaded = load(&tmp);
        assert_eq!(reloaded.presets.len(), 1);
        assert_eq!(reloaded.presets[0].name, "echo");
        assert_eq!(reloaded.presets[0].method, HttpMethod::Post);
        assert_eq!(reloaded.presets[0].response_target, ResponseTarget::AfterSelection);
        assert_eq!(reloaded.tokens.get("t"), Some(&"secret".to_string()));
        assert_eq!(reloaded.attachments_folder, "img");
        assert_eq!(reloaded.request_timeout_secs, 30);

        fs::remove_file(&tmp).ok();
    }

    #[test]
    fn unknown_json_keys_are_tolerated() {
        let tmp = temp_path("forward-compat/config");
        fs::create_dir_all(tmp.parent().unwrap()).unwrap();
        fs::write(
            &tmp,
            r#"{"presets":[],"tokens":{},"attachmentsFolder":"attachments","futureField":123}"#,
        )
        .unwrap();
        let cfg = load(&tmp);
        assert_eq!(cfg.attachments_folder, "attachments");
        fs::remove_file(&tmp).ok();
    }

    #[test]
    fn blank_file_loads_default() {
        let tmp = temp_path("blank/config");
        fs::create_dir_all(tmp.parent().unwrap()).unwrap();
        fs::write(&tmp, "   ").unwrap();
        let cfg = load(&tmp);
        assert!(cfg.presets.is_empty());
        fs::remove_file(&tmp).ok();
    }
}
