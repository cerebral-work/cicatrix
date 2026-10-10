//! Multi-replica version-vector Frontier engine (CER-2754, Phase 1.2).
//!
//! Provides causality tracking and temporal query evaluation across distributed agent
//! replicas using Lamport timestamps and version vectors.
//!
//! Lineage: `wbrown/janus-datalog`.
//! Invariants:
//! - Monotonically increasing Lamport clocks per replica identifier.
//! - Non-zero clock values (`:db/neverZeroValue`).
//! - Causal dominance: $A \ge B \iff \forall r \in \text{dom}(B), A(r) \ge B(r)$.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

/// A replica identifier in the distributed agent mesh.
pub type ReplicaId = String;

/// A monotonically increasing Lamport logical clock.
pub type LamportTimestamp = u64;

/// Error type for Frontier parsing and validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrontierError {
    EmptyString,
    EmptyReplica,
    ZeroTimestamp { replica: String },
    InvalidFormat(String),
    ParseIntError(String),
}

impl fmt::Display for FrontierError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FrontierError::EmptyString => {
                write!(
                    f,
                    ":db/neverZeroValue violation: frontier string is empty or whitespace"
                )
            }
            FrontierError::EmptyReplica => {
                write!(
                    f,
                    ":db/neverZeroValue violation: replica identifier cannot be empty"
                )
            }
            FrontierError::ZeroTimestamp { replica } => {
                write!(
                    f,
                    ":db/neverZeroValue violation: replica `{replica}` has timestamp 0 (must be > 0)"
                )
            }
            FrontierError::InvalidFormat(msg) => write!(f, "malformed frontier format: {msg}"),
            FrontierError::ParseIntError(msg) => write!(f, "failed to parse timestamp: {msg}"),
        }
    }
}

impl std::error::Error for FrontierError {}

/// A single replica point in causal time.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct FrontierStamp {
    pub replica: ReplicaId,
    pub lamport: LamportTimestamp,
}

impl FrontierStamp {
    pub fn new(
        replica: impl Into<String>,
        lamport: LamportTimestamp,
    ) -> Result<Self, FrontierError> {
        let rep = replica.into();
        let trimmed_rep = rep.trim();
        if trimmed_rep.is_empty() {
            return Err(FrontierError::EmptyReplica);
        }
        if lamport == 0 {
            return Err(FrontierError::ZeroTimestamp {
                replica: trimmed_rep.to_string(),
            });
        }
        Ok(Self {
            replica: trimmed_rep.to_string(),
            lamport,
        })
    }

    /// Parse a single stamp formatted as `replica:timestamp` (e.g. `nodeA:42`).
    pub fn parse(s: &str) -> Result<Self, FrontierError> {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return Err(FrontierError::EmptyString);
        }
        let (rep, ts_str) = trimmed
            .split_once(':')
            .or_else(|| trimmed.split_once('@'))
            .ok_or_else(|| {
                FrontierError::InvalidFormat(format!(
                    "expected `replica:timestamp` or `replica@timestamp`, found `{trimmed}`"
                ))
            })?;

        let rep = rep.trim();
        if rep.is_empty() {
            return Err(FrontierError::EmptyReplica);
        }
        let ts = ts_str
            .trim()
            .parse::<u64>()
            .map_err(|e| FrontierError::ParseIntError(e.to_string()))?;

        if ts == 0 {
            return Err(FrontierError::ZeroTimestamp {
                replica: rep.to_string(),
            });
        }
        Ok(Self {
            replica: rep.to_string(),
            lamport: ts,
        })
    }
}

impl fmt::Display for FrontierStamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.replica, self.lamport)
    }
}

impl FromStr for FrontierStamp {
    type Err = FrontierError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

/// Multi-replica version-vector representing a causal frontier cut across the estate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Frontier {
    pub clocks: BTreeMap<ReplicaId, LamportTimestamp>,
}

impl Frontier {
    /// Create a new empty frontier representing the causal genesis origin.
    pub fn new() -> Self {
        Self {
            clocks: BTreeMap::new(),
        }
    }

    /// Create a frontier from an existing map of clocks.
    pub fn from_map(clocks: BTreeMap<ReplicaId, LamportTimestamp>) -> Result<Self, FrontierError> {
        for (rep, &ts) in &clocks {
            if rep.trim().is_empty() {
                return Err(FrontierError::EmptyReplica);
            }
            if ts == 0 {
                return Err(FrontierError::ZeroTimestamp {
                    replica: rep.clone(),
                });
            }
        }
        Ok(Self { clocks })
    }

