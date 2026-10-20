//! Remote images (avatars, drop rewards, box art): fetched once in the background,
//! decoded off the UI thread, kept in memory for the session. Nothing is written to disk.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::Duration;

use slint::{Image, Rgba8Pixel, SharedPixelBuffer};
use tokio::runtime::Handle;

/// Images are shown small: larger ones are scaled down to save memory.
const MAX_SIDE: u32 = 256;
/// Bigger downloads are ignored.
const MAX_BYTES: usize = 4 * 1024 * 1024;

enum Entry {
    Loading,
    Ready(Image),
    Failed,
}

/// Runtime to download on, and what to do when an image arrived.
type Loader = (Handle, Box<dyn Fn()>);

thread_local! {
    static CACHE: RefCell<HashMap<String, Entry>> = RefCell::new(HashMap::new());
    static LOADER: RefCell<Option<Loader>> = const { RefCell::new(None) };
}

/// `on_loaded` runs on the UI thread each time an image arrives (re-render).
pub fn init(rt: Handle, on_loaded: impl Fn() + 'static) {
    LOADER.with(|l| *l.borrow_mut() = Some((rt, Box::new(on_loaded))));
}

/// The image at `url`, or an empty image while it loads (or if it cannot be loaded).
pub fn get(url: Option<&str>) -> Image {
    let Some(url) = url.filter(|u| u.starts_with("https://")) else {
        return Image::default();
    };
    let cached = CACHE.with(|c| match c.borrow().get(url) {
        Some(Entry::Ready(img)) => Some(Some(img.clone())),
        Some(_) => Some(None),
        None => None,
    });
    match cached {
        Some(img) => img.unwrap_or_default(),
        None => {
            fetch(url.to_string());
            Image::default()
        }
    }
}

fn fetch(url: String) {
    CACHE.with(|c| c.borrow_mut().insert(url.clone(), Entry::Loading));
    let Some(rt) = LOADER.with(|l| l.borrow().as_ref().map(|(rt, _)| rt.clone())) else {
        return;
    };
    rt.spawn(async move {
        let pixels = download(&url).await;
        let _ = slint::invoke_from_event_loop(move || {
            let entry = match pixels {
                Some(p) => Entry::Ready(Image::from_rgba8(p)),
                None => Entry::Failed,
            };
            CACHE.with(|c| c.borrow_mut().insert(url, entry));
            LOADER.with(|l| {
                if let Some((_, on_loaded)) = l.borrow().as_ref() {
                    on_loaded();
                }
            });
        });
    });
}

async fn download(url: &str) -> Option<SharedPixelBuffer<Rgba8Pixel>> {
    static CLIENT: OnceLock<Option<reqwest::Client>> = OnceLock::new();
    let client = CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .timeout(Duration::from_secs(20))
                .build()
                .ok()
        })
        .as_ref()?;
    let resp = client.get(url).send().await.ok()?.error_for_status().ok()?;
    let bytes = resp.bytes().await.ok()?;
    if bytes.len() > MAX_BYTES {
        return None;
    }
    tokio::task::spawn_blocking(move || decode(&bytes))
        .await
        .ok()?
}

fn decode(bytes: &[u8]) -> Option<SharedPixelBuffer<Rgba8Pixel>> {
    let mut img = image::load_from_memory(bytes).ok()?;
    if img.width() > MAX_SIDE || img.height() > MAX_SIDE {
        img = img.thumbnail(MAX_SIDE, MAX_SIDE);
    }
    let rgba = img.to_rgba8();
    Some(SharedPixelBuffer::clone_from_slice(
        rgba.as_raw(),
        rgba.width(),
        rgba.height(),
    ))
}
