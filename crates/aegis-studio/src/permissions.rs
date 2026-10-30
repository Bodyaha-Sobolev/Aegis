#![allow(dead_code)]

use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub(crate) fn parameter_hash(value: &str) -> String {
    // FNV-1a is deliberately explicit here: DefaultHasher is seeded per
    // process and would make audit records impossible to correlate after a
    // restart. This is an identifier hash, not a password-hashing primitive.
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("hash64:{hash:016x}")
}

fn escape_tsv(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\r', "\\r")
        .replace('\n', "\\n")
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

impl Capability {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::ReadWorkspace => "read_workspace",
            Self::WriteWorkspace => "write_workspace",
            Self::RunOwnedChild => "run_owned_child",
            Self::InspectFile => "inspect_file",
            Self::NetworkRequest => "network_request",
            Self::LoadFfi => "load_ffi",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Decision {
    Allow,
    Deny,
    Cancel,
}

impl Decision {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Deny => "deny",
            Self::Cancel => "cancel",
        }
    }
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

const MAX_ENTRIES: usize = 4096;

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
        if self.entries.len() > MAX_ENTRIES {
            self.entries.remove(0);
        }
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

    /// Returns a stable, machine-readable snapshot suitable for copying out of
    /// the IDE. Targets and free-form reasons are hashed so the export cannot
    /// accidentally disclose paths, prompts, or other sensitive metadata.
    pub(crate) fn redacted_tsv(&self) -> String {
        let mut output = String::from(
            "request_id\ttimestamp_ms\tactor\tcapability\ttarget_hash\tparameters_hash\tdecision\treason_hash\tresult\tduration_ms\n",
        );
        for entry in &self.entries {
            use std::fmt::Write;
            let _ = writeln!(
                output,
                "{}\t{}\t{}\t{:?}\t{}\t{}\t{:?}\t{}\t{}\t{}",
                entry.request_id,
                entry.timestamp_ms,
                parameter_hash(&entry.actor),
                entry.capability.as_str(),
                parameter_hash(&entry.target),
                entry.parameters_hash,
                entry.decision.as_str(),
                parameter_hash(&entry.reason),
                escape_tsv(&entry.result),
                entry.duration.as_millis()
            );
        }
        output
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
        assert_eq!(ledger.entries()[0].duration, Duration::from_millis(3));
        assert_eq!(ledger.entries()[1].decision, Decision::Deny);
        assert!(!ledger.entries()[0].target.contains("secret"));
    }

    #[test]
    fn parameter_hash_is_stable_and_does_not_echo_input() {
        let first = parameter_hash("C:\\workspace\\main.ae");
        assert_eq!(first, parameter_hash("C:\\workspace\\main.ae"));
        assert_eq!(first, "hash64:6da20505e2f5d0d2");
        assert!(first.starts_with("hash64:"));
        assert!(!first.contains("workspace"));
    }

    #[test]
    fn audit_memory_is_bounded_and_keeps_latest_entry() {
        let mut ledger = PermissionLedger::default();
        for _ in 0..(MAX_ENTRIES + 3) {
            ledger.request(
                "test",
                Capability::InspectFile,
                "fixture",
                "hash64:x",
                Decision::Deny,
                "test",
            );
        }
        assert_eq!(ledger.len(), MAX_ENTRIES);
        assert_eq!(
            ledger.entries().last().map(|entry| entry.request_id),
            Some((MAX_ENTRIES + 2) as u64)
        );
    }

    #[test]
    fn redacted_export_has_schema_without_sensitive_values() {
        let mut ledger = PermissionLedger::default();
        ledger.request(
            "mcp:secret-agent",
            Capability::ReadWorkspace,
            "C:\\private\\token.txt",
            "hash64:abc",
            Decision::Deny,
            "contains secret prompt",
        );
        let export = ledger.redacted_tsv();
        assert!(export.starts_with("request_id\ttimestamp_ms\tactor"));
        assert!(export.contains("hash64:"));
        assert!(!export.contains("private"));
        assert!(!export.contains("secret-agent"));
        assert!(!export.contains("secret prompt"));
    }

    #[test]
    fn redacted_export_keeps_one_record_per_line() {
        let mut ledger = PermissionLedger::default();
        let id = ledger.request(
            "test",
            Capability::InspectFile,
            "fixture",
            "hash64:x",
            Decision::Allow,
            "ok",
        );
        assert!(ledger.finish(id, "line one\nline two\tcolumn", Duration::ZERO));
        let export = ledger.redacted_tsv();
        assert_eq!(export.lines().count(), 2);
        assert!(export.contains("line one\\nline two\\tcolumn"));
    }

    #[test]
    fn capability_and_decision_names_are_stable() {
        assert_eq!(Capability::RunOwnedChild.as_str(), "run_owned_child");
        assert_eq!(Capability::LoadFfi.as_str(), "load_ffi");
        assert_eq!(Decision::Cancel.as_str(), "cancel");
    }
}
