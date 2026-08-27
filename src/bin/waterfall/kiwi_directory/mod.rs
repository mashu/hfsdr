//! Public KiwiSDR directory: geo lookup + receiver list sorted by proximity.
//!
//! The official list at kiwisdr.com/public is captcha-protected; we use the
//! community mirror at rx.linkfanel.net (Dyatlov map data) plus ip-api.com for
//! coarse geolocation. Results are cached under the app config directory.

use std::io::Read;
use std::path::PathBuf;
use hfsdr::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Community mirror of the KiwiSDR directory.
///
/// Plain http, and it has to be: the host answers on port 80 only. A build
/// fetching it over TLS gets `Failed to connect to rx.linkfanel.net port 443`,
/// which is how this was established — an earlier change to https on
/// mixed-content reasoning broke the desktop directory, which had been working,
/// and could never have fixed the browser one.
///
/// A browser on an https page therefore cannot reach this at all: http is
/// blocked as mixed content and https does not exist. That is not a gap to work
/// around at runtime, it is why the list is fetched at build time by CI (which
/// has no such restriction) and served from our own origin. See
/// `scripts/build-web.sh` and [`web::BUNDLED_LIST`].
const LIST_URL: &str = "http://rx.linkfanel.net/kiwisdr_com.js";
/// Coarse geolocation, used only to sort receivers by distance.
///
/// Best-effort: when this fails the list is still shown, just unsorted, so it
/// must never be the reason the directory appears empty.
const GEO_URL: &str = "https://ipapi.co/json/";
const CACHE_FILE: &str = "kiwi_directory_v2.json";
const CACHE_MAX_AGE: Duration = Duration::from_secs(30 * 60);
const NEARBY_LIMIT: usize = 12;
/// Kiwi's own default, used when a plain-http URL omits the port.
const KIWI_DEFAULT_PORT: u16 = 8073;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GeoLocation {
    pub country: String,
    pub country_code: String,
    pub lat: f64,
    pub lon: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KiwiReceiver {
    pub host: String,
    pub port: u16,
    pub name: String,
    pub location: String,
    pub lat: f64,
    pub lon: f64,
    pub users: u8,
    pub users_max: u8,
    pub snr: u8,
    pub distance_km: f64,
    /// The directory listed this receiver with an `https://` URL, so it accepts
    /// TLS. Only these are reachable from a page served over https — see
    /// [`reachable_from_page`].
    ///
    /// Defaulted so a cache written before this field existed still loads;
    /// those entries are treated as plain http, which is the safe assumption.
    #[serde(default)]
    pub tls: bool,
}

/// Whether a browser on this page can open a socket to `receiver`.
///
/// A page served over https may not open a `ws://` connection: browsers class
/// it as mixed content and block it outright, with no user override. Most
/// KiwiSDRs serve plain http, so from an https deployment they are simply
/// unreachable — not slow, not refusing, unreachable. Saying so beforehand is
/// the only useful thing the app can do about it.
///
/// `page_is_https` is false for the desktop build and for a page served over
/// http, where every receiver is reachable.
pub fn reachable_from_page(page_is_https: bool, receiver_is_tls: bool) -> bool {
    !page_is_https || receiver_is_tls
}

/// Order the browser's receiver list: reachable first, then not full, then
/// nearest.
///
/// A receiver this page cannot reach is worse than one that is merely full —
/// the full one will free up, the unreachable one never will — so
/// reachability outranks occupancy.
pub fn sort_for_display(list: &mut [KiwiReceiver], page_is_https: bool) {
    list.sort_by(|a, b| {
        let unreachable = |rx: &KiwiReceiver| !reachable_from_page(page_is_https, rx.tls);
        let full = |rx: &KiwiReceiver| rx.users >= rx.users_max;
        unreachable(a)
            .cmp(&unreachable(b))
            .then_with(|| full(a).cmp(&full(b)))
            .then_with(|| {
                a.distance_km
                    .partial_cmp(&b.distance_km)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });
}

/// The single line shown for a receiver in the browser list.
///
/// An unreachable receiver drops the distance and occupancy: neither tells the
/// user anything they can act on, and the reason it cannot be clicked is the
/// only thing worth the space.
pub fn receiver_line(rx: &KiwiReceiver, page_is_https: bool) -> String {
    if !reachable_from_page(page_is_https, rx.tls) {
        return format!(
            "{}:{} · no TLS — unreachable from https · {}",
            rx.host, rx.port, rx.location
        );
    }
    let distance = if rx.distance_km > 0.0 {
        format!("{:.0}km ", rx.distance_km)
    } else {
        String::new()
    };
    let users = if rx.users >= rx.users_max {
        format!("FULL {}/{}", rx.users, rx.users_max)
    } else {
        format!("{}/{}", rx.users, rx.users_max)
    };
    format!(
        "{}:{} · {}{} · {}",
        rx.host, rx.port, distance, users, rx.location
    )
}

/// Whether this page can reach any of these receivers at all.
///
/// False means the list is there but every entry is a dead end, which needs
/// saying once at the top rather than repeating on every row.
pub fn any_reachable(list: &[KiwiReceiver], page_is_https: bool) -> bool {
    list.iter()
        .any(|rx| reachable_from_page(page_is_https, rx.tls))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct CachedDirectory {
    fetched_at_secs: u64,
    geo: Option<GeoLocation>,
    receivers: Vec<KiwiReceiver>,
}

#[derive(Deserialize)]
struct GeoResponse {
    /// ip-api reports "success"/"fail" here; other providers omit it entirely,
    /// so absence means "no reason to think it failed".
    status: Option<String>,
    #[serde(alias = "country_name")]
    country: Option<String>,
    #[serde(rename = "countryCode", alias = "country_code")]
    country_code: Option<String>,
    #[serde(alias = "latitude")]
    lat: Option<f64>,
    #[serde(alias = "longitude")]
    lon: Option<f64>,
}

impl GeoResponse {
    fn into_location(self) -> Result<GeoLocation, String> {
        if self.status.as_deref().is_some_and(|s| s != "success") {
            return Err("geo lookup unsuccessful".into());
        }
        Ok(GeoLocation {
            country: self.country.unwrap_or_else(|| "Unknown".into()),
            country_code: self.country_code.unwrap_or_else(|| "??".into()),
            lat: self.lat.ok_or("geo missing lat")?,
            lon: self.lon.ok_or("geo missing lon")?,
        })
    }
}

#[derive(Deserialize)]
struct RawKiwi {
    status: String,
    offline: String,
    name: String,
    loc: String,
    gps: String,
    users: String,
    users_max: String,
    snr: String,
    url: String,
}

pub fn load_nearby_receivers() -> Result<(Option<GeoLocation>, Vec<KiwiReceiver>), String> {
    if let Some(cached) = read_cache() {
        return Ok((cached.geo, cached.receivers));
    }
    refresh_nearby_receivers()
}

/// Instant list from on-disk cache (no network). Used to populate the UI before refresh.
pub fn load_cached_receivers() -> Option<(Option<GeoLocation>, Vec<KiwiReceiver>)> {
    read_cache().map(|c| (c.geo, c.receivers))
}

pub fn refresh_nearby_receivers() -> Result<(Option<GeoLocation>, Vec<KiwiReceiver>), String> {
    let geo = fetch_geo().ok();
    let mut receivers = parse_receiver_list(&fetch_list_body()?)?;
    if let Some(ref g) = geo {
        rank_by_proximity(&mut receivers, g);
    }
    receivers.truncate(NEARBY_LIMIT);
    write_cache(&geo, &receivers)?;
    Ok((geo, receivers))
}

// The browser fetches through `fetch()`, which is async and cannot be wrapped
// in this blocking signature — see `web` below, which drives the same parsing
// from a future. These two exist only so the shared entry points still compile.
#[cfg(not(feature = "gui-core"))]
fn fetch_geo() -> Result<GeoLocation, String> {
    Err("geo lookup is asynchronous in the browser".into())
}

#[cfg(not(feature = "gui-core"))]
fn fetch_list_body() -> Result<String, String> {
    Err("the receiver list is fetched asynchronously in the browser".into())
}

#[cfg(all(target_arch = "wasm32", not(feature = "gui-core")))]
pub mod web;



/// Whether the document was served over TLS.
#[cfg(all(target_arch = "wasm32", not(feature = "gui-core")))]
fn web_page_is_https() -> bool {
    web_sys::window()
        .and_then(|w| w.location().protocol().ok())
        .is_some_and(|p| p.eq_ignore_ascii_case("https:"))
}

#[cfg(feature = "gui-core")]
fn fetch_geo() -> Result<GeoLocation, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(8))
        .build();
    let resp = agent
        .get(GEO_URL)
        .call()
        .map_err(|e| format!("geo lookup failed: {e}"))?;
    if !(200..300).contains(&resp.status()) {
        return Err(format!("geo HTTP {}", resp.status()));
    }
    let mut body = String::new();
    resp.into_reader()
        .read_to_string(&mut body)
        .map_err(|e| e.to_string())?;
    let parsed: GeoResponse =
        serde_json::from_str(&body).map_err(|e| format!("geo JSON: {e}"))?;
    parsed.into_location()
}

#[cfg(feature = "gui-core")]
fn fetch_list_body() -> Result<String, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(45))
        .build();
    let resp = agent
        .get(LIST_URL)
        .call()
        .map_err(|e| format!("receiver list download failed: {e}"))?;
    if !(200..300).contains(&resp.status()) {
        return Err(format!("receiver list HTTP {}", resp.status()));
    }
    let mut bytes = Vec::new();
    resp.into_reader()
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    String::from_utf8(bytes).map_err(|e| e.to_string())
}