    /// True if the frontier contains no recorded replica timestamps.
    pub fn is_empty(&self) -> bool {
        self.clocks.is_empty()
    }

    /// Number of tracked replicas in the frontier.
    pub fn len(&self) -> usize {
        self.clocks.len()
    }

    /// Get the logical timestamp recorded for a given replica, or 0 if unobserved.
    pub fn get(&self, replica: &str) -> LamportTimestamp {
        self.clocks.get(replica).copied().unwrap_or(0)
    }

    /// Record or advance the timestamp for a replica.
    pub fn set(
        &mut self,
        replica: impl Into<String>,
        lamport: LamportTimestamp,
    ) -> Result<(), FrontierError> {
        let rep = replica.into();
        let trimmed = rep.trim().to_string();
        if trimmed.is_empty() {
            return Err(FrontierError::EmptyReplica);
        }
        if lamport == 0 {
            return Err(FrontierError::ZeroTimestamp { replica: trimmed });
        }
        self.clocks.insert(trimmed, lamport);
        Ok(())
    }

    /// Advance the timestamp for a replica to the maximum of its current and new value.
    pub fn advance(
        &mut self,
        replica: impl Into<String>,
        lamport: LamportTimestamp,
    ) -> Result<(), FrontierError> {
        let rep = replica.into();
        let trimmed = rep.trim().to_string();
        if trimmed.is_empty() {
            return Err(FrontierError::EmptyReplica);
        }
        if lamport == 0 {
            return Err(FrontierError::ZeroTimestamp { replica: trimmed });
        }
        let cur = self.clocks.entry(trimmed).or_insert(0);
        *cur = (*cur).max(lamport);
        Ok(())
    }

    /// Returns true if this frontier causally includes (dominates or equals) `stamp`.
    pub fn contains_stamp(&self, stamp: &FrontierStamp) -> bool {
        self.get(&stamp.replica) >= stamp.lamport
    }

    /// Returns true if this frontier causally dominates `other` ($self \ge other$).
    ///
    /// By definition, $A \ge B \iff \forall r \in \text{dom}(B), A(r) \ge B(r)$.
    /// An empty frontier represents the root genesis, which is dominated by any frontier.
    pub fn dominates(&self, other: &Frontier) -> bool {
        other
            .clocks
            .iter()
            .all(|(replica, &ts)| self.get(replica) >= ts)
    }

    /// Merge another frontier into this one using component-wise pairwise maximum.
    pub fn merge(&mut self, other: &Frontier) {
        for (rep, &ts) in &other.clocks {
            let cur = self.clocks.entry(rep.clone()).or_insert(0);
            *cur = (*cur).max(ts);
        }
    }

    /// Validate that the frontier satisfies `:db/neverZeroValue`:
    /// It must contain at least one replica clock, replica names must not be empty/whitespace,
    /// and every Lamport timestamp must be greater than zero.
    pub fn validate_never_zero(&self) -> Result<(), FrontierError> {
        if self.clocks.is_empty() {
            return Err(FrontierError::EmptyString);
        }
        for (rep, &ts) in &self.clocks {
            if rep.trim().is_empty() {
                return Err(FrontierError::EmptyReplica);
            }
            if ts == 0 {
                return Err(FrontierError::ZeroTimestamp {
                    replica: rep.clone(),
                });
            }
        }
        Ok(())
    }

    /// Parse a version vector from string.
    ///
    /// Accepted formats:
    /// - Comma-separated pairs: `nodeA:10,nodeB:20` or `nodeA@10, nodeB@20`
    /// - JSON object: `{"nodeA": 10, "nodeB": 20}`
    /// - Single scalar integer: `42` (assigned to replica `"default"`)
    pub fn parse(s: &str) -> Result<Self, FrontierError> {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return Err(FrontierError::EmptyString);
        }

        // Try JSON object parsing first
        if trimmed.starts_with('{') && trimmed.ends_with('}') {
            let map: BTreeMap<String, u64> = serde_json::from_str(trimmed)
                .map_err(|e| FrontierError::InvalidFormat(format!("invalid JSON frontier: {e}")))?;
            return Self::from_map(map);
        }

        // Try single integer scalar (e.g. "42" -> default:42)
        if let Ok(val) = trimmed.parse::<u64>() {
            if val == 0 {
                return Err(FrontierError::ZeroTimestamp {
                    replica: "default".into(),
                });
            }
            let mut clocks = BTreeMap::new();
            clocks.insert("default".to_string(), val);
            return Ok(Self { clocks });
        }

