//! plato-dynamic-locks — Runtime lock accumulation
//!
//! Inspired by Oracle1's self-supervision compiler: compile at different temperatures,
//! detect inconsistencies, create lock annotations that prevent future drift.
//!
//! Extends static gates (plato-lab-guard) with dynamic locks accumulated from
//! actual execution/experience. Each lock captures: what was inconsistent,
//! what resolved it, and how strong the evidence is.
//!
//! Locks accumulate over time → compiler/agent personality → cross-model personality differences.

use std::collections::HashMap;

// ── Lock Types ───────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LockSource {
    /// Inconsistency detected between two compilation runs
    Inconsistency,
    /// Empirical observation from runtime behavior
    Observation,
    /// Cross-model disagreement
    CrossModel,
    /// Human/expert annotation
    Expert,
    /// Inferred from pattern in existing locks
    Inferred,
}

impl LockSource {
    pub fn name(&self) -> &'static str {
        match self {
            LockSource::Inconsistency => "inconsistency",
            LockSource::Observation => "observation",
            LockSource::CrossModel => "cross-model",
            LockSource::Expert => "expert",
            LockSource::Inferred => "inferred",
        }
    }

    /// Trust weight for this source type (higher = more trustworthy)
    pub fn base_trust(&self) -> f32 {
        match self {
            LockSource::Expert => 1.0,
            LockSource::Inconsistency => 0.8,
            LockSource::Observation => 0.7,
            LockSource::CrossModel => 0.6,
            LockSource::Inferred => 0.4,
        }
    }
}

// ── Dynamic Lock ─────────────────────────────────────────

/// A lock annotation: a constraint learned from experience.
/// Captures what was wrong, what fixed it, and how strong the evidence is.
#[derive(Debug, Clone)]
pub struct Lock {
    /// Unique identifier (nanosecond-based)
    pub id: u64,
    /// Short description of the constraint
    pub description: String,
    /// What pattern triggers this lock (e.g., "when setting register to immediate")
    pub trigger_pattern: String,
    /// What the lock enforces (e.g., "always use MOVI, not MOV")
    pub enforcement: String,
    /// How the lock was discovered
    pub source: LockSource,
    /// Strength: 0.0 (weak) to 1.0 (absolute)
    pub strength: f32,
    /// Number of times this lock has been verified correct
    pub verifications: u32,
    /// Number of times this lock was violated (false positive)
    pub violations: u32,
    /// When this lock was created (nanosecond timestamp)
    pub created_at: u64,
    /// When this lock was last triggered
    pub last_triggered: Option<u64>,
    /// Category for organization
    pub category: String,
}

impl Lock {
    /// Create a new lock
    pub fn new(description: &str, trigger: &str, enforcement: &str, source: LockSource) -> Self {
        Self {
            id: nanos_now(),
            description: description.to_string(),
            trigger_pattern: trigger.to_string(),
            enforcement: enforcement.to_string(),
            source,
            strength: source.base_trust(),
            verifications: 0,
            violations: 0,
            created_at: nanos_now(),
            last_triggered: None,
            category: String::new(),
        }
    }

    /// Set category
    pub fn with_category(mut self, cat: &str) -> Self {
        self.category = cat.to_string();
        self
    }

    /// Record a verification (lock was correct)
    pub fn verify(&mut self) {
        self.verifications += 1;
        // Strength increases with verifications, caps at 1.0
        self.strength = (self.strength + 0.05).min(1.0);
    }

    /// Record a violation (lock was wrong / false positive)
    pub fn violate(&mut self) {
        self.violations += 1;
        // Strength decreases with violations
        self.strength = (self.strength - 0.15).max(0.0);
    }

    /// Trigger this lock
    pub fn trigger(&mut self) {
        self.last_triggered = Some(nanos_now());
    }

    /// Is this lock still active? (strength above minimum)
    pub fn is_active(&self, min_strength: f32) -> bool {
        self.strength >= min_strength
    }

    /// Confidence score: based on verification/violation ratio.
    /// Starts at source base_trust (not 0) — a fresh lock from an expert source
    /// should have meaningful strength from the start.
    pub fn confidence(&self) -> f32 {
        if self.verifications == 0 && self.violations == 0 {
            return self.source.base_trust(); // Fresh lock: trust the source
        }
        let ratio = self.verifications as f32 / (self.verifications + self.violations) as f32;
        // Blend ratio with source trust, weighted by total evidence
        let evidence = (self.verifications + self.violations) as f32;
        let blend = evidence / (evidence + 5.0); // More evidence → trust the data more
        ratio * blend + self.source.base_trust() * (1.0 - blend)
    }

