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
    parse_receiver_list, rank_by_proximity, GeoLocation, GeoResponse, KiwiReceiver,
    GEO_URL, LIST_URL, NEARBY_LIMIT,
};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

/// Cached list, so reopening the drawer does not refetch.
const CACHE_KEY: &str = "hfsdr.kiwi_directory.v2";

type Directory = (Option<GeoLocation>, Vec<KiwiReceiver>);

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

fn read_cache() -> Option<Directory> {
    let raw = storage()?.get_item(CACHE_KEY).ok().flatten()?;
    serde_json::from_str::<super::CachedDirectory>(&raw)
        .ok()
        .map(|c| (c.geo, c.receivers))
}

fn write_cache(dir: &Directory) {
    let Some(store) = storage() else { return };
    let cached = super::CachedDirectory {
        fetched_at_secs: 0,
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

async fn fetch_directory() -> Result<Directory, String> {
    // The list is the point; geo only sorts it.
    let list = get_text(LIST_URL).await?;
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
    if let Some(g) = &geo {
        rank_by_proximity(&mut receivers, g);
    }
    receivers.truncate(NEARBY_LIMIT);
    Ok((geo, receivers))
}

/// Fetch the directory and deliver it on `tx`, cache first unless forced.
pub fn start(tx: std::sync::mpsc::Sender<Result<Directory, String>>, force_refresh: bool) {
    if !force_refresh {
        if let Some(cached) = read_cache() {
            let _ = tx.send(Ok(cached));
            return;
        }
    }
    wasm_bindgen_futures::spawn_local(async move {
        let result = fetch_directory().await;
        if let Ok(dir) = &result {
            write_cache(dir);
        }
        let _ = tx.send(result);
    });
}
