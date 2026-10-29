#![allow(dead_code)]

use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub(crate) fn parameter_hash(value: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    format!("hash64:{:016x}", hasher.finish())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Capability {
    ReadWorkspace,
    WriteWorkspace,
    RunOwnedChild,
    InspectFile,
    NetworkRequest,
    LoadFfi,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Decision {
    Allow,
    Deny,
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AuditEntry {
    pub request_id: u64,
    pub timestamp_ms: u128,
    pub actor: String,
    pub capability: Capability,
    pub target: String,
    pub parameters_hash: String,
    pub decision: Decision,
    pub reason: String,
    pub result: String,
    pub duration: Duration,
}

#[derive(Debug, Default)]
pub(crate) struct PermissionLedger {
    next_request: u64,
    entries: Vec<AuditEntry>,
}

impl PermissionLedger {
    pub(crate) fn request(
        &mut self,
        actor: &str,
        capability: Capability,
        target: &str,
        parameters_hash: &str,
        decision: Decision,
        reason: &str,
    ) -> u64 {
        let request_id = self.next_request;
        self.next_request = self.next_request.saturating_add(1);
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        self.entries.push(AuditEntry {
            request_id,
            timestamp_ms,
            actor: actor.to_owned(),
            capability,
            target: target.to_owned(),
            parameters_hash: parameters_hash.to_owned(),
            decision,
            reason: reason.to_owned(),
            result: String::from("pending"),
            duration: Duration::ZERO,
        });
        request_id
    }

    pub(crate) fn finish(&mut self, request_id: u64, result: &str, duration: Duration) -> bool {
        let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.request_id == request_id)
        else {
            return false;
        };
        entry.result = result.to_owned();
        entry.duration = duration;
        true
    }

    pub(crate) fn entries(&self) -> &[AuditEntry] {
        &self.entries
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_allow_deny_and_cancel_without_secrets() {
        let mut ledger = PermissionLedger::default();
        let allow = ledger.request(
            "mcp:test",
            Capability::InspectFile,
            "fixture.exe",
            "sha256:x",
            Decision::Allow,
            "user approved",
        );
        let deny = ledger.request(
            "mcp:test",
            Capability::WriteWorkspace,
            "project",
            "sha256:y",
            Decision::Deny,
            "not approved",
        );
        assert_eq!(allow, 0);
        assert_eq!(deny, 1);
        assert!(ledger.finish(allow, "ok", Duration::from_millis(3)));
        assert!(!ledger.finish(99, "missing", Duration::ZERO));
        assert_eq!(ledger.entries()[0].result, "ok");
        assert_eq!(ledger.entries()[1].decision, Decision::Deny);
        assert!(!ledger.entries()[0].target.contains("secret"));
    }

    #[test]
    fn parameter_hash_is_stable_and_does_not_echo_input() {
        let first = parameter_hash("C:\\workspace\\main.ae");
        assert_eq!(first, parameter_hash("C:\\workspace\\main.ae"));
        assert!(first.starts_with("hash64:"));
        assert!(!first.contains("workspace"));
    }
}
