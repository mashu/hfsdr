//! CQ / callsign recognition over a decoded text stream.
//!
//! Validates tokens against MASTER.SCP when loaded, with a heuristic fallback.

use super::scp::MasterScp;
use super::spots::SpotKind;

/// Common CW abbreviations / exchange tokens that are never callsigns.
const STOPWORDS: &[&str] = &[
    "CQ", "TEST", "DE", "TU", "GL", "GM", "GA", "GE", "73", "599", "5NN", "K", "KN", "R", "AR",
    "BK", "QRZ", "UP", "DN", "AGN", "PSE", "NW", "ES", "OM",
];

/// A recognised pattern in the decoded text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatternMatch {
    pub callsign: Option<String>,
    pub kind: SpotKind,
}

/// Heuristic callsign validator when SCP is unavailable.
pub fn looks_like_callsign(token: &str) -> bool {
    let t = token.trim();
    let core = t.split('/').max_by_key(|s| s.len()).unwrap_or(t);
    if core.len() < 3 || core.len() > 10 {
        return false;
    }
    if STOPWORDS.contains(&t) {
        return false;
    }
    let mut has_alpha = false;
    let mut has_digit = false;
    for c in core.chars() {
        match c {
            'A'..='Z' => has_alpha = true,
            '0'..='9' => has_digit = true,
            _ => return false,
        }
    }
    has_alpha && has_digit
}

fn validate_token(token: &str, scp: Option<&MasterScp>, require_scp: bool) -> Option<String> {
    if let Some(db) = scp {
        if db.is_loaded() {
            if let Some(call) = db.resolve(token) {
                return Some(call);
            }
            if require_scp {
                return None;
            }
        }
    }
    if looks_like_callsign(token) {
        return Some(token.trim().to_ascii_uppercase());
    }
    None
}

/// Scan decoded text for a CQ/run flag and the most likely callsign.
pub fn analyze(text: &str, scp: Option<&MasterScp>, require_scp: bool) -> Option<PatternMatch> {
    let upper = text.to_ascii_uppercase();
    let tokens: Vec<&str> = upper.split_whitespace().collect();
    if tokens.is_empty() {
        return None;
    }

    let calling_cq = tokens.iter().any(|&t| t == "CQ" || t == "QRZ");

    let mut callsign = None;
    if calling_cq {
        if let Some(pos) = tokens.iter().position(|&t| t == "CQ" || t == "QRZ") {
            callsign = tokens
                .iter()
                .skip(pos + 1)
                .find_map(|t| validate_token(t, scp, require_scp));
        }
    }
    if callsign.is_none() {
        callsign = tokens
            .iter()
            .find_map(|t| validate_token(t, scp, require_scp));
    }

    let kind = if calling_cq {
        SpotKind::CallingCq
    } else if callsign.is_some() {
        SpotKind::Answering
    } else {
        SpotKind::Heard
    };

    if callsign.is_none() && !calling_cq {
        return None;
    }
    Some(PatternMatch { callsign, kind })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skimmer::scp::MasterScp;

    const SAMPLE_SCP: &str = "VER20260202\nW1AW\nOH2BH\nK3LR\n";

    #[test]
    fn validates_callsigns_heuristic() {
        assert!(looks_like_callsign("W1AW"));
        assert!(!looks_like_callsign("CQ"));
    }

    #[test]
    fn detects_cq_with_call() {
        let scp = MasterScp::from_text(SAMPLE_SCP);
        let m = analyze("CQ TEST OH2BH OH2BH", Some(&scp), true).expect("match");
        assert_eq!(m.kind, SpotKind::CallingCq);
        assert_eq!(m.callsign.as_deref(), Some("OH2BH"));
    }

    #[test]
    fn scp_rejects_garbage() {
        let scp = MasterScp::from_text(SAMPLE_SCP);
        assert!(analyze("5B63BO DE TEST", Some(&scp), true).is_none());
    }

    #[test]
    fn scp_completes_unique_partial() {
        let scp = MasterScp::from_text(SAMPLE_SCP);
        let m = analyze("DE OH2B", Some(&scp), true).expect("match");
        assert_eq!(m.callsign.as_deref(), Some("OH2BH"));
    }

    #[test]
    fn no_scp_falls_back_to_heuristic() {
        let m = analyze("W1AW DE K3LR", None, false).expect("match");
        assert_eq!(m.kind, SpotKind::Answering);
    }

    #[test]
    fn garbage_returns_none_without_scp() {
        assert!(analyze("ETIA NN", None, false).is_none());
    }
}
