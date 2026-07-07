//! Heuristics for judging whether skimmer decode text is real CW copy.

/// True when decoded text is mostly uncertainty / noise, not real copy.
pub fn decode_is_garbage(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return true;
    }
    let chars: Vec<char> = trimmed.chars().collect();
    let q = chars.iter().filter(|&&c| c == '?').count();
    if chars.len() >= 5 && (q as f32) / (chars.len() as f32) > 0.30 {
        return true;
    }
    let alnum = chars
        .iter()
        .filter(|c| c.is_ascii_alphanumeric())
        .count();
    alnum < 3
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn garbage_is_mostly_question_marks() {
        assert!(decode_is_garbage("?E?I?GV?EINSFV?"));
        assert!(!decode_is_garbage("CQ DE F6FJK"));
    }

    #[test]
    fn empty_is_garbage() {
        assert!(decode_is_garbage(""));
        assert!(decode_is_garbage("   "));
    }
}
