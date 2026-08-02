pub mod webkit;

use crate::error::AppResult;

pub trait SlackRenderer: Send + Sync {
    fn navigate(&self, url: &str) -> AppResult<()>;
    fn set_zoom_level(&self, level: f64) -> AppResult<()>;
    fn eval(&self, js: &str) -> AppResult<()>;
    fn reload(&self) -> AppResult<()>;
    fn clear_cache(&self) -> AppResult<()>;
    fn media_playing(&self) -> bool;
}
