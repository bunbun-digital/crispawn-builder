// Copyright (c) 2026 bunbun-digital and contributors.
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};

use crate::{Completion, Language, Project, validate_project};

const DEFAULTS: &str = include_str!("../../../config/crispawn/examples/live-v2-usb.json");

/// The community Builder emits local USB settings only. Network delivery and
/// driver acquisition belong to a separate product.
pub(crate) fn image_settings(project: &Project) -> Result<Value, &'static str> {
    validate_project(project)?;
    let mut runtime: Value =
        serde_json::from_str(DEFAULTS).map_err(|_| "Invalid runtime defaults.")?;
    if !project.source_ids.is_empty() {
        runtime["sources"] = json!({"mode": "selected_ids", "ids": project.source_ids});
    }
    runtime["destination"]["backup_directory"] = json!(project.options.backup_directory);
    runtime["startup"] = if project.options.startup_mode == "auto_copy" {
        json!({"mode": "auto_copy", "delay_seconds": project.start_seconds})
    } else {
        json!({"mode": project.options.startup_mode})
    };
    runtime["ui"]["mode"] = json!(project.options.ui_mode);
    runtime["ui"]["language"] = match project.language {
        Language::Japanese => json!("ja"),
        Language::English => json!("en"),
    };
    runtime["completion"] = match project.completion {
        Completion::Stay => json!({"action": "stay"}),
        Completion::PowerOff { seconds } => json!({"action": "poweroff", "delay_seconds": seconds}),
    };
    runtime
        .as_object_mut()
        .ok_or("Invalid runtime defaults.")?
        .remove("failure");
    if project.options.failure_action == "stay" {
        runtime["failure"] = json!({"action": "stay"});
    }
    runtime["logging"]["policy"] = json!(project.options.log_policy);
    runtime["logging"]["sync_interval_ms"] = json!(project.options.log_sync_interval_ms);
    runtime["processing"]["initial_profile"] = json!(project.options.initial_profile);
    runtime["processing"]["io_policy"] = json!({
        "no_progress_seconds": project.options.io_policy.no_progress_seconds,
        "failure_grace_seconds": project.options.io_policy.failure_grace_seconds,
        "shutdown_grace_seconds": project.options.io_policy.shutdown_grace_seconds,
        "transient_retries": project.options.io_policy.transient_retries,
        "retry_delay_ms": project.options.io_policy.retry_delay_ms,
    });
    runtime["processing"]
        .as_object_mut()
        .ok_or("Invalid runtime defaults.")?
        .remove("verify_after_write");
    if !project.options.verify_after_write {
        runtime["processing"]["verify_after_write"] = json!(false);
    }
    runtime["diagnostics"]["probe_interval_ms"] = json!(project.options.probe_interval_ms);
    runtime["network"] = json!({"mode": "none"});
    Ok(json!({"runtime": runtime, "credentials": null}))
}

pub fn preview(project: &Project) -> Result<String, String> {
    let settings = image_settings(project)?;
    serde_json::to_string_pretty(&settings["runtime"])
        .map_err(|_| "Could not serialize runtime settings.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn community_runtime_disables_network_and_credentials() {
        let settings = image_settings(&Project::default()).unwrap();
        assert_eq!(settings["runtime"]["network"]["mode"], "none");
        assert!(settings["credentials"].is_null());
        assert_eq!(settings["runtime"]["destination"]["kind"], "usb_disk");
        assert!(
            settings["runtime"]["processing"]
                .get("verify_after_write")
                .is_none()
        );
    }

    #[test]
    fn advanced_project_maps_to_runtime_without_legacy_fallback() {
        let mut project = Project::default();
        project.options.backup_directory = "crispawn/archive".into();
        project.options.startup_mode = "menu".into();
        project.options.failure_action = "stay".into();
        project.options.log_policy = "on_error".into();
        project.options.log_sync_interval_ms = 500;
        project.options.initial_profile = "high_compression".into();
        project.options.io_policy.transient_retries = 5;
        project.options.verify_after_write = false;
        project.options.probe_interval_ms = 1000;
        let runtime = image_settings(&project).unwrap()["runtime"].clone();
        assert!(crate::plan(&project).is_err());
        assert_eq!(
            runtime["destination"]["backup_directory"],
            "crispawn/archive"
        );
        assert_eq!(runtime["startup"], json!({"mode": "menu"}));
        assert_eq!(runtime["failure"]["action"], "stay");
        assert_eq!(runtime["logging"]["policy"], "on_error");
        assert_eq!(runtime["logging"]["sync_interval_ms"], 500);
        assert_eq!(runtime["processing"]["initial_profile"], "high_compression");
        assert_eq!(runtime["processing"]["io_policy"]["transient_retries"], 5);
        assert_eq!(runtime["processing"]["verify_after_write"], false);
        assert_eq!(runtime["diagnostics"]["probe_interval_ms"], 1000);
    }
}
