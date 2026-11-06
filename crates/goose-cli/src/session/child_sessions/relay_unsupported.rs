//! No pane off unix: the session keeps its plain output and line editor.

use std::sync::Arc;

pub type Painter = Arc<dyn Fn(usize, usize) -> Vec<String> + Send + Sync>;

pub fn install(_painter: Painter) -> bool {
    false
}

pub fn begin() -> bool {
    false
}

pub fn remove_painter() {}

pub fn show_child(_transcript: &str, _painter: Painter) -> bool {
    false
}

pub fn hide_child() {}

pub fn is_active() -> bool {
    false
}

pub fn repaint() {}

pub fn suspend() {}

pub fn resume() {}