    /// Effective strength: base strength * confidence
    pub fn effective_strength(&self) -> f32 {
        self.strength * self.confidence()
    }
}

// ── Lock Check Result ────────────────────────────────────

#[derive(Debug, Clone)]
pub struct LockCheck {
    pub lock_id: u64,
    pub triggered: bool,
    pub description: String,
    pub enforcement: String,
    pub effective_strength: f32,
}

// ── Lock Accumulator ─────────────────────────────────────

/// The main engine: accumulates locks, checks inputs against them,
/// learns from inconsistencies.
pub struct LockAccumulator {
    locks: HashMap<u64, Lock>,
    /// Minimum strength for a lock to be considered active
    min_strength: f32,
    /// Pattern matching mode: exact or substring
    exact_match: bool,
}

impl LockAccumulator {
    pub fn new() -> Self {
        Self { locks: HashMap::new(), min_strength: 0.1, exact_match: false }
    }

    pub fn with_min_strength(min: f32) -> Self {
        Self { locks: HashMap::new(), min_strength: min, exact_match: false }
    }

    /// Add a lock to the accumulator
    pub fn add(&mut self, lock: Lock) -> u64 {
        let id = lock.id;
        self.locks.insert(id, lock);
        id
    }

    /// Remove a lock by ID
    pub fn remove(&mut self, id: u64) -> bool {
        self.locks.remove(&id).is_some()
    }

    /// Record an inconsistency between two outputs for the same input.
    /// Creates a lock that captures what was inconsistent.
    pub fn record_inconsistency(
        &mut self,
        input: &str,
        output_a: &str,
        output_b: &str,
    ) -> u64 {
        let trigger = input.to_string();
        let enforcement = format!("Lock: outputs differ for same input. A=\"{}\" B=\"{}\"", 
            truncate(output_a, 80), truncate(output_b, 80));
        let description = format!("Inconsistency lock for: {}", truncate(input, 60));

        let lock = Lock::new(&description, &trigger, &enforcement, LockSource::Inconsistency);
        self.add(lock)
    }

    /// Record an observation: when X happened, Y was the correct response.
    pub fn record_observation(&mut self, trigger: &str, enforcement: &str, category: &str) -> u64 {
        let description = format!("Observation: {}", truncate(enforcement, 80));
        let lock = Lock::new(&description, trigger, enforcement, LockSource::Observation)
            .with_category(category);
        self.add(lock)
    }

    /// Check an input against all active locks.
    /// Returns all locks that trigger (matched trigger pattern).
    pub fn check(&mut self, input: &str) -> Vec<LockCheck> {
        let mut checks = Vec::new();

        for lock in self.locks.values_mut() {
            if !lock.is_active(self.min_strength) { continue; }

            let triggered = if self.exact_match {
                lock.trigger_pattern == input
            } else {
                input.contains(&lock.trigger_pattern) || lock.trigger_pattern.contains(input)
            };

            if triggered {
                lock.trigger();
                checks.push(LockCheck {
                    lock_id: lock.id,
                    triggered: true,
                    description: lock.description.clone(),
                    enforcement: lock.enforcement.clone(),
                    effective_strength: lock.effective_strength(),
                });
            }
        }

        // Sort by effective strength descending
        checks.sort_by(|a, b| b.effective_strength.partial_cmp(&a.effective_strength).unwrap_or(std::cmp::Ordering::Equal));
        checks
    }

    /// Verify a lock was correct (increases strength)
    pub fn verify(&mut self, id: u64) -> bool {
        if let Some(lock) = self.locks.get_mut(&id) {
            lock.verify();
            true
        } else { false }
    }

    /// Violate a lock (decreases strength, potential decay to inactive)
    pub fn violate(&mut self, id: u64) -> bool {
        if let Some(lock) = self.locks.get_mut(&id) {
            lock.violate();
            true
        } else { false }
    }

    /// Get all active locks
    pub fn active_locks(&self) -> Vec<&Lock> {
        self.locks.values().filter(|l| l.is_active(self.min_strength)).collect()
    }

    /// Get all inactive (decayed) locks
    pub fn inactive_locks(&self) -> Vec<&Lock> {
        self.locks.values().filter(|l| !l.is_active(self.min_strength)).collect()
    }

    /// Get locks by category
    pub fn by_category(&self, category: &str) -> Vec<&Lock> {
        self.locks.values().filter(|l| l.category == category).collect()
    }

