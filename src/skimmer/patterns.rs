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
    if core.len() < 4 || core.len() > 10 {
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

fn validate_token_heuristic(token: &str) -> Option<String> {
    if looks_like_callsign(token) {
        Some(token.trim().to_ascii_uppercase())
    } else {
        None
    }
}

/// True when `token` is the canonical call or an SCP completion of it.
fn token_refers_to_call(token: &str, call: &str, scp: Option<&MasterScp>) -> bool {
    let call = call.trim().to_ascii_uppercase();
    let token = token.trim().to_ascii_uppercase();
    if token == call {
        return true;
    }
    if let Some(db) = scp {
        if db.resolve(&token).as_deref() == Some(call.as_str()) {
            return true;
        }
    }
    false
}

fn matching_token_count(tokens: &[&str], call: &str, scp: Option<&MasterScp>) -> usize {
    tokens
        .iter()
        .filter(|&&t| token_refers_to_call(t, call, scp))
        .count()
}

fn call_exact_occurrences(text: &str, call: &str) -> usize {
    let upper = text.to_ascii_uppercase();
    let call = call.to_ascii_uppercase();
    if call.is_empty() {
        return 0;
    }
    upper.match_indices(&call).count()
}

/// Require evidence beyond a single SCP hit — real exchanges repeat the call
/// or place it right after CQ/QRZ/DE.
fn callsign_confirmed(
    text: &str,
    tokens: &[&str],
    call: &str,
    scp: Option<&MasterScp>,
    calling_cq: bool,
) -> bool {
    let token_hits = matching_token_count(tokens, call, scp);
    let substring_hits = call_exact_occurrences(text, call);
    if token_hits.max(substring_hits) >= 2 {
        return true;
    }
    if calling_cq {
        if let Some(pos) = tokens.iter().position(|&t| t == "CQ" || t == "QRZ") {
            if tokens
                .iter()
                .skip(pos + 1)
                .any(|t| token_refers_to_call(t, call, scp))
            {
                return true;
            }
        }
    }
    tokens.iter().enumerate().any(|(i, &t)| {
        (t == "DE" || t == "CQ" || t == "QRZ")
            && tokens
                .get(i + 1)
                .is_some_and(|next| token_refers_to_call(next, call, scp))
    })
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
    if require_scp {
        return None;
    }
    validate_token_heuristic(token)
}

fn is_calling_cq(upper: &str, tokens: &[&str]) -> bool {
    if tokens.iter().any(|&t| t == "CQ" || t == "QRZ") {
        return true;
    }
    if upper.contains(" CQ ") || upper.starts_with("CQ ") {
        return true;
    }
    let compact: String = upper
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    compact.contains("CQCQ")
}

/// Scan decoded text for a CQ/run flag and the most likely callsign.
pub fn analyze(text: &str, scp: Option<&MasterScp>, require_scp: bool) -> Option<PatternMatch> {
    let upper = text.to_ascii_uppercase();
    let tokens: Vec<&str> = upper.split_whitespace().collect();
    if tokens.is_empty() {
        return None;
    }

    let calling_cq = is_calling_cq(&upper, &tokens);

    // SCP full-text scan first — catches callsigns glued to decoder garbage.
    let mut callsign = scp.and_then(|db| db.find_in_text(&upper, require_scp));

    if callsign.is_none() && calling_cq {
        if let Some(pos) = tokens.iter().position(|&t| t == "CQ" || t == "QRZ") {
            callsign = tokens
                .iter()
                .skip(pos + 1)
                .filter(|t| !STOPWORDS.contains(t))
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

    if let Some(ref call) = callsign {
        let confirmed = callsign_confirmed(&upper, &tokens, call, scp, calling_cq);
        let relaxed_cq = calling_cq
            && upper
                .chars()
                .filter(|c| c.is_ascii_alphanumeric())
                .collect::<String>()
                .contains("CQCQ")
            && call_exact_occurrences(&upper, call) >= 1;
        let cq_single = calling_cq && call_exact_occurrences(&upper, call) >= 1;
        if require_scp && !confirmed && !relaxed_cq && !cq_single {
            callsign = None;
        }
    }

    if callsign.is_none() && !calling_cq {
        return None;
    }
    Some(PatternMatch { callsign, kind })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skimmer::scp::MasterScp;

    const SAMPLE_SCP: &str = "VER20260202\nW1AW\nOH2BH\nK3LR\nSM5DAJ\n";

    #[test]
    fn detects_garbled_cqcq() {
        let m = analyze("?CQCQDES?D? SD5DE", None, false).expect("cq");
        assert_eq!(m.kind, SpotKind::CallingCq);
    }

    #[test]
    fn validates_callsigns_heuristic() {
        assert!(looks_like_callsign("W1AW"));
        assert!(!looks_like_callsign("ES5"));
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
        assert!(analyze("ES5 DE EE5", Some(&scp), true).is_none());
    }

    #[test]
    fn scp_finds_embedded_call() {
        let scp = MasterScp::from_text(SAMPLE_SCP);
        let m = analyze("DE SM5DAJ SM5DAJ AR", Some(&scp), true).expect("match");
        assert_eq!(m.callsign.as_deref(), Some("SM5DAJ"));
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
    fn detects_cq_de_call_once() {
        let scp = MasterScp::from_text(SAMPLE_SCP);
        let m = analyze("CQ DE W1AW", Some(&scp), true).expect("match");
        assert_eq!(m.kind, SpotKind::CallingCq);
        assert_eq!(m.callsign.as_deref(), Some("W1AW"));
    }

    #[test]
    fn garbage_returns_none_without_scp() {
        assert!(analyze("ETIA NN", None, false).is_none());
    }

    #[test]
    fn scp_rejects_unconfirmed_single_hit() {
        let scp = MasterScp::from_text("VER20260202\nSE5S\nEE5E\n");
        assert!(analyze("SE5S", Some(&scp), true).is_none());
        assert!(analyze("EE5E", Some(&scp), true).is_none());
        assert!(analyze("DE SE5S", Some(&scp), true).is_some());
        assert!(analyze("SE5S SE5S", Some(&scp), true).is_some());
    }
}
