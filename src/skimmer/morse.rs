//! Morse code element table — pure data, no state.
//!
//! Elements use `.` for a dit and `-` for a dah. Decoding maps an element string
//! to a character; unknown patterns return `None` so the decoder can emit a
//! placeholder rather than guess.

/// (character, element pattern) for letters, digits and common punctuation/prosigns.
pub const MORSE_TABLE: &[(char, &str)] = &[
    ('A', ".-"),
    ('B', "-..."),
    ('C', "-.-."),
    ('D', "-.."),
    ('E', "."),
    ('F', "..-."),
    ('G', "--."),
    ('H', "...."),
    ('I', ".."),
    ('J', ".---"),
    ('K', "-.-"),
    ('L', ".-.."),
    ('M', "--"),
    ('N', "-."),
    ('O', "---"),
    ('P', ".--."),
    ('Q', "--.-"),
    ('R', ".-."),
    ('S', "..."),
    ('T', "-"),
    ('U', "..-"),
    ('V', "...-"),
    ('W', ".--"),
    ('X', "-..-"),
    ('Y', "-.--"),
    ('Z', "--.."),
    ('0', "-----"),
    ('1', ".----"),
    ('2', "..---"),
    ('3', "...--"),
    ('4', "....-"),
    ('5', "....."),
    ('6', "-...."),
    ('7', "--..."),
    ('8', "---.."),
    ('9', "----."),
    ('/', "-..-."),
    ('?', "..--.."),
    ('.', ".-.-.-"),
    (',', "--..--"),
    ('=', "-...-"),
    ('+', ".-.-."),
];

/// Decode an element string (e.g. `"-.-."`) into a character.
pub fn decode_elements(elements: &str) -> Option<char> {
    MORSE_TABLE
        .iter()
        .find(|(_, code)| *code == elements)
        .map(|(ch, _)| *ch)
}

/// Encode a character into its element string (used by tests / future keyer).
pub fn encode_char(ch: char) -> Option<&'static str> {
    let up = ch.to_ascii_uppercase();
    MORSE_TABLE
        .iter()
        .find(|(c, _)| *c == up)
        .map(|(_, code)| *code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_letters() {
        assert_eq!(decode_elements("-.-."), Some('C'));
        assert_eq!(decode_elements("--.-"), Some('Q'));
        assert_eq!(decode_elements("."), Some('E'));
    }

    #[test]
    fn unknown_pattern_is_none() {
        assert_eq!(decode_elements("......."), None);
    }

    #[test]
    fn encode_roundtrip() {
        for &(ch, code) in MORSE_TABLE {
            assert_eq!(encode_char(ch), Some(code));
            assert_eq!(decode_elements(code), Some(ch));
        }
    }
}