        // Parse comma-separated list of `replica:timestamp` pairs
        let mut clocks = BTreeMap::new();
        for item in trimmed.split(',') {
            let item_trimmed = item.trim();
            if item_trimmed.is_empty() {
                continue;
            }
            let stamp = FrontierStamp::parse(item_trimmed)?;
            let cur = clocks.entry(stamp.replica).or_insert(0);
            *cur = (*cur).max(stamp.lamport);
        }

        if clocks.is_empty() {
            return Err(FrontierError::InvalidFormat(
                "no valid replica:timestamp pairs found in frontier string".into(),
            ));
        }

        Ok(Self { clocks })
    }
}

impl fmt::Display for Frontier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.clocks.is_empty() {
            return write!(f, "(empty)");
        }
        let pairs: Vec<String> = self
            .clocks
            .iter()
            .map(|(rep, ts)| format!("{rep}:{ts}"))
            .collect();
        write!(f, "{}", pairs.join(","))
    }
}

impl FromStr for Frontier {
    type Err = FrontierError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_comma_separated_pairs() {
        let f = Frontier::parse("ceres:42, cygnus:18").unwrap();
        assert_eq!(f.get("ceres"), 42);
        assert_eq!(f.get("cygnus"), 18);
        assert_eq!(f.get("unknown"), 0);
        assert_eq!(f.to_string(), "ceres:42,cygnus:18");
    }

    #[test]
    fn parse_json_object() {
        let f = Frontier::parse(r#"{"ceres": 100, "nodeB": 50}"#).unwrap();
        assert_eq!(f.get("ceres"), 100);
        assert_eq!(f.get("nodeB"), 50);
        assert_eq!(f.len(), 2);
    }

    #[test]
    fn parse_single_scalar_int() {
        let f = Frontier::parse("10").unwrap();
        assert_eq!(f.get("default"), 10);
        assert_eq!(f.len(), 1);
    }

    #[test]
    fn never_zero_value_enforcement() {
        // Empty string is rejected
        assert!(matches!(
            Frontier::parse(""),
            Err(FrontierError::EmptyString)
        ));
        assert!(matches!(
            Frontier::parse("   "),
            Err(FrontierError::EmptyString)
        ));

        // Zero timestamp is rejected
        assert!(matches!(
            Frontier::parse("nodeA:0"),
            Err(FrontierError::ZeroTimestamp { .. })
        ));
        assert!(matches!(
            Frontier::parse("0"),
            Err(FrontierError::ZeroTimestamp { .. })
        ));

        // Empty replica name is rejected
        assert!(matches!(
            Frontier::parse(":42"),
            Err(FrontierError::EmptyReplica)
        ));
    }

    #[test]
    fn causal_dominance() {
        let f_high = Frontier::parse("nodeA:10, nodeB:20").unwrap();
        let f_low = Frontier::parse("nodeA:5, nodeB:20").unwrap();
        let f_concurrent = Frontier::parse("nodeA:12, nodeB:15").unwrap();
        let f_empty = Frontier::new();

        // Dominance is reflexive
        assert!(f_high.dominates(&f_high));
        assert!(f_low.dominates(&f_low));
        assert!(f_empty.dominates(&f_empty));

        // Higher dominates lower
        assert!(f_high.dominates(&f_low));
        assert!(!f_low.dominates(&f_high));

        // Genesis is dominated by everything
        assert!(f_high.dominates(&f_empty));
        assert!(f_low.dominates(&f_empty));
        assert!(!f_empty.dominates(&f_high));

        // Concurrent vectors do not dominate each other
        assert!(!f_high.dominates(&f_concurrent));
        assert!(!f_concurrent.dominates(&f_high));
    }

    #[test]
    fn stamp_containment() {
        let f = Frontier::parse("nodeA:10, nodeB:20").unwrap();
        let s1 = FrontierStamp::new("nodeA", 10).unwrap();
        let s2 = FrontierStamp::new("nodeA", 5).unwrap();
        let s3 = FrontierStamp::new("nodeA", 15).unwrap();
        let s4 = FrontierStamp::new("nodeC", 1).unwrap();

        assert!(f.contains_stamp(&s1));
        assert!(f.contains_stamp(&s2));
        assert!(!f.contains_stamp(&s3));
        assert!(!f.contains_stamp(&s4));
    }

    #[test]
    fn merge_pairwise_maximum() {
        let mut f1 = Frontier::parse("nodeA:10, nodeB:5").unwrap();
        let f2 = Frontier::parse("nodeA:8, nodeB:12, nodeC:3").unwrap();

        f1.merge(&f2);
        assert_eq!(f1.get("nodeA"), 10);
        assert_eq!(f1.get("nodeB"), 12);
        assert_eq!(f1.get("nodeC"), 3);
    }
}
