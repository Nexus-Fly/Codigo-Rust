use std::collections::HashMap;

use crate::types::SafetyZone;

// ---------------------------------------------------------------------------
// SafetyMonitor
// ---------------------------------------------------------------------------

/// Tracks active safety zones and determines whether an agent's position
/// falls inside a restricted area.
///
/// Distance is computed with a simple Euclidean formula on (x, y) coordinates.
/// For MVP purposes this is sufficient; a geodesic formula can replace it later.
#[derive(Debug, Default)]
pub struct SafetyMonitor {
    /// Active zones keyed by `zone_id`.
    zones: HashMap<String, SafetyZone>,
}

impl SafetyMonitor {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register or update an active safety zone.
    pub fn add_alert(&mut self, zone: SafetyZone) {
        self.zones.insert(zone.zone_id.clone(), zone);
    }

    /// Remove a safety zone by id. No-op if the zone is not tracked.
    pub fn clear_alert(&mut self, zone_id: &str) {
        self.zones.remove(zone_id);
    }

    /// Return `true` if `(x, y)` falls inside any active safety zone.
    ///
    /// Uses Euclidean distance. `radius_m` is treated as a unitless radius
    /// consistent with the coordinate units used by callers (degrees or metres).
    pub fn is_paused_by_safety(&self, x: f64, y: f64) -> bool {
        self.zones.values().any(|z| {
            if !z.active {
                return false;
            }
            let dx = z.center.0 - x;
            let dy = z.center.1 - y;
            let dist = (dx * dx + dy * dy).sqrt();
            dist <= z.radius_m as f64
        })
    }

    /// Snapshot of all currently active zone ids (useful for logging/debugging).
    #[allow(dead_code)]
    pub fn active_zone_ids(&self) -> Vec<&str> {
        self.zones
            .values()
            .filter(|z| z.active)
            .map(|z| z.zone_id.as_str())
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn zone(zone_id: &str, cx: f64, cy: f64, radius_m: f32, active: bool) -> SafetyZone {
        SafetyZone {
            zone_id: zone_id.to_owned(),
            center: (cx, cy),
            radius_m,
            active,
            declared_at: 0u64,
        }
    }

    #[test]
    fn agent_inside_radius_is_paused() {
        let mut monitor = SafetyMonitor::new();
        monitor.add_alert(zone("z1", 0.0, 0.0, 10.0, true));
        assert!(monitor.is_paused_by_safety(3.0, 4.0)); // dist = 5 < 10
    }

    #[test]
    fn agent_outside_radius_is_not_paused() {
        let mut monitor = SafetyMonitor::new();
        monitor.add_alert(zone("z1", 0.0, 0.0, 10.0, true));
        assert!(!monitor.is_paused_by_safety(8.0, 8.0)); // dist ≈ 11.3 > 10
    }

    #[test]
    fn agent_exactly_on_boundary_is_paused() {
        let mut monitor = SafetyMonitor::new();
        monitor.add_alert(zone("z1", 0.0, 0.0, 5.0, true));
        assert!(monitor.is_paused_by_safety(3.0, 4.0)); // dist = 5 == radius
    }

    #[test]
    fn clearing_alert_removes_restriction() {
        let mut monitor = SafetyMonitor::new();
        monitor.add_alert(zone("z1", 0.0, 0.0, 10.0, true));
        assert!(monitor.is_paused_by_safety(3.0, 4.0));
        monitor.clear_alert("z1");
        assert!(!monitor.is_paused_by_safety(3.0, 4.0));
    }

    #[test]
    fn multiple_zones_any_match_pauses() {
        let mut monitor = SafetyMonitor::new();
        monitor.add_alert(zone("z1", 100.0, 100.0, 5.0, true));
        monitor.add_alert(zone("z2", 0.0, 0.0, 10.0, true));
        // Only inside z2.
        assert!(monitor.is_paused_by_safety(3.0, 4.0));
        // Outside both.
        assert!(!monitor.is_paused_by_safety(50.0, 50.0));
    }

    #[test]
    fn inactive_zone_does_not_pause() {
        let mut monitor = SafetyMonitor::new();
        monitor.add_alert(zone("z1", 0.0, 0.0, 10.0, false));
        assert!(!monitor.is_paused_by_safety(3.0, 4.0));
    }
}