fn extract_json_array(body: &str) -> Result<String, String> {
    let start = body.find('[').ok_or("receiver list: no JSON array")?;
    let end = body.rfind(']').ok_or("receiver list: unterminated array")?;
    if end <= start {
        return Err("receiver list: empty array".into());
    }
    Ok(sanitize_json_array(&body[start..=end]))
}

/// Strip trailing commas that break strict JSON parsers (common in kiwisdr_com.js).
/// Copies string bytes verbatim so UTF-8 locations (e.g. `Kungsängen`) stay intact.
fn sanitize_json_array(json: &str) -> String {
    let bytes = json.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut in_string = false;
    let mut escape = false;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if in_string {
            out.push(b);
            if escape {
                escape = false;
            } else if b == b'\\' {
                escape = true;
            } else if b == b'"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        if b == b'"' {
            in_string = true;
            out.push(b);
            i += 1;
            continue;
        }
        if b == b',' {
            let mut j = i + 1;
            let mut trailing = false;
            while j < bytes.len() {
                let ch = bytes[j];
                if ch.is_ascii_whitespace() {
                    j += 1;
                    continue;
                }
                if ch == b']' || ch == b'}' {
                    trailing = true;
                }
                break;
            }
            if trailing {
                i += 1;
            } else {
                out.push(b);
                i += 1;
            }
            continue;
        }
        out.push(b);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| json.to_string())
}

