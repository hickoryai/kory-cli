use super::keys::Key;
use std::sync::Arc;

pub fn is_capable() -> bool {
    false
}

pub fn screen_is_open() -> bool {
    false
}

pub type DirectKeys = Arc<dyn Fn(Key) -> Option<Key> + Send + Sync>;

pub struct KeyListener;

impl KeyListener {
    pub fn start() -> std::io::Result<Self> {
        Err(std::io::ErrorKind::Unsupported.into())
    }

    pub fn start_direct(_direct: DirectKeys) -> std::io::Result<Self> {
        Self::start()
    }

    pub fn set_ctrl_keys(&self, _ctrl_keys: bool) -> std::io::Result<()> {
        Ok(())
    }

    pub fn is_direct(&self) -> bool {
        false
    }

    pub async fn next(&mut self) -> Option<Key> {
        None
    }

    pub fn handled(&mut self) {}
}

pub struct Screen;

impl Screen {
    pub fn enter() -> Self {
        Self
    }

    pub fn size(&self) -> (usize, usize) {
        (0, 0)
    }

    pub fn draw(&self, _lines: &[String]) {}
}
