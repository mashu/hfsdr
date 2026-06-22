//! Public KiwiSDR directory: geo lookup + receiver list sorted by proximity.
//!
//! The official list at kiwisdr.com/public is captcha-protected; we use the
//! community mirror at rx.linkfanel.net (Dyatlov map data) plus ip-api.com for
//! coarse geolocation. Results are cached under the app config directory.

use std::io::Read;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

const LIST_URL: &str = "http://rx.linkfanel.net/kiwisdr_com.js";
const GEO_URL: &str = "http://ip-api.com/json/?fields=status,country,countryCode,lat,lon";
const CACHE_FILE: &str = "kiwi_directory_v2.json";
const CACHE_MAX_AGE: Duration = Duration::from_secs(30 * 60);
const NEARBY_LIMIT: usize = 12;

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
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct CachedDirectory {
    fetched_at_secs: u64,
    geo: Option<GeoLocation>,
    receivers: Vec<KiwiReceiver>,
}

#[derive(Deserialize)]
struct GeoResponse {
    status: String,
    country: Option<String>,
    #[serde(rename = "countryCode")]
    country_code: Option<String>,
    lat: Option<f64>,
    lon: Option<f64>,
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
    if parsed.status != "success" {
        return Err("geo lookup unsuccessful".into());
    }
    Ok(GeoLocation {
        country: parsed.country.unwrap_or_else(|| "Unknown".into()),
        country_code: parsed.country_code.unwrap_or_else(|| "??".into()),
        lat: parsed.lat.ok_or("geo missing lat")?,
        lon: parsed.lon.ok_or("geo missing lon")?,
    })
}

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
        let Some((host, port)) = parse_kiwi_url(&entry.url) else {
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

fn parse_kiwi_url(url: &str) -> Option<(String, u16)> {
    let rest = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))?;
    let host_port = rest.split('/').next()?;
    if let Some((host, port_s)) = host_port.rsplit_once(':') {
        let port: u16 = port_s.parse().ok()?;
        if host.is_empty() {
            return None;
        }
        Some((host.to_string(), port))
    } else if !host_port.is_empty() {
        Some((host_port.to_string(), 8073))
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
}
