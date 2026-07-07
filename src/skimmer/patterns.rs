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
    if calling_cq && scp_exact_call_in_text(call, text, scp) {
        return true;
    }
    if calling_cq {
        for (i, &t) in tokens.iter().enumerate() {
            if t == "CQ" || t == "QRZ" {
                if tokens
                    .iter()
                    .skip(i + 1)
                    .any(|t| token_refers_to_call(t, call, scp))
                {
                    return true;
                }
            }
        }
    }
    tokens.iter().enumerate().any(|(i, &t)| {
        (t == "DE" || t == "CQ" || t == "QRZ")
            && tokens
                .get(i + 1)
                .is_some_and(|next| token_refers_to_call(next, call, scp))
    }) || de_glued_refers_to_call(tokens, call, scp)
}

fn de_glued_refers_to_call(tokens: &[&str], call: &str, scp: Option<&MasterScp>) -> bool {
    tokens.iter().enumerate().any(|(i, &t)| {
        if t != "DE" {
            return false;
        }
        glued_callsign_tokens(tokens, i + 1)
            .iter()
            .any(|glued| token_refers_to_call(glued, call, scp))
    })
}

fn scp_exact_call_in_text(call: &str, text: &str, scp: Option<&MasterScp>) -> bool {
    let Some(db) = scp else {
        return false;
    };
    if !db.is_loaded() {
        return false;
    }
    let call = call.trim().to_ascii_uppercase();
    if db.resolve(&call).as_deref() != Some(call.as_str()) {
        return false;
    }
    db.find_in_text(text, false)
        .is_some_and(|found| found == call)
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

/// Like [`validate_token`], but allows heuristic shape checks after CQ/DE even when
/// strict SCP mode is on — many contest calls are not in MASTER.SCP yet.
fn validate_token_in_exchange(token: &str, scp: Option<&MasterScp>, require_scp: bool) -> Option<String> {
    validate_token(token, scp, require_scp).or_else(|| {
        if require_scp {
            validate_token_heuristic(token)
        } else {
            None
        }
    })
}

/// Merge single-character decode fragments (`F 6 F J K`) into callsign-shaped strings.
fn glued_callsign_tokens(tokens: &[&str], start: usize) -> Vec<String> {
    const MAX_GLUE: usize = 12;
    let mut longest = String::new();
    let mut buf = String::new();
    for &t in tokens.iter().skip(start).take(MAX_GLUE) {
        if STOPWORDS.contains(&t) {
            if buf.len() > longest.len() {
                longest.clone_from(&buf);
            }
            buf.clear();
            continue;
        }
        let alnum: String = t.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
        if alnum.is_empty() {
            continue;
        }
        if !buf.is_empty() && buf.len() + alnum.len() > 10 {
            if buf.len() > longest.len() {
                longest.clone_from(&buf);
            }
            buf.clear();
        }
        buf.push_str(&alnum);
    }
    if buf.len() > longest.len() {
        longest = buf;
    }
    if longest.len() >= 4 {
        vec![longest]
    } else {
        Vec::new()
    }
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

    // Candidates in priority order: SCP full-text scan (catches callsigns
    // glued to decoder garbage), tokens right after CQ/QRZ, then any token.
    let mut candidates: Vec<String> = Vec::new();
    let push_candidate = |c: Option<String>, candidates: &mut Vec<String>| {
        if let Some(c) = c {
            if !candidates.contains(&c) {
                candidates.push(c);
            }
        }
    };
    push_candidate(
        scp.and_then(|db| db.find_in_text(&upper, require_scp)),
        &mut candidates,
    );
    if calling_cq {
        for (i, &t) in tokens.iter().enumerate() {
            if t == "CQ" || t == "QRZ" {
                for token in tokens.iter().skip(i + 1).filter(|t| !STOPWORDS.contains(t)) {
                    push_candidate(
                        validate_token_in_exchange(token, scp, require_scp),
                        &mut candidates,
                    );
                }
                for glued in glued_callsign_tokens(&tokens, i + 1) {
                    push_candidate(
                        validate_token_in_exchange(&glued, scp, require_scp),
                        &mut candidates,
                    );
                }
            }
            if t == "DE" {
                if let Some(next) = tokens.get(i + 1) {
                    push_candidate(
                        validate_token_in_exchange(next, scp, require_scp),
                        &mut candidates,
                    );
                }
                for glued in glued_callsign_tokens(&tokens, i + 1) {
                    push_candidate(
                        validate_token_in_exchange(&glued, scp, require_scp),
                        &mut candidates,
                    );
                }
            }
        }
    }
    for (i, &t) in tokens.iter().enumerate() {
        if t == "DE" {
            for glued in glued_callsign_tokens(&tokens, i + 1) {
                push_candidate(
                    validate_token_in_exchange(&glued, scp, require_scp),
                    &mut candidates,
                );
            }
        }
    }
    for t in &tokens {
        push_candidate(validate_token(t, scp, require_scp), &mut candidates);
    }

    // Whatever the validation source, demand corroboration: a repeat of the
    // call, or CQ/QRZ/DE context. A lone callsign-shaped token in garbled
    // text is the main source of false spots.
    // Prefer longer glued calls over shorter prefixes (F6FJK over F6FJ).
    candidates.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
    let callsign = candidates.into_iter().find(|call| {
        callsign_confirmed(&upper, &tokens, call, scp, calling_cq)
            || (calling_cq && call_exact_occurrences(&upper, call) >= 1)
    });

    let kind = if calling_cq {
        SpotKind::CallingCq
    } else if callsign.is_some() {
        SpotKind::Answering
    } else {
        SpotKind::Heard
    };

    if callsign.is_none() {
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
    fn detects_garbled_cqcq_without_call() {
        assert!(analyze("?CQCQDES?D?", None, false).is_none());
    }

    #[test]
    fn cq_only_returns_none() {
        assert!(analyze("CQ CQ", None, false).is_none());
        assert!(analyze("CQ", None, false).is_none());
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
    fn cq_cq_then_call_confirms_after_second_cq() {
        let scp = MasterScp::from_text(SAMPLE_SCP);
        let m = analyze("CQ CQ W1AW", Some(&scp), true).expect("match");
        assert_eq!(m.callsign.as_deref(), Some("W1AW"));
        assert_eq!(m.kind, SpotKind::CallingCq);
    }

    #[test]
    fn scp_embedded_in_cq_garble_counts_once() {
        let scp = MasterScp::from_text(SAMPLE_SCP);
        let m = analyze("CQCQDEW1AW", Some(&scp), true).expect("match");
        assert_eq!(m.callsign.as_deref(), Some("W1AW"));
    }
    #[test]
    fn glued_fragments_after_de() {
        let m = analyze("XX DE F 6 F J K TEST", None, false).expect("match");
        assert!(
            m.callsign
                .as_deref()
                .is_some_and(|c| c == "F6FJK" || c == "F6FJ"),
            "got {:?}",
            m.callsign
        );
    }

    #[test]
    fn strict_scp_allows_heuristic_after_de() {
        let scp = MasterScp::from_text(SAMPLE_SCP);
        let m = analyze("CQ DE ZZ9ZZZ", Some(&scp), true).expect("match");
        assert_eq!(m.callsign.as_deref(), Some("ZZ9ZZZ"));
        assert_eq!(m.kind, SpotKind::CallingCq);
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
