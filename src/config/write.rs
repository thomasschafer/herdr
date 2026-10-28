#[derive(Clone, Copy)]
pub(crate) enum ConfigEdit<'a> {
    Theme(&'a str),
    StatusIndicators(super::StatusIndicatorStyle),
    Sound(bool),
    ToastDelivery(super::ToastDelivery),
}

impl ConfigEdit<'_> {
    pub(crate) fn description(self) -> &'static str {
        match self {
            Self::Theme(_) => "theme",
            Self::StatusIndicators(_) => "status indicators",
            Self::Sound(_) => "sound setting",
            Self::ToastDelivery(_) => "toast setting",
        }
    }

    /// The config keys this edit writes or removes.
    pub(crate) fn keys(self) -> &'static [&'static [&'static str]] {
        match self {
            Self::Theme(_) => &[&["theme", "name"], &["theme", "auto_switch"]],
            Self::StatusIndicators(_) => &[&["ui", "status_indicators"]],
            Self::Sound(_) => &[&["ui", "sound", "enabled"]],
            Self::ToastDelivery(_) => &[&["ui", "toast", "delivery"], &["ui", "toast", "enabled"]],
        }
    }

    pub(crate) fn apply(self, content: &str) -> String {
        match self {
            Self::Theme(name) => {
                let content =
                    super::upsert_section_value(content, "theme", "name", &format!("\"{name}\""));
                super::upsert_section_bool(&content, "theme", "auto_switch", false)
            }
            Self::StatusIndicators(style) => super::upsert_section_value(
                content,
                "ui",
                "status_indicators",
                &format!("\"{}\"", style.as_str()),
            ),
            Self::Sound(enabled) => {
                super::upsert_section_bool(content, "ui.sound", "enabled", enabled)
            }
            Self::ToastDelivery(delivery) => {
                let value = match delivery {
                    super::ToastDelivery::Off => "\"off\"",
                    super::ToastDelivery::Herdr => "\"herdr\"",
                    super::ToastDelivery::Terminal => "\"terminal\"",
                    super::ToastDelivery::System => "\"system\"",
                };
                let content = super::upsert_section_value(content, "ui.toast", "delivery", value);
                super::remove_section_key(&content, "ui.toast", "enabled")
            }
        }
    }
}

/// Rewrites the config file at `path` with `update`, which changes only `keys`.
pub(crate) fn update_file_at(
    path: &std::path::Path,
    description: &str,
    keys: &[&[&str]],
    update: impl FnOnce(&str) -> String,
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create config directory: {error}"))?;
    }
    let content = match super::io::read_optional_config(path) {
        Ok(Some(content)) => content,
        Ok(None) => String::new(),
        Err(error) => {
            return Err(format!(
                "failed to read config before saving {description}: {error}"
            ));
        }
    };
    super::io::ensure_config_keys_not_included(path, keys)
        .map_err(|error| format!("can't save {description}: {error}"))?;
    std::fs::write(path, update(&content))
        .map_err(|error| format!("failed to save {description}: {error}"))
}

pub(crate) fn write_edit(edit: ConfigEdit<'_>) -> Result<(), String> {
    update_file_at(
        &super::config_path(),
        edit.description(),
        edit.keys(),
        |content| edit.apply(content),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_file_at_does_not_move_a_leading_bom_into_the_file() {
        let dir = std::env::temp_dir().join(format!("herdr-config-bom-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(
            &path,
            b"\xEF\xBB\xBF[terminal]\ndefault_shell = \"pwsh.exe\"\n",
        )
        .unwrap();

        update_file_at(&path, "onboarding setting", &[&["onboarding"]], |content| {
            crate::config::upsert_top_level_bool(content, "onboarding", false)
        })
        .unwrap();

        let written = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_dir_all(dir);

        assert!(
            !written.contains('\u{feff}'),
            "unexpected BOM in {written:?}"
        );
        assert!(
            toml::from_str::<toml::Value>(&written).is_ok(),
            "written config is not valid TOML: {written:?}"
        );
    }

    #[test]
    fn update_file_at_refuses_to_save_keys_set_in_included_files() {
        let dir =
            std::env::temp_dir().join(format!("herdr-config-included-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        let content = "include = [\"config.local.toml\"]\n[ui.sound]\nenabled = true\n";
        std::fs::write(&path, content).unwrap();
        std::fs::write(
            dir.join("config.local.toml"),
            "[ui.sound]\nenabled = true\n",
        )
        .unwrap();

        let edit = ConfigEdit::Sound(false);
        let result = update_file_at(&path, edit.description(), edit.keys(), |content| {
            edit.apply(content)
        });
        let written = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_dir_all(dir);

        let error = result.unwrap_err();
        assert!(
            error.starts_with("can't save sound setting: ui.sound.enabled is set in "),
            "{error}"
        );
        assert_eq!(written, content);
    }
}
