// Copyright (c) 2026 bunbun-digital and contributors. SPDX-License-Identifier: Apache-2.0

mod cpio;
pub mod efi;
mod fat12;
pub mod image;
mod iso9660;
pub mod release;
pub mod runtime;
pub mod usb;

use serde::Deserialize;

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

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IoPolicy {
    pub no_progress_seconds: u32,
    pub failure_grace_seconds: u32,
    pub shutdown_grace_seconds: u32,
    pub transient_retries: u32,
    pub retry_delay_ms: u32,
}

impl Default for IoPolicy {
    fn default() -> Self {
        Self {
            no_progress_seconds: 180,
            failure_grace_seconds: 5,
            shutdown_grace_seconds: 20,
            transient_retries: 3,
            retry_delay_ms: 100,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeOptions {
    pub backup_directory: String,
    pub startup_mode: String,
    pub ui_mode: String,
    pub failure_action: String,
    pub log_policy: String,
    pub log_sync_interval_ms: u32,
    pub initial_profile: String,
    pub io_policy: IoPolicy,
    pub verify_after_write: bool,
    pub probe_interval_ms: u32,
}

impl Default for RuntimeOptions {
    fn default() -> Self {
        Self {
            backup_directory: "crispawn/backups".into(),
            startup_mode: "auto_copy".into(),
            ui_mode: "gui".into(),
            failure_action: "poweroff".into(),
            log_policy: "always".into(),
            log_sync_interval_ms: 0,
            initial_profile: "balanced".into(),
            io_policy: IoPolicy::default(),
            verify_after_write: true,
            probe_interval_ms: 250,
        }
    }
}

pub struct Project {
    /// IDs already validated against the Linux stable-ID contract.
    /// Windows drive letters and disk numbers must never be converted here.
    pub source_ids: Vec<String>,
    pub language: Language,
    pub start_seconds: u32,
    pub completion: Completion,
    pub options: RuntimeOptions,
}

impl Default for Project {
    fn default() -> Self {
        Self {
            source_ids: Vec::new(),
            language: Language::Japanese,
            start_seconds: 30,
            completion: Completion::PowerOff { seconds: 5 },
            options: RuntimeOptions::default(),
        }
    }
}

impl Project {
    pub fn requires_v2(&self) -> bool {
        self.options != RuntimeOptions::default()
    }
}

pub fn validate_project(project: &Project) -> Result<(), &'static str> {
    let options = &project.options;
    if project.start_seconds > 300 {
        return Err("automatic start delay must be 0..300 seconds");
    }
    if let Completion::PowerOff { seconds } = project.completion {
        if seconds > 60 {
            return Err("power-off delay must be 0..60 seconds");
        }
    }
    if !matches!(
        options.startup_mode.as_str(),
        "auto_copy" | "menu" | "restore_select"
    ) {
        return Err("unsupported startup mode");
    }
    if !matches!(options.ui_mode.as_str(), "gui" | "console" | "headless") {
        return Err("unsupported LiveOS interface mode");
    }
    if options.ui_mode == "headless"
        && (options.startup_mode != "auto_copy" || project.start_seconds != 0)
    {
        return Err("headless mode requires automatic startup with zero delay");
    }
    if options.ui_mode == "console"
        && options.startup_mode == "auto_copy"
        && project.start_seconds != 0
    {
        return Err("console automatic startup requires zero delay");
    }
    if !matches!(options.failure_action.as_str(), "poweroff" | "stay") {
        return Err("unsupported failure action");
    }
    if !matches!(options.log_policy.as_str(), "always" | "on_error" | "never") {
        return Err("unsupported log policy");
    }
    if options.log_sync_interval_ms > 60_000 {
        return Err("log sync interval must be 0..60000 ms");
    }
    if !matches!(
        options.initial_profile.as_str(),
        "balanced" | "long_matches" | "high_compression" | "incompressible"
    ) {
        return Err("unsupported processing profile");
    }
    let io = &options.io_policy;
    if !(1..=3600).contains(&io.no_progress_seconds)
        || !(1..=120).contains(&io.failure_grace_seconds)
        || !(1..=120).contains(&io.shutdown_grace_seconds)
        || io.transient_retries > 8
        || !(1..=2000).contains(&io.retry_delay_ms)
    {
        return Err("I/O policy is outside the LiveOS limits");
    }
    if !(100..=5000).contains(&options.probe_interval_ms) {
        return Err("probe interval must be 100..5000 ms");
    }
    validate_backup_directory(&options.backup_directory)?;
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
    Ok(())
}

fn validate_backup_directory(path: &str) -> Result<(), &'static str> {
    if path.is_empty()
        || path.len() > 512
        || path
            .chars()
            .any(|c| c.is_control() || "\\<>:\"|?*".contains(c))
    {
        return Err("backup directory must be a portable relative path");
    }
    for part in path.split('/') {
        let stem = part.split('.').next().unwrap_or("").to_ascii_uppercase();
        if part.is_empty()
            || part.len() > 255
            || part == "."
            || part == ".."
            || part.ends_with([' ', '.'])
            || matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || ((stem.starts_with("COM") || stem.starts_with("LPT"))
                && stem.len() == 4
                && (b'1'..=b'9').contains(&stem.as_bytes()[3]))
        {
            return Err("backup directory must be a portable relative path");
        }
    }
    Ok(())
}

/// Builds the settings for a USB backup on a schema-v1 LiveOS release.
pub fn plan(project: &Project) -> Result<String, &'static str> {
    validate_project(project)?;
    if project.requires_v2() {
        return Err("These settings require a LiveOS release with runtime configuration v2");
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

    #[test]
    fn rejects_unsafe_runtime_options() {
        let mut project = Project::default();
        project.options.backup_directory = "../outside".into();
        assert!(validate_project(&project).is_err());
        project.options.backup_directory = "crispawn/backups".into();
        project.options.ui_mode = "headless".into();
        assert!(validate_project(&project).is_err());
        project.start_seconds = 0;
        assert!(validate_project(&project).is_ok());
        project.options.io_policy.retry_delay_ms = 0;
        assert!(validate_project(&project).is_err());
    }
}