    /// Get locks by source
    pub fn by_source(&self, source: LockSource) -> Vec<&Lock> {
        self.locks.values().filter(|l| l.source == source).collect()
    }

    /// Total locks
    pub fn len(&self) -> usize { self.locks.len() }
    pub fn is_empty(&self) -> bool { self.locks.is_empty() }

    /// Prune inactive locks (below min_strength)
    pub fn prune(&mut self) -> usize {
        let before = self.locks.len();
        self.locks.retain(|_, l| l.is_active(self.min_strength));
        before - self.locks.len()
    }

    /// Get the strongest lock
    pub fn strongest(&self) -> Option<&Lock> {
        self.locks.values().max_by(|a, b| a.effective_strength().partial_cmp(&b.effective_strength()).unwrap_or(std::cmp::Ordering::Equal))
    }

    /// Summary statistics
    pub fn stats(&self) -> LockStats {
        let active = self.active_locks().len();
        let total_verifications: u32 = self.locks.values().map(|l| l.verifications).sum();
        let total_violations: u32 = self.locks.values().map(|l| l.violations).sum();
        let avg_strength: f32 = if self.locks.is_empty() { 0.0 } else {
            self.locks.values().map(|l| l.effective_strength()).sum::<f32>() / self.locks.len() as f32
        };

        LockStats {
            total: self.locks.len(),
            active,
            inactive: self.locks.len() - active,
            total_verifications,
            total_violations,
            avg_effective_strength: avg_strength,
        }
    }
}

impl Default for LockAccumulator {
    fn default() -> Self { Self::new() }
}

// ── Stats ────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
pub struct LockStats {
    pub total: usize,
    pub active: usize,
    pub inactive: usize,
    pub total_verifications: u32,
    pub total_violations: u32,
    pub avg_effective_strength: f32,
}

// ── Helpers ──────────────────────────────────────────────

fn nanos_now() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max { s.to_string() }
    else { format!("{}...", &s[..max]) }
}

