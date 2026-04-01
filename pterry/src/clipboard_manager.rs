use arboard::Clipboard;
use chrono::{DateTime, Local};
use crossbeam_channel::{Receiver, Sender};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone)]
pub enum ClipboardContent {
    Text(String),
    Image {
        width: usize,
        height: usize,
        rgba: Vec<u8>,
    },
}

#[derive(Debug, Clone)]
pub struct ClipboardItem {
    pub content: ClipboardContent,
    pub timestamp: DateTime<Local>,
}

pub struct ClipboardManager {
    /// The inner Arc<Vec<_>> is replaced atomically on each write so callers
    /// can clone the Arc cheaply instead of cloning the whole Vec.
    history: Arc<Mutex<Arc<Vec<ClipboardItem>>>>,
    clipboard: Arc<Mutex<Clipboard>>,
    sender: Sender<ClipboardEvent>,
    receiver: Receiver<ClipboardEvent>,
}

#[derive(Debug, Clone)]
pub enum ClipboardEvent {
    NewItem(ClipboardItem),
    Error(String),
}

/// Hash the first 1 KB + total length — fast enough for 500 ms polling.
fn quick_image_hash(bytes: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    bytes.len().hash(&mut hasher);
    bytes[..bytes.len().min(1024)].hash(&mut hasher);
    hasher.finish()
}

impl ClipboardManager {
    pub fn new() -> Result<Self, String> {
        let clipboard = Clipboard::new().map_err(|e| format!("Failed to access clipboard: {e}"))?;

        let (sender, receiver) = crossbeam_channel::unbounded();

        Ok(Self {
            history: Arc::new(Mutex::new(Arc::new(Vec::new()))),
            clipboard: Arc::new(Mutex::new(clipboard)),
            sender,
            receiver,
        })
    }

    pub fn start_monitoring(&self) {
        let history = Arc::clone(&self.history);
        let clipboard = Arc::clone(&self.clipboard);
        let sender = self.sender.clone();

        thread::spawn(move || {
            let mut last_text = String::new();
            let mut last_image_hash: u64 = 0;

            loop {
                let text_result = clipboard
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get_text();

                match text_result {
                    Ok(text) => {
                        last_image_hash = 0; // clipboard switched to text
                        if !text.is_empty() && text != last_text {
                            last_text = text.clone();

                            let item = ClipboardItem {
                                content: ClipboardContent::Text(text.clone()),
                                timestamp: Local::now(),
                            };

                            if let Ok(mut guard) = history.lock() {
                                let mut h = (**guard).clone();
                                h.retain(|i| {
                                    !matches!(&i.content, ClipboardContent::Text(t) if *t == text)
                                });
                                h.insert(0, item.clone());
                                if h.len() > 100 {
                                    h.truncate(100);
                                }
                                *guard = Arc::new(h);
                            }

                            let _ = sender.send(ClipboardEvent::NewItem(item));
                        }
                    }
                    Err(e) => {
                        let msg = e.to_string();
                        if msg.contains("incorrect type") {
                            // Clipboard holds non-text content — try image.
                            if let Ok(img) = clipboard
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .get_image()
                            {
                                let hash = quick_image_hash(&img.bytes);
                                if hash != last_image_hash {
                                    last_image_hash = hash;
                                    last_text.clear();

                                    let rgba = img.bytes.into_owned();
                                    let item = ClipboardItem {
                                        content: ClipboardContent::Image {
                                            width: img.width,
                                            height: img.height,
                                            rgba,
                                        },
                                        timestamp: Local::now(),
                                    };

                                    if let Ok(mut guard) = history.lock() {
                                        let mut h = (**guard).clone();
                                        h.insert(0, item.clone());
                                        if h.len() > 100 {
                                            h.truncate(100);
                                        }
                                        *guard = Arc::new(h);
                                    }

                                    let _ = sender.send(ClipboardEvent::NewItem(item));
                                }
                            } // not text, not image — ignore
                        } else if msg.contains("No selection")
                            || msg.contains("empty")
                            || msg.contains("no content")
                            || msg.contains("clipboard is empty")
                        {
                            // Clipboard is simply empty — silently ignore.
                        } else {
                            let _ = sender
                                .send(ClipboardEvent::Error(format!("Clipboard error: {msg}")));
                        }
                    }
                }

                thread::sleep(Duration::from_millis(500));
            }
        });
    }

    pub fn get_history(&self) -> Arc<Vec<ClipboardItem>> {
        Arc::clone(&*self.history.lock().unwrap_or_else(|e| e.into_inner()))
    }

    pub fn get_receiver(&self) -> &Receiver<ClipboardEvent> {
        &self.receiver
    }

    pub fn copy_to_clipboard(&self, text: &str) -> Result<(), String> {
        self.clipboard
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .set_text(text)
            .map_err(|e| format!("Failed to copy to clipboard: {e}"))
    }

    pub fn copy_image_to_clipboard(
        &self,
        width: usize,
        height: usize,
        rgba: &[u8],
    ) -> Result<(), String> {
        let img = arboard::ImageData {
            width,
            height,
            bytes: std::borrow::Cow::Borrowed(rgba),
        };
        self.clipboard
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .set_image(img)
            .map_err(|e| format!("Failed to copy image to clipboard: {e}"))
    }

    /// Directly populate the history — only used in unit tests.
    #[cfg(test)]
    pub fn seed_history_for_test(&self, items: Vec<ClipboardItem>) {
        *self.history.lock().unwrap_or_else(|e| e.into_inner()) = Arc::new(items);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn make_text_item(text: &str) -> ClipboardItem {
        ClipboardItem {
            content: ClipboardContent::Text(text.to_string()),
            timestamp: chrono::Local::now(),
        }
    }

    /// get_history() must return an Arc so that repeated calls without intervening
    /// writes share the same allocation (no Vec clone per call).
    #[test]
    fn get_history_returns_same_arc_when_unchanged() {
        let manager = ClipboardManager::new().expect("should create manager");
        manager.seed_history_for_test(vec![make_text_item("hello")]);
        let h1 = manager.get_history();
        let h2 = manager.get_history();
        assert!(Arc::ptr_eq(&h1, &h2));
    }

    #[test]
    fn get_history_reflects_seeded_items() {
        let manager = ClipboardManager::new().expect("should create manager");
        manager.seed_history_for_test(vec![make_text_item("a"), make_text_item("b")]);
        let history = manager.get_history();
        assert_eq!(history.len(), 2);
    }
}