fn parse_receiver_list(body: &str) -> Result<Vec<KiwiReceiver>, String> {
    let json = extract_json_array(body)?;
    let raw: Vec<RawKiwi> =
        serde_json::from_str(&json).map_err(|e| format!("receiver list JSON: {e}"))?;
    let mut out = Vec::new();
    for entry in raw {
        if entry.status != "active" || entry.offline != "no" {
            continue;
        }
        let users: u8 = entry.users.parse().unwrap_or(255);
        let users_max: u8 = entry.users_max.parse().unwrap_or(0);
        if users_max == 0 || users >= users_max {
            continue;
        }
        let Some((host, port, tls)) = parse_kiwi_url(&entry.url) else {
            continue;
        };
        let Some((lat, lon)) = parse_gps(&entry.gps) else {
            continue;
        };
        let snr = entry
            .snr
            .split(',')
            .next()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0);
        out.push(KiwiReceiver {
            host,
            port,
            name: trim_display_name(&entry.name),
            location: entry.loc,
            lat,
            lon,
            users,
            users_max,
            snr,
            tls,
            distance_km: 0.0,
        });
    }
    Ok(out)
}

fn trim_display_name(name: &str) -> String {
    let trimmed = name.trim();
    const MAX_CHARS: usize = 72;
    if trimmed.chars().count() <= MAX_CHARS {
        trimmed.to_string()
    } else {
        let short: String = trimmed.chars().take(69).collect();
        format!("{short}…")
    }
}

