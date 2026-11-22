// Copyright (c) 2026 bunbun-digital and contributors. SPDX-License-Identifier: Apache-2.0

mod cpio;
pub mod efi;
mod fat12;
pub mod image;
mod iso9660;
pub mod runtime;
pub mod usb;

#[derive(Clone, Copy, Default)]
pub enum Language {
    #[default]
    Japanese,
    English,
}

pub enum Completion {
    PowerOff { seconds: u32 },
    Stay,
}

pub struct Project {
    /// IDs already validated against the Linux stable-ID contract.
    /// Windows drive letters and disk numbers must never be converted here.
    pub source_ids: Vec<String>,
    pub language: Language,
    pub start_seconds: u32,
    pub completion: Completion,
}

impl Default for Project {
    fn default() -> Self {
        Self {
            source_ids: Vec::new(),
            language: Language::Japanese,
            start_seconds: 30,
            completion: Completion::PowerOff { seconds: 5 },
        }
    }
}

/// Builds the settings for a USB backup on a schema-v1 LiveOS release.
pub fn plan(project: &Project) -> Result<String, &'static str> {
    if project.start_seconds > 300 {
        return Err("automatic start delay must be 0..300 seconds");
    }
    let (power_off, completion_seconds) = match project.completion {
        Completion::PowerOff { seconds } if seconds <= 60 => (true, seconds),
        Completion::PowerOff { .. } => return Err("power-off delay must be 0..60 seconds"),
        Completion::Stay => (false, 5),
    };
    if project.source_ids.len() > 64 {
        return Err("at most 64 preset source IDs are supported");
    }
    for (index, id) in project.source_ids.iter().enumerate() {
        let valid_id = id
            .strip_prefix("wwid:")
            .is_some_and(|value| !value.is_empty())
            || id
                .strip_prefix("serial:")
                .and_then(|value| value.rsplit_once(":ns:"))
                .is_some_and(|(serial, namespace)| {
                    !serial.is_empty()
                        && serial.trim() == serial
                        && (namespace.is_empty()
                            || namespace == "0"
                            || (!namespace.starts_with('0')
                                && namespace.bytes().all(|c| c.is_ascii_digit())))
                });
        if !valid_id
            || id.len() > 512
            || id.trim() != id
            || id.chars().any(char::is_control)
            || id
                .strip_prefix("wwid:")
                .is_some_and(|value| value.trim() != value)
        {
            return Err("source IDs must use the Linux wwid: or serial:…:ns: format");
        }
        if project.source_ids[..index].contains(id) {
            return Err("source IDs must be unique");
        }
    }
    let language = match project.language {
        Language::Japanese => "ja",
        Language::English => "en",
    };
    let ids = project
        .source_ids
        .iter()
        .map(|id| quote_json(id))
        .collect::<Vec<_>>();
    let json = format!(
        concat!(
            "{{\n",
            "  \"schema_version\": 1,\n",
            "  \"operation\": \"backup\",\n",
            "  \"gui\": true,\n",
            "  \"language\": \"{}\",\n",
            "  \"backup_directory\": \"crispawn/backups\",\n",
            "  \"source_ids\": [{}],\n",
            "  \"menu_seconds\": {},\n",
            "  \"completion_seconds\": {},\n",
            "  \"poweroff_on_success\": {},\n",
            "  \"log_policy\": \"always\",\n",
            "  \"log_sync_interval_ms\": 0\n",
            "}}\n"
        ),
        language,
        ids.join(", "),
        project.start_seconds,
        completion_seconds,
        power_off
    );
    if json.len() > 65_536 {
        return Err("settings exceed the LiveOS ISO configuration size limit");
    }
    Ok(json)
}

fn quote_json(value: &str) -> String {
    let mut quoted = String::from("\"");
    for character in value.chars() {
        match character {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            _ => quoted.push(character),
        }
    }
    quoted.push('"');
    quoted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_out_of_range_delays() {
        let mut project = Project {
            start_seconds: 301,
            ..Project::default()
        };
        assert!(plan(&project).is_err());
        project.start_seconds = 300;
        project.completion = Completion::PowerOff { seconds: 61 };
        assert!(plan(&project).is_err());
        project.completion = Completion::PowerOff { seconds: 60 };
        assert!(plan(&project).is_ok());
    }

    #[test]
    fn rejects_unverified_id_formats_and_duplicates() {
        let mut project = Project {
            source_ids: vec!["C:".into()],
            ..Project::default()
        };
        assert!(plan(&project).is_err());
        project.source_ids = vec!["wwid:fixture".into(), "serial:fixture:ns:".into()];
        assert!(plan(&project).is_ok());
        project.source_ids.push("wwid:fixture".into());
        assert!(plan(&project).is_err());
    }
}
