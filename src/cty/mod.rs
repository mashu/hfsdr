//! Callsign → DXCC entity → continent resolution (build-order item 5).
//!
//! The contest-correct path is to load AD1C's `cty.dat` and apply its full
//! exception logic (portable `/` calls, explicit exceptions, zone overrides).
//! That parser is not implemented yet; [`ContinentResolver`] ships with a small
//! built-in prefix fallback so continent filters are usable immediately, and a
//! [`ContinentResolver::load_cty_dat`] seam for the real database.

/// CQ/ITU continents.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Continent {
    NorthAmerica,
    SouthAmerica,
    Europe,
    Africa,
    Asia,
    Oceania,
    Antarctica,
}

impl Continent {
    pub fn code(self) -> &'static str {
        match self {
            Continent::NorthAmerica => "NA",
            Continent::SouthAmerica => "SA",
            Continent::Europe => "EU",
            Continent::Africa => "AF",
            Continent::Asia => "AS",
            Continent::Oceania => "OC",
            Continent::Antarctica => "AN",
        }
    }

    pub const ALL: [Continent; 7] = [
        Continent::NorthAmerica,
        Continent::SouthAmerica,
        Continent::Europe,
        Continent::Africa,
        Continent::Asia,
        Continent::Oceania,
        Continent::Antarctica,
    ];
}

/// Resolves a callsign to a continent.
#[derive(Debug, Default)]
pub struct ContinentResolver {
    cty_loaded: bool,
}

impl ContinentResolver {
    pub fn new() -> Self {
        Self { cty_loaded: false }
    }

    /// Load AD1C `cty.dat` contents. Not yet implemented — returns whether the
    /// full database is active (currently always `false`, falling back to the
    /// built-in prefix table).
    pub fn load_cty_dat(&mut self, _contents: &str) -> bool {
        // TODO: parse cty.dat records + exception/override lines.
        self.cty_loaded = false;
        self.cty_loaded
    }

    pub fn uses_full_database(&self) -> bool {
        self.cty_loaded
    }

    /// Best-effort continent from the leading prefix characters.
    ///
    /// This is the documented fallback, NOT the full exception logic. It is
    /// good enough to drive continent filters but will misclassify edge cases.
    pub fn continent_of(&self, callsign: &str) -> Option<Continent> {
        let call = callsign.trim().to_ascii_uppercase();
        let base = call.split('/').max_by_key(|s| s.len())?;
        prefix_continent(base)
    }
}

fn prefix_continent(call: &str) -> Option<Continent> {
    let c0 = call.chars().next()?;
    let two: String = call.chars().take(2).collect();

    // A coarse first-letter / common-prefix map. Replace with cty.dat.
    use Continent::*;
    let by_two = match two.as_str() {
        "EA" | "EB" | "EC" | "EI" | "EU" | "ER" | "ES" | "EV" | "EW" => Some(Europe),
        "VE" | "VA" | "VO" | "VY" => Some(NorthAmerica),
        "VK" => Some(Oceania),
        "VU" => Some(Asia),
        "ZL" | "ZM" => Some(Oceania),
        "ZS" => Some(Africa),
        "PY" | "PP" | "PT" | "PU" | "PV" | "PW" => Some(SouthAmerica),
        "LU" => Some(SouthAmerica),
        "JA" | "JE" | "JF" | "JG" | "JH" | "JI" | "JJ" | "JK" | "JR" => Some(Asia),
        "UA" | "UB" | "RA" | "RU" | "RW" | "RV" | "RK" => Some(Europe),
        _ => None,
    };
    if by_two.is_some() {
        return by_two;
    }

    match c0 {
        'K' | 'N' | 'W' => Some(NorthAmerica),
        'A' => Some(NorthAmerica),
        'G' | 'M' | 'F' | 'I' | 'D' | 'O' | 'S' | 'H' | 'L' | 'P' | 'T' | 'Y' | 'Z' => {
            Some(Europe)
        }
        'J' | 'B' | 'U' | 'R' => Some(Asia),
        'V' => Some(Oceania),
        'C' => Some(NorthAmerica),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn continent_codes_are_two_letters() {
        assert_eq!(Continent::NorthAmerica.code(), "NA");
        assert_eq!(Continent::SouthAmerica.code(), "SA");
        assert_eq!(Continent::Europe.code(), "EU");
        assert_eq!(Continent::Africa.code(), "AF");
        assert_eq!(Continent::Asia.code(), "AS");
        assert_eq!(Continent::Oceania.code(), "OC");
        assert_eq!(Continent::Antarctica.code(), "AN");
        assert_eq!(Continent::ALL.len(), 7);
        assert!(Continent::ALL.contains(&Continent::Antarctica));
    }

    #[test]
    fn resolves_common_prefixes() {
        let r = ContinentResolver::new();
        assert_eq!(r.continent_of("W1AW"), Some(Continent::NorthAmerica));
        assert_eq!(r.continent_of("EA3ABC"), Some(Continent::Europe));
        assert_eq!(r.continent_of("VK3XYZ"), Some(Continent::Oceania));
        assert_eq!(r.continent_of("PY2ABC"), Some(Continent::SouthAmerica));
    }

    #[test]
    fn resolves_asia_africa_and_europe_two_letter() {
        let r = ContinentResolver::new();
        assert_eq!(r.continent_of("JA1ABC"), Some(Continent::Asia));
        assert_eq!(r.continent_of("VU2XYZ"), Some(Continent::Asia));
        assert_eq!(r.continent_of("ZS6ABC"), Some(Continent::Africa));
        assert_eq!(r.continent_of("UA3ABC"), Some(Continent::Europe));
        assert_eq!(r.continent_of("LU7ABC"), Some(Continent::SouthAmerica));
    }

    #[test]
    fn resolves_single_letter_prefix_fallback() {
        let r = ContinentResolver::new();
        assert_eq!(r.continent_of("K0ABC"), Some(Continent::NorthAmerica));
        assert_eq!(r.continent_of("G4ABC"), Some(Continent::Europe));
        assert_eq!(r.continent_of("JH1ABC"), Some(Continent::Asia));
        assert_eq!(r.continent_of("VE3ABC"), Some(Continent::NorthAmerica));
    }

    #[test]
    fn handles_portable_calls() {
        let r = ContinentResolver::new();
        assert_eq!(r.continent_of("W1AW/3"), Some(Continent::NorthAmerica));
        assert_eq!(r.continent_of("g0abc/m"), Some(Continent::Europe));
        assert_eq!(r.continent_of("  w1aw  "), Some(Continent::NorthAmerica));
    }

    #[test]
    fn unknown_or_empty_callsign() {
        let r = ContinentResolver::new();
        assert_eq!(r.continent_of(""), None);
        assert_eq!(r.continent_of("   "), None);
        assert_eq!(r.continent_of("Q9999"), None);
    }

    #[test]
    fn load_cty_dat_falls_back_to_prefix_table() {
        let mut r = ContinentResolver::new();
        assert!(!r.uses_full_database());
        assert!(!r.load_cty_dat("Country file contents"));
        assert!(!r.uses_full_database());
        assert_eq!(r.continent_of("W1AW"), Some(Continent::NorthAmerica));
    }
}
