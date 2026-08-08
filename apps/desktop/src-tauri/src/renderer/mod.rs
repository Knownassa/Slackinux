#[cfg(feature = "cef")]
pub mod cef;
pub mod webkit;

use crate::error::AppResult;

pub trait SlackRenderer: Send + Sync {
    fn navigate(&self, url: &str) -> AppResult<()>;
    fn set_zoom_level(&self, level: f64) -> AppResult<()>;
    fn eval(&self, js: &str) -> AppResult<()>;
    fn reload(&self) -> AppResult<()>;
    fn clear_cache(&self) -> AppResult<()>;
    fn media_playing(&self) -> bool;
    /// Runs the Huddle codec/media-API probe inside the renderer and reports
    /// the JSON result (or `None` if the probe could not run).
    #[cfg(target_os = "linux")]
    fn probe_media_codecs(&self, on_result: Box<dyn FnOnce(Option<String>) + Send + 'static>);
}
