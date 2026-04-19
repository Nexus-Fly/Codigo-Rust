use std::collections::HashMap;
use crate::types::{AlertId, Point};

#[derive(Debug, Clone)]
pub struct SafetyZone {
    pub alert_id: AlertId,
    pub center:   Point,
    pub radius:   f64,
    pub active:   bool,
}

/// Safety mesh: propagates alerts and freezes agents inside zones.
#[derive(Debug, Default)]
pub struct SafetyMesh {
    zones: HashMap<AlertId, SafetyZone>,
}

impl SafetyMesh {
    pub fn new() -> Self { Self::default() }

    /// Activate a safety zone (triggered by SafetyAlert transaction).
    pub fn activate(&mut self, alert_id: AlertId, center: Point, radius: f64) {
        tracing::warn!(
            "[safety] Alert {alert_id} activated: center ({:.2},{:.2}) radius {:.2}",
            center.x, center.y, radius
        );
        self.zones.insert(alert_id, SafetyZone { alert_id, center, radius, active: true });
    }

    /// Deactivate a safety zone (SafetyClear transaction).
    pub fn clear(&mut self, alert_id: AlertId) {
        if let Some(zone) = self.zones.get_mut(&alert_id) {
            zone.active = false;
            tracing::info!("[safety] Alert {alert_id} cleared");
        }
    }

    /// Return true if `position` is inside any active safety zone.
    pub fn is_frozen(&self, position: &Point) -> bool {
        self.zones.values().any(|z| {
            z.active && position.distance_to(&z.center) <= z.radius
        })
    }

    /// All active zones.
    pub fn active_zones(&self) -> Vec<&SafetyZone> {
        self.zones.values().filter(|z| z.active).collect()
    }
}
