//! No pane off unix: the session keeps its plain output and line editor.

use std::sync::Arc;

pub type Painter = Arc<dyn Fn(usize, usize) -> Vec<String> + Send + Sync>;

pub fn install(_painter: Painter) -> bool {
    false
}

pub fn remove_painter() {}

pub fn repaint() {}

pub fn suspend() {}

pub fn resume() {}
