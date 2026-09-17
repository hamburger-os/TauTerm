//! Canonical TauTerm log-segment file naming.
//!
//! Every log file starts with the same sortable local timestamp key. PID, session identity,
//! nonce and segment index provide uniqueness but never precede the chronological key, so a normal
//! filename sort matches segment creation order across system/session logs and process instances.

use chrono::{DateTime, Local};

fn timestamp_key(timestamp: &DateTime<Local>) -> String {
    timestamp.format("%Y%m%d_%H%M%S_%3f").to_string()
}

pub fn system_segment_file_name(timestamp: &DateTime<Local>, segment_index: u32) -> String {
    format!(
        "TauTerm_{}_system_p{}_{segment_index:03}.log",
        timestamp_key(timestamp),
        std::process::id()
    )
}

pub fn session_segment_file_name(
    timestamp: &DateTime<Local>,
    session_key: &str,
    nonce: &str,
    segment_index: u32,
) -> String {
    format!(
        "TauTerm_{}_session_{session_key}_p{}_{}_{segment_index:04}.log",
        timestamp_key(timestamp),
        std::process::id(),
        nonce
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone};

    #[test]
    fn chronological_prefix_sorts_by_segment_creation_time() {
        let first = Local
            .timestamp_millis_opt(1_800_000_000_001)
            .single()
            .unwrap();
        let second = first + Duration::milliseconds(1);

        let first_name = system_segment_file_name(&first, 999);
        let second_name = system_segment_file_name(&second, 0);
        assert!(first_name < second_name);
    }

    #[test]
    fn system_and_session_share_the_same_timestamp_prefix_contract() {
        let timestamp = Local
            .timestamp_millis_opt(1_800_000_000_123)
            .single()
            .unwrap();
        let prefix = format!("TauTerm_{}_", timestamp_key(&timestamp));
        assert!(system_segment_file_name(&timestamp, 0).starts_with(&prefix));
        assert!(session_segment_file_name(&timestamp, "abc", "deadbeef", 0).starts_with(&prefix));
    }
}
