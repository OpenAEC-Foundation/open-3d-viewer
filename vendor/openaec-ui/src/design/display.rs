//! The display settings an app keeps per user: theme, Werkruimte, Statusbalk
//! and Tekstgrootte (issue #27).
//!
//! They are saved as JSON next to the shortcuts, at [`settings_path`]
//! (`%APPDATA%\OpenAEC\<app>\weergave.json` on Windows). A missing or
//! unreadable file, or a value an older or newer version does not know, falls
//! back to the default, so a setting never stops an app from starting.
use super::{Look, StatusbarSetting, TextSize, ThemeId, WorkspaceSetting};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Display {
    pub theme: ThemeId,
    pub workspace: WorkspaceSetting,
    pub statusbar: StatusbarSetting,
    pub text_size: TextSize,
}

impl Display {
    /// The look for these settings.
    pub fn look(&self) -> Look {
        Look::new(self.theme, self.workspace, self.statusbar).with_text_size(self.text_size)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("display settings serialise")
    }

    /// Reads the JSON form; a field it does not understand keeps its default.
    pub fn from_json(json: &str) -> Self {
        let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(json) else {
            return Self::default();
        };
        fn field<T: for<'de> Deserialize<'de> + Default>(map: &serde_json::Map<String, serde_json::Value>, key: &str) -> T {
            map.get(key).and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default()
        }
        Self {
            theme: field(&map, "theme"),
            workspace: field(&map, "workspace"),
            statusbar: field(&map, "statusbar"),
            text_size: field(&map, "text-size"),
        }
    }

    /// Reads the file, or the defaults when there is none or it is unreadable.
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path).map(|json| Self::from_json(&json)).unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, self.to_json())
    }
}

/// Where an app keeps its display settings, next to its shortcuts.
pub fn settings_path(app: &str) -> Option<PathBuf> {
    super::shortcuts::app_dir(app).map(|dir| dir.join("weergave.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let d = Display { theme: ThemeId::Blueprint, workspace: WorkspaceSetting::Dark, statusbar: StatusbarSetting::Dark, text_size: TextSize::Large };
        assert_eq!(Display::from_json(&d.to_json()), d);
        assert!(d.to_json().contains("\"text-size\": \"large\""));
    }

    #[test]
    fn unknown_values_fall_back() {
        let d = Display::from_json(r#"{"theme": "sepia", "text-size": "compact", "extra": 1}"#);
        assert_eq!(d.theme, ThemeId::default());
        assert_eq!(d.text_size, TextSize::Compact);
        assert_eq!(Display::from_json("not json"), Display::default());
    }

    #[test]
    fn look_follows_the_text_size() {
        let look = Display { text_size: TextSize::Large, ..Default::default() }.look();
        assert_eq!(look.text, TextSize::Large.sizes());
        assert!(TextSize::Compact.sizes().body < look.text.body);
        // Standaard is the 0.4 size, and Compact stays readable (ontwerponderzoek, chapter 8).
        assert_eq!(TextSize::Standard.sizes().body, super::super::font::SIZE_BODY);
        assert!(TextSize::ALL.iter().all(|s| s.sizes().label >= 11.0));
    }
}
