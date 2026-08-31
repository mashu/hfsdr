//! Browser directory fetch.
//!
//! Everything that turns a response into a sorted receiver list is shared with
//! the desktop path; only getting the bytes differs. Geolocation is best-effort
//! and never blocks the list: without it receivers are simply unsorted.
//!
//! Its own file so coverage can exclude it by path: this never compiles for
//! the x86 target the coverage job builds, so llvm-cov cannot instrument a
//! line of it and it would read as permanently uncovered.
use super::{
    parse_receiver_list, GeoLocation, GeoResponse, KiwiReceiver, GEO_URL, LIST_URL,
};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

/// Cached list, so reopening the drawer does not refetch.
/// Bumped when the *selection* changes, not only the shape of the data.
///
/// A cache written before `select_nearby` existed holds twelve receivers
/// chosen by distance alone and already truncated, so re-selecting it cannot
/// bring back the reachable receivers that were discarded. Only a refetch can,
/// and only a new key forces one.
const CACHE_KEY: &str = "hfsdr.kiwi_directory.v3";

type Directory = (Option<GeoLocation>, Vec<KiwiReceiver>);

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

fn now_secs() -> u64 {
    (js_sys::Date::now() / 1000.0) as u64
}

fn read_cache() -> Option<Directory> {
    let raw = storage()?.get_item(CACHE_KEY).ok().flatten()?;
    let cached = serde_json::from_str::<super::CachedDirectory>(&raw).ok()?;
    // Previously this cache had no timestamp and no expiry, so a list survived
    // in localStorage indefinitely — across deploys, across fixes to the
    // selection, until the user happened to press Refresh. Occupancy alone
    // makes a day-old list wrong.
    if now_secs().saturating_sub(cached.fetched_at_secs) > super::CACHE_MAX_AGE.as_secs() {
        return None;
    }
    Some((cached.geo, cached.receivers))
}

fn write_cache(dir: &Directory) {
    let Some(store) = storage() else { return };
    let cached = super::CachedDirectory {
        fetched_at_secs: now_secs(),
        geo: dir.0.clone(),
        receivers: dir.1.clone(),
    };
    if let Ok(text) = serde_json::to_string(&cached) {
        let _ = store.set_item(CACHE_KEY, &text);
    }
}

/// Body of a GET, or a message that says what actually went wrong.
async fn get_text(url: &str) -> Result<String, String> {
    let window = web_sys::window().ok_or("no window")?;
    let response = JsFuture::from(window.fetch_with_str(url))
        .await
        .map_err(|e| describe(url, &e))?
        .dyn_into::<web_sys::Response>()
        .map_err(|_| "fetch returned a non-Response".to_string())?;
    if !response.ok() {
        return Err(format!("{url}: HTTP {}", response.status()));
    }
    let text = JsFuture::from(response.text().map_err(|_| "no response body")?)
        .await
        .map_err(|e| describe(url, &e))?;
    text.as_string().ok_or_else(|| "response was not text".into())
}

/// A rejected `fetch` deliberately says nothing about why, so name the two
/// causes the user can act on rather than repeating an empty error.
fn describe(url: &str, err: &wasm_bindgen::JsValue) -> String {
    let detail = err
        .as_string()
        .or_else(|| js_sys::Reflect::get(err, &"message".into()).ok()?.as_string())
        .unwrap_or_default();
    let hint = if url.starts_with("http://") && super::web_page_is_https() {
        " — this page is https, so a plain-http request is blocked as mixed content"
    } else {
        " — the host may be down, or may not allow cross-origin requests"
    };
    format!("could not reach {url}{}{hint}", if detail.is_empty() { String::new() } else { format!(": {detail}") })
}

/// Same-origin copy of the directory, baked in at build time.
///
/// The live host is third-party, and whether it allows cross-origin requests
/// is not ours to control — a page whose receiver list depends on someone
/// else's CORS policy is a page that shows an empty list. This copy is fetched
/// by CI and served from our own origin, where neither CORS nor mixed content
/// applies.
const BUNDLED_LIST: &str = "./receivers.js";

/// Whether the live directory URL is reachable from this page at all.
///
/// It is plain http, so an https page cannot request it — the browser blocks
/// it as mixed content before anything else happens. Trying anyway produces a
/// misleading "failed to fetch" that reads like the host is down, so don't.
/// Served over http (local development), it works normally.
fn live_list_is_reachable() -> bool {
    !super::web_page_is_https()
}

/// The bundled list, falling back to the live one only where that can work.
///
/// Bundled wins on first load because it always works. On an http page the
/// live URL is also tried, and preferred on an explicit refresh, so local
/// development is not pinned to whatever the last deployment captured.
async fn fetch_list(force_refresh: bool) -> Result<String, String> {
    let live_ok = live_list_is_reachable();
    if force_refresh && live_ok {
        match get_text(LIST_URL).await {
            Ok(live) => return Ok(live),
            Err(e) => crate::log::warn(format!(
                "kiwi directory: live refresh failed ({e}), using the bundled list"
            )),
        }
    }
    match get_text(BUNDLED_LIST).await {
        Ok(list) => Ok(list),
        Err(bundled_err) if live_ok => get_text(LIST_URL)
            .await
            .map_err(|live_err| format!("{bundled_err}; live list also failed: {live_err}")),
        Err(bundled_err) => Err(format!(
            "{bundled_err}. The list is normally built into this deployment; \
             the directory host serves plain http only, so this page cannot \
             fetch it directly over https."
        )),
    }
}

async fn fetch_directory(force_refresh: bool) -> Result<Directory, String> {
    // The list is the point; geo only sorts it.
    let list = fetch_list(force_refresh).await?;
    let mut receivers = parse_receiver_list(&list)?;

    let geo = match get_text(GEO_URL).await {
        Ok(body) => serde_json::from_str::<GeoResponse>(&body)
            .ok()
            .and_then(|g| g.into_location().ok()),
        Err(e) => {
            crate::log::warn(format!("kiwi directory: {e}"));
            None
        }
    };
    // The page's scheme decides which receivers are worth a slot at all, so it
    // has to be known before the list is cut to size.
    super::select_nearby(&mut receivers, geo.as_ref(), super::web_page_is_https());
    Ok((geo, receivers))
}

/// Fetch the directory and deliver it on `tx`, cache first unless forced.
pub fn start(tx: std::sync::mpsc::Sender<Result<Directory, String>>, force_refresh: bool) {
    if !force_refresh {
        if let Some((geo, mut receivers)) = read_cache() {
            // Re-select rather than replaying the cache verbatim. Which
            // receivers are reachable depends on the scheme of the page doing
            // the asking, and the same browser profile can load this app over
            // http locally and https from Pages.
            super::select_nearby(&mut receivers, geo.as_ref(), super::web_page_is_https());
            let _ = tx.send(Ok((geo, receivers)));
            return;
        }
    }
    wasm_bindgen_futures::spawn_local(async move {
        let result = fetch_directory(force_refresh).await;
        if let Ok(dir) = &result {
            write_cache(dir);
        }
        let _ = tx.send(result);
    });
}