/// Host and port from a directory URL.
///
/// The default port follows the scheme, and getting that wrong matters more
/// than it looks. A receiver listed as `https://…` with no port is on 443 —
/// that is *why* it has no port — and those TLS receivers are the only ones an
/// https page can reach at all. Defaulting them to Kiwi's plain-http 8073 aimed
/// the browser build at a closed port on exactly the receivers that could have
/// worked.
fn parse_kiwi_url(url: &str) -> Option<(String, u16, bool)> {
    let (rest, default_port, tls) = url
        .strip_prefix("https://")
        .map(|r| (r, 443u16, true))
        .or_else(|| url.strip_prefix("http://").map(|r| (r, KIWI_DEFAULT_PORT, false)))?;
    let host_port = rest.split('/').next()?;
    if let Some((host, port_s)) = host_port.rsplit_once(':') {
        let port: u16 = port_s.parse().ok()?;
        if host.is_empty() {
            return None;
        }
        Some((host.to_string(), port, tls))
    } else if !host_port.is_empty() {
        Some((host_port.to_string(), default_port, tls))
    } else {
        None
    }
}

fn parse_gps(s: &str) -> Option<(f64, f64)> {
    let inner = s.trim().strip_prefix('(')?.strip_suffix(')')?;
    let (lat_s, lon_s) = inner.split_once(',')?;
    Some((lat_s.trim().parse().ok()?, lon_s.trim().parse().ok()?))
}

fn haversine_km(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    const R: f64 = 6371.0;
    let dlat = (lat2 - lat1).to_radians();
    let dlon = (lon2 - lon1).to_radians();
    let lat1 = lat1.to_radians();
    let lat2 = lat2.to_radians();
    let a = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());
    R * c
}

fn rank_by_proximity(receivers: &mut [KiwiReceiver], geo: &GeoLocation) {
    for rx in receivers.iter_mut() {
        rx.distance_km = haversine_km(geo.lat, geo.lon, rx.lat, rx.lon);
    }
    receivers.sort_by(|a, b| {
        let full_a = a.users >= a.users_max;
        let full_b = b.users >= b.users_max;
        let same_country_a = location_matches_country(&a.location, geo);
        let same_country_b = location_matches_country(&b.location, geo);
        full_a
            .cmp(&full_b)
            .then_with(|| same_country_b.cmp(&same_country_a))
            .then_with(|| a.distance_km.partial_cmp(&b.distance_km).unwrap_or(std::cmp::Ordering::Equal))
            .then_with(|| b.snr.cmp(&a.snr))
    });
}

fn location_matches_country(loc: &str, geo: &GeoLocation) -> bool {
    let loc_lower = loc.to_ascii_lowercase();
    loc_lower.contains(&geo.country.to_ascii_lowercase())
        || loc_lower.ends_with(&format!(", {}", geo.country_code.to_ascii_lowercase()))
}

