//! Safe optimization action registry.
//!
//! The registry contains only actions FluxCore can justify: operations on
//! FluxCore's own caches and background activity, plus read-only guidance.
//! Execution lives in the engine (`MemoryEngine::execute_action`); this
//! module owns the descriptors and the read-only guidance collectors.

use super::models::{ActionDescriptor, OptimizationMode, RiskLevel};
use super::recommendations::{
    ACTION_CLEAR_CACHES, ACTION_GUIDANCE_CLOSE_APPS, ACTION_GUIDANCE_PAGEFILE,
    ACTION_GUIDANCE_STARTUP, ACTION_PAUSE_BACKGROUND, ACTION_REFRESH_DATA,
    ACTION_RELEASE_RESOURCES,
};

fn all_modes() -> Vec<OptimizationMode> {
    vec![OptimizationMode::SmartAssist, OptimizationMode::Manual]
}

/// The complete action registry. Anything not listed here cannot run.
pub fn descriptors() -> Vec<ActionDescriptor> {
    vec![
        ActionDescriptor {
            id: ACTION_CLEAR_CACHES.to_string(),
            title: "Clear FluxCore caches".to_string(),
            description: "Drops FluxCore's in-memory history buffers beyond the retention window and prunes expired rows from the local database.".to_string(),
            side_effects: "Older charts lose detail beyond the retained window; monitoring continues unaffected.".to_string(),
            risk: RiskLevel::Low,
            requires_confirmation: true,
            changes_system: false,
            available_in_modes: all_modes(),
        },
        ActionDescriptor {
            id: ACTION_RELEASE_RESOURCES.to_string(),
            title: "Release unused FluxCore resources".to_string(),
            description: "Shrinks internal buffers (trend windows, recommendation log, audit ring) back to their minimum sizes.".to_string(),
            side_effects: "Trend and prediction quality degrades until the windows refill.".to_string(),
            risk: RiskLevel::Low,
            requires_confirmation: true,
            changes_system: false,
            available_in_modes: all_modes(),
        },
        ActionDescriptor {
            id: ACTION_PAUSE_BACKGROUND.to_string(),
            title: "Pause FluxCore background activity".to_string(),
            description: "Suspends process scanning and database persistence for five minutes; lightweight analysis continues.".to_string(),
            side_effects: "Process trends and optimization history have a five-minute gap.".to_string(),
            risk: RiskLevel::Low,
            requires_confirmation: true,
            changes_system: false,
            available_in_modes: all_modes(),
        },
        ActionDescriptor {
            id: ACTION_REFRESH_DATA.to_string(),
            title: "Refresh stale monitoring data".to_string(),
            description: "Forces an immediate process rescan on the next monitoring tick.".to_string(),
            side_effects: "One slightly more expensive collection cycle.".to_string(),
            risk: RiskLevel::None,
            requires_confirmation: false,
            changes_system: false,
            available_in_modes: all_modes(),
        },
        ActionDescriptor {
            id: ACTION_GUIDANCE_CLOSE_APPS.to_string(),
            title: "Guidance: closing unnecessary applications".to_string(),
            description: "Lists the current top private-memory consumers so you can decide what to close yourself.".to_string(),
            side_effects: "None; FluxCore never closes applications.".to_string(),
            risk: RiskLevel::None,
            requires_confirmation: false,
            changes_system: false,
            available_in_modes: all_modes(),
        },
        ActionDescriptor {
            id: ACTION_GUIDANCE_STARTUP.to_string(),
            title: "Guidance: managing startup applications".to_string(),
            description: "Reads the documented Run registry keys (read-only) and lists registered startup entries.".to_string(),
            side_effects: "None; entries are only listed, never modified.".to_string(),
            risk: RiskLevel::None,
            requires_confirmation: false,
            changes_system: false,
            available_in_modes: all_modes(),
        },
        ActionDescriptor {
            id: ACTION_GUIDANCE_PAGEFILE.to_string(),
            title: "Guidance: pagefile configuration".to_string(),
            description: "Shows the current pagefile configuration and explains the manual Windows steps plus their consequences.".to_string(),
            side_effects: "None; the pagefile is never resized, moved or disabled by FluxCore.".to_string(),
            risk: RiskLevel::None,
            requires_confirmation: false,
            changes_system: false,
            available_in_modes: all_modes(),
        },
    ]
}

pub fn descriptor(action_id: &str) -> Option<ActionDescriptor> {
    descriptors().into_iter().find(|descriptor| descriptor.id == action_id)
}

/// Reads the documented startup Run keys (HKCU and HKLM), read-only.
#[cfg(windows)]
pub fn startup_entries() -> Vec<String> {
    use windows::core::w;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegEnumValueW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE,
        KEY_READ,
    };
    let paths: [(HKEY, windows::core::PCWSTR); 2] = [
        (HKEY_CURRENT_USER, w!(r"Software\Microsoft\Windows\CurrentVersion\Run")),
        (HKEY_LOCAL_MACHINE, w!(r"Software\Microsoft\Windows\CurrentVersion\Run")),
    ];
    let mut entries = Vec::new();
    for (root, path) in paths {
        let mut key = HKEY::default();
        if unsafe { RegOpenKeyExW(root, path, 0, KEY_READ, &mut key) }.is_err() {
            continue;
        }
        let mut index = 0u32;
        loop {
            let mut name = [0u16; 256];
            let mut name_len = name.len() as u32;
            let result = unsafe {
                RegEnumValueW(
                    key,
                    index,
                    windows::core::PWSTR(name.as_mut_ptr()),
                    &mut name_len,
                    None,
                    None,
                    None,
                    None,
                )
            };
            if result.is_err() {
                break;
            }
            let value = String::from_utf16_lossy(&name[..name_len as usize]);
            if !value.is_empty() {
                entries.push(value);
            }
            index += 1;
            if index > 512 {
                break;
            }
        }
        unsafe {
            let _ = RegCloseKey(key);
        }
    }
    entries
}

#[cfg(not(windows))]
pub fn startup_entries() -> Vec<String> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_covers_every_documented_action() {
        let registry = descriptors();
        let ids: Vec<&str> = registry.iter().map(|descriptor| descriptor.id.as_str()).collect();
        for expected in [
            ACTION_CLEAR_CACHES,
            ACTION_RELEASE_RESOURCES,
            ACTION_PAUSE_BACKGROUND,
            ACTION_REFRESH_DATA,
            ACTION_GUIDANCE_CLOSE_APPS,
            ACTION_GUIDANCE_STARTUP,
            ACTION_GUIDANCE_PAGEFILE,
        ] {
            assert!(ids.contains(&expected), "missing action {expected}");
        }
    }

    #[test]
    fn no_descriptor_is_destructive() {
        for descriptor in descriptors() {
            assert!(!super::super::safety::is_forbidden(&descriptor.id));
            assert!(!descriptor.changes_system, "{} must not change system state", descriptor.id);
        }
    }

    #[test]
    fn analysis_only_mode_has_no_executable_actions() {
        for descriptor in descriptors() {
            assert!(!descriptor.available_in_modes.contains(&OptimizationMode::AnalysisOnly));
        }
    }

    #[test]
    fn descriptor_lookup_by_id() {
        assert!(descriptor(ACTION_REFRESH_DATA).is_some());
        assert!(descriptor("kill_process").is_none());
    }
}
