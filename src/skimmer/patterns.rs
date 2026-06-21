//! CQ / callsign recognition over a decoded text stream.
//!
//! Cuts decode garbage by validating tokens against a callsign heuristic and
//! flagging CQ / contest exchanges. A future supercheck-partial (MASTER.SCP)
//! dictionary lookup can replace [`looks_like_callsign`] without changing the
//! interface.

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

/// Heuristic callsign validator (no MASTER.SCP yet).
///
/// Accepts uppercase tokens of 3..=10 chars made of letters/digits/`/` that
/// contain at least one letter and one digit and are not exchange stopwords.
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

/// Scan decoded text for a CQ/run flag and the most likely callsign.
pub fn analyze(text: &str) -> Option<PatternMatch> {
    let upper = text.to_ascii_uppercase();
    let tokens: Vec<&str> = upper.split_whitespace().collect();
    if tokens.is_empty() {
        return None;
    }

    let calling_cq = tokens.iter().any(|&t| t == "CQ" || t == "QRZ");

    // Prefer the callsign right after a CQ marker, else the first valid call.
    let mut callsign = None;
    if calling_cq {
        if let Some(pos) = tokens.iter().position(|&t| t == "CQ" || t == "QRZ") {
            callsign = tokens
                .iter()
                .skip(pos + 1)
                .find(|t| looks_like_callsign(t))
                .map(|s| s.to_string());
        }
    }
    if callsign.is_none() {
        callsign = tokens
            .iter()
            .find(|t| looks_like_callsign(t))
            .map(|s| s.to_string());
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

    #[test]
    fn validates_callsigns() {
        assert!(looks_like_callsign("W1AW"));
        assert!(looks_like_callsign("OH2BH"));
        assert!(looks_like_callsign("W1AW/4"));
        assert!(!looks_like_callsign("CQ"));
        assert!(!looks_like_callsign("TEST"));
        assert!(!looks_like_callsign("HELLO"));
        assert!(!looks_like_callsign("73"));
    }

    #[test]
    fn detects_cq_with_call() {
        let m = analyze("CQ TEST OH2BH OH2BH").expect("match");
        assert_eq!(m.kind, SpotKind::CallingCq);
        assert_eq!(m.callsign.as_deref(), Some("OH2BH"));
    }

    #[test]
    fn detects_answering_call() {
        let m = analyze("W1AW DE K3LR").expect("match");
        assert_eq!(m.kind, SpotKind::Answering);
        assert!(m.callsign.is_some());
    }

    #[test]
    fn garbage_returns_none() {
        assert!(analyze("ETIA NN").is_none());
    }
}