fn cache_path() -> Option<PathBuf> {
    let mut dir = dirs::config_dir()?;
    dir.push("hfsdr");
    Some(dir.join(CACHE_FILE))
}

fn read_cache() -> Option<CachedDirectory> {
    let path = cache_path()?;
    let text = std::fs::read_to_string(&path).ok()?;
    let cached: CachedDirectory = serde_json::from_str(&text).ok()?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
    if now.saturating_sub(cached.fetched_at_secs) > CACHE_MAX_AGE.as_secs() {
        return None;
    }
    Some(cached)
}

fn write_cache(geo: &Option<GeoLocation>, receivers: &[KiwiReceiver]) -> Result<(), String> {
    let Some(path) = cache_path() else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let fetched_at_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs();
    let cached = CachedDirectory {
        fetched_at_secs,
        geo: geo.clone(),
        receivers: receivers.to_vec(),
    };
    let text = serde_json::to_string(&cached).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    /// The geo response is parsed from whichever provider answers, and the two
    /// in use spell every field differently. Getting this wrong does not fail
    /// loudly — it silently drops the sort order — so pin both shapes.
    #[test]
    fn geo_response_accepts_both_provider_shapes() {
        // ip-api.com: explicit status, camelCase country code.
        let ip_api: GeoResponse = serde_json::from_str(
            r#"{"status":"success","country":"Sweden","countryCode":"SE","lat":59.3,"lon":18.1}"#,
        )
        .expect("ip-api shape");
        let loc = ip_api.into_location().expect("location");
        assert_eq!(loc.country, "Sweden");
        assert_eq!(loc.country_code, "SE");
        assert!((loc.lat - 59.3).abs() < 1e-9);
        assert!((loc.lon - 18.1).abs() < 1e-9);

        // ipapi.co: no status field at all, snake_case names.
        let ipapi: GeoResponse = serde_json::from_str(
            r#"{"country_name":"Sweden","country_code":"SE","latitude":59.3,"longitude":18.1}"#,
        )
        .expect("ipapi shape");
        let loc = ipapi.into_location().expect("a missing status is not a failure");
        assert_eq!(loc.country_code, "SE");
        assert!((loc.lat - 59.3).abs() < 1e-9);
    }

    /// An explicit failure status must be honoured even when the response
    /// otherwise looks complete.
    ///
    /// The coordinates here are deliberate: without them the conversion fails
    /// on the missing latitude no matter what the status says, and the test
    /// would pass with the status check deleted.
    #[test]
    fn geo_response_reports_an_explicit_failure() {
        let failed: GeoResponse = serde_json::from_str(
            r#"{"status":"fail","country":"Sweden","countryCode":"SE","lat":59.3,"lon":18.1}"#,
        )
        .expect("fail shape");
        assert!(
            failed.into_location().is_err(),
            "a failed lookup was accepted because the rest of the fields parsed"
        );
    }

    /// Coordinates are the whole point; without them there is nothing to sort
    /// by, so a response missing them must not pass as a location.
    #[test]
    fn geo_response_without_coordinates_is_an_error() {
        let no_lat: GeoResponse =
            serde_json::from_str(r#"{"country_name":"Sweden","longitude":18.1}"#)
                .expect("partial shape");
        assert!(no_lat.into_location().is_err());

        let no_lon: GeoResponse =
            serde_json::from_str(r#"{"country_name":"Sweden","latitude":59.3}"#)
                .expect("partial shape");
        assert!(no_lon.into_location().is_err());
    }

    /// Names are optional and only ever displayed, so a response without them
    /// still locates the user.
    #[test]
    fn geo_response_without_names_still_locates() {
        let bare: GeoResponse =
            serde_json::from_str(r#"{"latitude":59.3,"longitude":18.1}"#).expect("bare shape");
        let loc = bare.into_location().expect("location");
        assert_eq!(loc.country, "Unknown");
        assert_eq!(loc.country_code, "??");
    }

    use super::*;

    const SAMPLE: &str = r#"var kiwisdr_com =
[
	{
		"status":"active",
		"offline":"no",
		"name":"G3SDR test",
		"loc":"Weston-super-Mare, United Kingdom",
		"gps":"(51.317266, -2.950479)",
		"users":"2",
		"users_max":"4",
		"snr":"43,41",
		"url":"http://g3sdr.com:8073"
	},
	{
		"status":"active",
		"offline":"yes",
		"name":"offline",
		"loc":"Nowhere",
		"gps":"(0, 0)",
		"users":"0",
		"users_max":"4",
		"snr":"10,10",
		"url":"http://offline.example:8073"
	}
];"#;

    #[test]
    fn parses_available_receivers() {
        let list = parse_receiver_list(SAMPLE).expect("parse");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].host, "g3sdr.com");
        assert_eq!(list[0].port, 8073);
    }

    /// The scheme has to survive the whole parse, not just `parse_kiwi_url`:
    /// the reachability check reads `KiwiReceiver::tls`, so a receiver that
    /// lost its scheme on the way into the struct would be shown as
    /// unreachable on every https page.
    #[test]
    fn parse_carries_the_scheme_onto_each_receiver() {
        let body = r#"[
            {"status":"active","offline":"no","name":"plain","loc":"a","gps":"(1,2)","users":"0","users_max":"4","snr":"10,10","url":"http://plain.example:8073"},
            {"status":"active","offline":"no","name":"secure","loc":"b","gps":"(3,4)","users":"0","users_max":"4","snr":"10,10","url":"https://secure.example/"}
        ]"#;
        let list = parse_receiver_list(body).expect("parse");
        assert_eq!(list.len(), 2);
        let plain = list.iter().find(|r| r.host == "plain.example").expect("plain");
        let secure = list.iter().find(|r| r.host == "secure.example").expect("secure");
        assert!(!plain.tls);
        assert!(secure.tls);
    }

    /// A cache written before `tls` existed must still load. It deserializes
    /// as plain http, so those receivers are hidden on an https page until the
    /// next refresh — wrong in the safe direction, unlike a parse error that
    /// would blank the list entirely.
    #[test]
    fn cache_without_the_tls_field_still_loads() {
        let old = r#"{"host":"rx.test","port":8073,"name":"n","location":"l","lat":1.0,"lon":2.0,"users":0,"users_max":4,"snr":10,"distance_km":0.0}"#;
        let rx: KiwiReceiver = serde_json::from_str(old).expect("old cache entry");
        assert!(!rx.tls);
    }

    #[test]
    fn strips_trailing_commas() {
        let broken = r#"[{"status":"active","offline":"no","name":"x","loc":"a","gps":"(1,2)","users":"1","users_max":"4","snr":"10,10","url":"http://h:8073"},]"#;
        let list = parse_receiver_list(broken).expect("parse trailing comma");
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn ranks_by_distance() {
        let mut list = parse_receiver_list(SAMPLE).expect("parse");
        let geo = GeoLocation {
            country: "United Kingdom".into(),
            country_code: "GB".into(),
            lat: 51.5,
            lon: -0.1,
        };
        rank_by_proximity(&mut list, &geo);
        assert!(list[0].distance_km < 500.0);
    }

    #[test]
    fn preserves_utf8_locations() {
        let body = r#"var kiwisdr_com =
[
	{
		"status":"active",
		"offline":"no",
		"name":"test",
		"loc":"Kungsängen, Sweden",
		"gps":"(59.5,17.7)",
		"users":"1",
		"users_max":"4",
		"snr":"10,10",
		"url":"http://example.com:8073"
	}
];"#;
        let list = parse_receiver_list(body).expect("parse");
        assert_eq!(list[0].location, "Kungsängen, Sweden");
    }

    #[test]
    fn trims_display_name_on_char_boundary() {
        let name = "0 - 30 MHz SDR | 🇮🇪 🇮🇪 🇮🇪 🇮🇪 URL: http://rx3.radio101.de 🇮🇪 🇮🇪 🇮🇪 🇮🇪 Glenbeigh, Kerry / Ireland";
        let out = trim_display_name(name);
        assert!(out.is_char_boundary(out.len()));
        assert!(out.chars().count() <= 73);
    }

    #[test]
    fn parse_rejects_non_array_payload() {
        assert!(parse_receiver_list("not json").is_err());
        assert!(parse_receiver_list("[]").unwrap().is_empty());
    }

    #[test]
    fn parse_kiwi_url_host_and_port() {
        assert_eq!(
            parse_kiwi_url("http://g3sdr.com:8073/"),
            Some(("g3sdr.com".into(), 8073, false))
        );
        // https with no port means 443, not Kiwi's plain-http default: these
        // are the TLS receivers, and the only ones an https page can reach.
        assert_eq!(
            parse_kiwi_url("https://rx.test"),
            Some(("rx.test".into(), 443, true))
        );
        assert_eq!(
            parse_kiwi_url("https://sk2hg.proxy.kiwisdr.com/"),
            Some(("sk2hg.proxy.kiwisdr.com".into(), 443, true))
        );
        // An explicit port always wins over the scheme default.
        assert_eq!(
            parse_kiwi_url("https://rx.test:8073"),
            Some(("rx.test".into(), 8073, true))
        );
        assert!(parse_kiwi_url("ftp://bad").is_none());
    }

    /// The whole point of tracking TLS: on an https page a plain-http receiver
    /// cannot be reached at all, and the app should say so rather than offer a
    /// connection that the browser will refuse before it leaves the tab.
    #[test]
    fn reachability_follows_the_page_scheme() {
        // An https page can only reach TLS receivers.
        assert!(reachable_from_page(true, true));
        assert!(!reachable_from_page(true, false));

        // Served over http — or on the desktop — everything is reachable.
        assert!(reachable_from_page(false, true));
        assert!(reachable_from_page(false, false));
    }

    fn receiver(host: &str, tls: bool, users: u8, distance_km: f64) -> KiwiReceiver {
        KiwiReceiver {
            host: host.into(),
            port: if tls { 443 } else { 8073 },
            name: host.into(),
            location: "Somewhere".into(),
            lat: 0.0,
            lon: 0.0,
            users,
            users_max: 4,
            snr: 30,
            distance_km,
            tls,
        }
    }

    /// Reachability outranks occupancy: a full receiver frees up, one this
    /// page cannot reach never does. Within each group, nearest first.
    #[test]
    fn display_order_puts_unreachable_last_then_full() {
        let mut list = vec![
            receiver("plain-near", false, 0, 1.0),
            receiver("tls-full", true, 4, 5.0),
            receiver("tls-far", true, 0, 900.0),
            receiver("tls-near", true, 0, 10.0),
        ];
        sort_for_display(&mut list, true);
        let order: Vec<&str> = list.iter().map(|r| r.host.as_str()).collect();
        assert_eq!(order, ["tls-near", "tls-far", "tls-full", "plain-near"]);

        // Over http nothing is unreachable, so only occupancy and distance
        // decide and the plain-http receiver comes first on distance.
        sort_for_display(&mut list, false);
        let order: Vec<&str> = list.iter().map(|r| r.host.as_str()).collect();
        assert_eq!(order, ["plain-near", "tls-near", "tls-far", "tls-full"]);
    }

    /// An unreachable row spends its space on the reason rather than on a
    /// distance and occupancy the user cannot act on.
    #[test]
    fn display_line_states_the_reason_when_unreachable() {
        let unreachable = receiver_line(&receiver("plain.example", false, 1, 12.0), true);
        assert_eq!(
            unreachable,
            "plain.example:8073 · no TLS — unreachable from https · Somewhere"
        );
        assert!(!unreachable.contains("12km"));
        assert!(!unreachable.contains("1/4"));

        // The same receiver is fine on an http page, and reads normally.
        assert_eq!(
            receiver_line(&receiver("plain.example", false, 1, 12.0), false),
            "plain.example:8073 · 12km 1/4 · Somewhere"
        );
    }

    /// Distance is omitted when unknown — geolocation is best-effort, and a
    /// leading "0km" would claim the receiver is next door.
    #[test]
    fn display_line_omits_unknown_distance_and_marks_full() {
        assert_eq!(
            receiver_line(&receiver("rx.test", true, 0, 0.0), true),
            "rx.test:443 · 0/4 · Somewhere"
        );
        assert_eq!(
            receiver_line(&receiver("rx.test", true, 4, 0.0), true),
            "rx.test:443 · FULL 4/4 · Somewhere"
        );
    }

    /// The all-unreachable banner: worth saying once at the top, and only when
    /// every entry really is a dead end.
    #[test]
    fn any_reachable_needs_only_one() {
        let all_plain = vec![receiver("a", false, 0, 0.0), receiver("b", false, 0, 0.0)];
        assert!(!any_reachable(&all_plain, true));
        assert!(any_reachable(&all_plain, false));

        let mixed = vec![receiver("a", false, 0, 0.0), receiver("b", true, 0, 0.0)];
        assert!(any_reachable(&mixed, true));

        // An empty list is not a page-wide failure; there is nothing to say.
        assert!(!any_reachable(&[], true));
    }

    #[test]
    fn parse_gps_parentheses() {
        let (lat, lon) = parse_gps("(51.317266, -2.950479)").expect("gps");
        assert!((lat - 51.317266).abs() < 1e-6);
        assert!((lon + 2.950479).abs() < 1e-6);
        assert!(parse_gps("bad").is_none());
    }

    #[test]
    fn haversine_same_point_is_zero() {
        assert!(haversine_km(51.5, -0.1, 51.5, -0.1).abs() < 1e-6);
    }

    #[test]
    fn location_matches_country_by_name_or_code() {
        let geo = GeoLocation {
            country: "Sweden".into(),
            country_code: "SE".into(),
            lat: 59.0,
            lon: 18.0,
        };
        assert!(location_matches_country("Stockholm, Sweden", &geo));
        assert!(location_matches_country("Somewhere, se", &geo));
        assert!(!location_matches_country("Berlin, Germany", &geo));
    }

    #[test]
    fn extract_json_array_from_js_wrapper() {
        let body = "var kiwisdr_com =\n[{\"a\":1}];";
        let json = extract_json_array(body).expect("extract");
        assert!(json.starts_with('['));
        assert!(json.contains("\"a\""));
    }

    #[test]
    fn sanitize_json_strips_trailing_commas() {
        let clean = sanitize_json_array("[{\"x\":1},]");
        assert!(!clean.contains(",]"));
    }

    #[test]
    fn rank_deprioritizes_full_receivers() {
        let geo = GeoLocation {
            country: "United Kingdom".into(),
            country_code: "GB".into(),
            lat: 51.5,
            lon: -0.1,
        };
        let mut list = vec![
            KiwiReceiver {
                host: "full.example".into(),
                port: 8073,
                name: "full".into(),
                location: "London, United Kingdom".into(),
                lat: 51.5,
                lon: -0.1,
                users: 4,
                users_max: 4,
                snr: 50,
                distance_km: 0.0,
                tls: false,
            },
            KiwiReceiver {
                host: "open.example".into(),
                port: 8073,
                name: "open".into(),
                location: "London, United Kingdom".into(),
                lat: 51.5,
                lon: -0.12,
                users: 1,
                users_max: 4,
                snr: 30,
                distance_km: 0.0,
                tls: false,
            },
        ];
        rank_by_proximity(&mut list, &geo);
        assert_eq!(list[0].host, "open.example");
    }
}
