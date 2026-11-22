// Copyright (c) 2026 bunbun-digital and contributors.
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};

use crate::{Completion, Language, Project, plan};

const DEFAULTS: &str = include_str!("../../../config/crispawn/examples/live-v2-usb.json");

/// The community Builder emits local USB settings only. Network delivery and
/// driver acquisition belong to a separate product.
pub(crate) fn image_settings(project: &Project) -> Result<Value, &'static str> {
    plan(project)?;
    let mut runtime: Value =
        serde_json::from_str(DEFAULTS).map_err(|_| "Invalid runtime defaults.")?;
    if !project.source_ids.is_empty() {
        runtime["sources"] = json!({"mode": "selected_ids", "ids": project.source_ids});
    }
    runtime["startup"]["delay_seconds"] = json!(project.start_seconds);
    runtime["ui"]["language"] = match project.language {
        Language::Japanese => json!("ja"),
        Language::English => json!("en"),
    };
    runtime["completion"] = match project.completion {
        Completion::Stay => json!({"action": "stay"}),
        Completion::PowerOff { seconds } => json!({"action": "poweroff", "delay_seconds": seconds}),
    };
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
    }
}
