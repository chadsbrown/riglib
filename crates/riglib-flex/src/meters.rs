//! FlexRadio meter types and runtime ID mapping.
//!
//! FlexRadio assigns numeric meter IDs dynamically at runtime. The mapping from
//! meter name to ID must be obtained via the TCP `meter list` command during the
//! connection handshake. This module provides well-known meter name constants
//! and a [`MeterMap`] for runtime ID resolution.

use std::collections::HashMap;

/// Well-known meter name: S-meter reading.
pub const METER_S_LEVEL: &str = "s-meter";

/// Well-known meter name: forward TX power.
pub const METER_POWER_FORWARD: &str = "power_forward";

/// Well-known meter name: reflected TX power.
pub const METER_POWER_REFLECTED: &str = "power_reflected";

/// Well-known meter name: standing wave ratio.
pub const METER_SWR: &str = "swr";

/// Well-known meter name: automatic level control.
pub const METER_ALC: &str = "alc";

/// Well-known meter name: microphone input level.
pub const METER_MIC_LEVEL: &str = "mic_level";

/// Well-known meter name: compression level.
pub const METER_COMP_LEVEL: &str = "comp_level";

/// Bidirectional mapping between meter names and their runtime-assigned IDs.
///
/// Populated at connection time from the radio's `meter list` response and
/// subsequent meter status messages.
#[derive(Debug, Clone)]
pub struct MeterMap {
    name_to_id: HashMap<String, u16>,
    id_to_name: HashMap<u16, String>,
}

impl MeterMap {
    /// Create an empty meter map.
    pub fn new() -> Self {
        MeterMap {
            name_to_id: HashMap::new(),
            id_to_name: HashMap::new(),
        }
    }

    /// Register a meter name-to-ID mapping.
    ///
    /// If the name or ID was previously registered, the old mapping is replaced.
    pub fn insert(&mut self, id: u16, name: &str) {
        if let Some(old_name) = self.id_to_name.remove(&id) {
            self.name_to_id.remove(&old_name);
        }

        if let Some(old_id) = self.name_to_id.remove(name) {
            self.id_to_name.remove(&old_id);
        }

        self.name_to_id.insert(name.to_string(), id);
        self.id_to_name.insert(id, name.to_string());
    }

    /// Look up a meter ID by its name.
    pub fn id_for_name(&self, name: &str) -> Option<u16> {
        self.name_to_id.get(name).copied()
    }

    /// Look up a meter name by its ID.
    pub fn name_for_id(&self, id: u16) -> Option<&str> {
        self.id_to_name.get(&id).map(|s| s.as_str())
    }
}

impl Default for MeterMap {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_map_is_empty() {
        let map = MeterMap::new();
        assert_eq!(map.id_for_name("swr"), None);
        assert_eq!(map.name_for_id(0), None);
    }

    #[test]
    fn insert_and_lookup_by_name() {
        let mut map = MeterMap::new();
        map.insert(5, METER_S_LEVEL);
        assert_eq!(map.id_for_name(METER_S_LEVEL), Some(5));
    }

    #[test]
    fn insert_and_lookup_by_id() {
        let mut map = MeterMap::new();
        map.insert(10, METER_POWER_FORWARD);
        assert_eq!(map.name_for_id(10), Some(METER_POWER_FORWARD));
    }

    #[test]
    fn unknown_name_returns_none() {
        let map = MeterMap::new();
        assert_eq!(map.id_for_name("nonexistent"), None);
    }

    #[test]
    fn unknown_id_returns_none() {
        let map = MeterMap::new();
        assert_eq!(map.name_for_id(999), None);
    }

    #[test]
    fn multiple_meters() {
        let mut map = MeterMap::new();
        map.insert(1, METER_S_LEVEL);
        map.insert(2, METER_SWR);
        map.insert(3, METER_ALC);
        map.insert(4, METER_POWER_FORWARD);
        map.insert(5, METER_POWER_REFLECTED);

        assert_eq!(map.id_for_name(METER_S_LEVEL), Some(1));
        assert_eq!(map.id_for_name(METER_SWR), Some(2));
        assert_eq!(map.id_for_name(METER_ALC), Some(3));
        assert_eq!(map.id_for_name(METER_POWER_FORWARD), Some(4));
        assert_eq!(map.id_for_name(METER_POWER_REFLECTED), Some(5));

        assert_eq!(map.name_for_id(1), Some(METER_S_LEVEL));
        assert_eq!(map.name_for_id(2), Some(METER_SWR));
        assert_eq!(map.name_for_id(3), Some(METER_ALC));
        assert_eq!(map.name_for_id(4), Some(METER_POWER_FORWARD));
        assert_eq!(map.name_for_id(5), Some(METER_POWER_REFLECTED));
    }

    #[test]
    fn insert_replaces_existing() {
        let mut map = MeterMap::new();
        map.insert(1, "old_name");
        map.insert(1, "new_name");
        // ID 1 now maps to "new_name"
        assert_eq!(map.name_for_id(1), Some("new_name"));
        assert_eq!(map.id_for_name("new_name"), Some(1));
        assert_eq!(map.id_for_name("old_name"), None);
    }

    #[test]
    fn insert_name_with_new_id_removes_stale_reverse_mapping() {
        let mut map = MeterMap::new();
        map.insert(1, "s-meter");
        map.insert(2, "s-meter");

        assert_eq!(map.id_for_name("s-meter"), Some(2));
        assert_eq!(map.name_for_id(1), None);
        assert_eq!(map.name_for_id(2), Some("s-meter"));
    }

    #[test]
    fn default_is_empty() {
        let map = MeterMap::default();
        assert_eq!(map.id_for_name("anything"), None);
        assert_eq!(map.name_for_id(0), None);
    }
}