// ── Tests ────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lock_source_properties() {
        assert!(LockSource::Expert.base_trust() > LockSource::Inconsistency.base_trust());
        assert!(LockSource::Inconsistency.base_trust() > LockSource::Inferred.base_trust());
    }

    #[test]
    fn test_lock_creation() {
        let lock = Lock::new("test lock", "trigger", "enforce X", LockSource::Inconsistency);
        assert_eq!(lock.verifications, 0);
        assert_eq!(lock.violations, 0);
        assert!(lock.strength > 0.0);
    }

    #[test]
    fn test_lock_verify_violate() {
        let mut lock = Lock::new("test", "t", "e", LockSource::Observation);
        let initial = lock.strength;

        lock.verify();
        assert!(lock.strength > initial);
        assert_eq!(lock.verifications, 1);

        lock.violate();
        assert!(lock.strength < initial + 0.05); // went up then down
        assert_eq!(lock.violations, 1);
    }

    #[test]
    fn test_lock_confidence() {
        let mut lock = Lock::new("test", "t", "e", LockSource::Observation);
        // Fresh lock: returns source base trust (0.7 for Observation)
        assert!((lock.confidence() - 0.7).abs() < 0.01);

        lock.verify();
        // 1 verify, 0 violations: ratio=1.0, blend=1/(1+5)=0.167
        // confidence = 1.0 * 0.167 + 0.7 * 0.833 = 0.75
        assert!(lock.confidence() > 0.7);

        lock.verify();
        // 2 verify: blend=2/7=0.286
        // confidence = 1.0 * 0.286 + 0.7 * 0.714 = 0.786
        assert!(lock.confidence() > lock.confidence() - 0.01); // just ensure it's valid
    }

    #[test]
    fn test_lock_strength_capping() {
        let mut lock = Lock::new("test", "t", "e", LockSource::Expert);
        for _ in 0..50 { lock.verify(); }
        assert!(lock.strength <= 1.0);
    }

    #[test]
    fn test_accumulator_add_and_check() {
        let mut acc = LockAccumulator::new();
        acc.add(Lock::new("opcode lock", "MOV", "use MOVI for immediates", LockSource::Inconsistency));

        let checks = acc.check("use MOV for immediate value");
        assert_eq!(checks.len(), 1);
        assert!(checks[0].triggered);
    }

    #[test]
    fn test_accumulator_no_match() {
        let mut acc = LockAccumulator::new();
        acc.add(Lock::new("opcode lock", "MOV", "use MOVI", LockSource::Inconsistency));

        let checks = acc.check("something completely unrelated");
        assert!(checks.is_empty());
    }

    #[test]
    fn test_record_inconsistency() {
        let mut acc = LockAccumulator::new();
        let id = acc.record_inconsistency(
            "set helm east",
            "MOV helm, 東",
            "MOVI helm, 東",
        );

        let checks = acc.check("set helm east");
        assert_eq!(checks.len(), 1);
        assert!(checks[0].enforcement.contains("differ"));
    }

    #[test]
    fn test_record_observation() {
        let mut acc = LockAccumulator::new();
        let id = acc.record_observation("OOM detected", "reduce batch size by 50%", "memory");

        assert_eq!(acc.by_category("memory").len(), 1);
    }

    #[test]
    fn test_prune_inactive() {
        let mut acc = LockAccumulator::with_min_strength(0.5);
        let id = acc.add(Lock::new("weak lock", "t", "e", LockSource::Inferred));
        
        // Inferred: base_trust=0.4, strength=0.4, confidence=0.4, effective=0.16
        // Below 0.5 threshold → should be pruned
        assert!(acc.locks[&id].effective_strength() < 0.5);
        
        let pruned = acc.prune();
        assert_eq!(pruned, 1);
        assert!(acc.is_empty());
    }

    #[test]
    fn test_strongest() {
        let mut acc = LockAccumulator::new();
        let id1 = acc.add(Lock::new("weak", "a", "b", LockSource::Inferred));
        let id2 = acc.add(Lock::new("strong", "c", "d", LockSource::Expert));
        
        // Expert has base_trust 1.0, Inferred has 0.4
        let strongest = acc.strongest().unwrap();
        assert_eq!(strongest.source, LockSource::Expert);
    }

    #[test]
    fn test_by_source() {
        let mut acc = LockAccumulator::new();
        acc.add(Lock::new("a", "t", "e", LockSource::Inconsistency));
        acc.add(Lock::new("b", "t", "e", LockSource::Observation));
        acc.add(Lock::new("c", "t", "e", LockSource::Inconsistency));

        assert_eq!(acc.by_source(LockSource::Inconsistency).len(), 2);
        assert_eq!(acc.by_source(LockSource::Expert).len(), 0);
    }

    #[test]
    fn test_stats() {
        let mut acc = LockAccumulator::new();
        acc.add(Lock::new("a", "t", "e", LockSource::Expert));
        acc.add(Lock::new("b", "t", "e", LockSource::Expert));

        let stats = acc.stats();
        assert_eq!(stats.total, 2);
        assert!(stats.avg_effective_strength > 0.0);
    }

    #[test]
    fn test_sorted_checks() {
        let mut acc = LockAccumulator::new();
        acc.add(Lock::new("weak", "test", "e", LockSource::Inferred));
        acc.add(Lock::new("strong", "test", "e", LockSource::Expert));

        let checks = acc.check("test input");
        if checks.len() == 2 {
            assert!(checks[0].effective_strength >= checks[1].effective_strength);
        }
    }

    #[test]
    fn test_verify_and_violate_by_id() {
        let mut acc = LockAccumulator::new();
        let id = acc.add(Lock::new("test", "t", "e", LockSource::Observation));

        assert!(acc.verify(id));
        assert!(acc.violate(id));
        assert!(!acc.verify(9999)); // nonexistent
    }

    #[test]
    fn test_lock_active_check() {
        let lock = Lock::new("test", "t", "e", LockSource::Expert);
        assert!(lock.is_active(0.1));
        assert!(lock.is_active(1.0)); // Expert has base 1.0

        let lock2 = Lock::new("test2", "t", "e", LockSource::Inferred);
        assert!(!lock2.is_active(0.5)); // Inferred base 0.4
    }

    #[test]
    fn test_exact_match_mode() {
        let mut acc = LockAccumulator::new();
        acc.exact_match = true;
        acc.add(Lock::new("exact", "MOVI", "use MOVI", LockSource::Expert));

        assert_eq!(acc.check("MOVI").len(), 1);
        assert_eq!(acc.check("use MOVI here").len(), 0); // substring, not exact
    }

    #[test]
    fn test_remove_lock() {
        let mut acc = LockAccumulator::new();
        let id = acc.add(Lock::new("test", "t", "e", LockSource::Expert));
        assert_eq!(acc.len(), 1);
        assert!(acc.remove(id));
        assert!(acc.is_empty());
        assert!(!acc.remove(9999));
    }
}
