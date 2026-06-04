//! Safety engine: validates every optimization request before execution.
//!
//! Rules enforced here:
//! - Analysis-only mode never executes anything.
//! - Actions that change behavior require explicit user confirmation.
//! - Destructive operations (process termination, working-set trimming as a
//!   RAM-cleaning routine, standby-list purging, pagefile modification) are
//!   not part of the action registry at all and are rejected by name.

use super::models::{ActionDescriptor, OptimizationMode};

/// Action ids that FluxCore refuses to ever execute, even if requested.
pub const FORBIDDEN_ACTIONS: &[&str] = &[
    "kill_process",
    "terminate_process",
    "empty_working_set",
    "trim_working_set",
    "purge_standby_list",
    "resize_pagefile",
    "disable_pagefile",
    "disable_compression",
];

pub fn is_forbidden(action_id: &str) -> bool {
    FORBIDDEN_ACTIONS.contains(&action_id)
}

/// Validates one execution request. Returns `Ok(())` when the action may run,
/// or a human-readable rejection reason.
pub fn validate(
    mode: OptimizationMode,
    descriptor: &ActionDescriptor,
    confirmed: bool,
) -> Result<(), String> {
    if is_forbidden(&descriptor.id) {
        return Err(format!("'{}' is a destructive operation and is not supported by FluxCore.", descriptor.id));
    }
    if mode == OptimizationMode::AnalysisOnly {
        return Err("Analysis-only mode performs no actions; switch to Smart Assist or Manual Optimization.".to_string());
    }
    if !descriptor.available_in_modes.contains(&mode) {
        return Err(format!("'{}' is not available in the current optimization mode.", descriptor.id));
    }
    if descriptor.requires_confirmation && !confirmed {
        return Err(format!(
            "'{}' requires explicit user confirmation before it runs.",
            descriptor.title
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory_engine::models::RiskLevel;

    fn descriptor(id: &str, requires_confirmation: bool, modes: Vec<OptimizationMode>) -> ActionDescriptor {
        ActionDescriptor {
            id: id.to_string(),
            title: id.to_string(),
            description: "test".to_string(),
            side_effects: "none".to_string(),
            risk: RiskLevel::Low,
            requires_confirmation,
            changes_system: false,
            available_in_modes: modes,
        }
    }

    fn all_modes() -> Vec<OptimizationMode> {
        vec![OptimizationMode::AnalysisOnly, OptimizationMode::SmartAssist, OptimizationMode::Manual]
    }

    #[test]
    fn destructive_actions_are_forbidden_by_name() {
        for forbidden in FORBIDDEN_ACTIONS {
            assert!(is_forbidden(forbidden));
            let result = validate(
                OptimizationMode::Manual,
                &descriptor(forbidden, true, all_modes()),
                true,
            );
            assert!(result.is_err(), "{forbidden} must never validate");
        }
    }

    #[test]
    fn analysis_only_mode_executes_nothing() {
        let result = validate(
            OptimizationMode::AnalysisOnly,
            &descriptor("clear_fluxcore_caches", true, all_modes()),
            true,
        );
        assert!(result.unwrap_err().contains("Analysis-only"));
    }

    #[test]
    fn confirmation_is_required_for_flagged_actions() {
        let descriptor = descriptor("clear_fluxcore_caches", true, all_modes());
        assert!(validate(OptimizationMode::Manual, &descriptor, false).is_err());
        assert!(validate(OptimizationMode::SmartAssist, &descriptor, true).is_ok());
    }

    #[test]
    fn mode_restrictions_are_enforced() {
        let descriptor = descriptor("guidance_pagefile", false, vec![OptimizationMode::Manual]);
        assert!(validate(OptimizationMode::SmartAssist, &descriptor, false).is_err());
        assert!(validate(OptimizationMode::Manual, &descriptor, false).is_ok());
    }
}
