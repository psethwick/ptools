use crate::extension_trait::{DetailMetadataRow, ExtensionItem, TagItem};
use egui::*;
use std::collections::{HashMap, VecDeque};
use std::io::Read as _;
use std::time::{Duration, Instant};

/// Completed URL icon fetch results shared between the render loop and background threads.
/// Each entry is `(url, raw_image_bytes)`.
type UrlFetchQueue = Vec<(String, Vec<u8>)>;

/// A capacity-bounded cache with FIFO eviction.  When the cache is full and a
/// new key is inserted, the oldest-inserted entry is discarded first.  Updating
/// an existing key does **not** count as a new insertion.
///
/// Used for GPU texture handles to prevent unbounded VRAM growth over long
/// sessions (spec §13: max 256 entries).
struct BoundedCache<K, V> {
    capacity: usize,
    map: HashMap<K, V>,
    /// Insertion order; front = oldest, back = newest.
    order: VecDeque<K>,
}

impl<K: std::hash::Hash + Eq + Clone, V> BoundedCache<K, V> {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            capacity,
            map: HashMap::with_capacity(capacity + 1),
            order: VecDeque::with_capacity(capacity + 1),
        }
    }

    fn contains_key<Q>(&self, key: &Q) -> bool
    where
        K: std::borrow::Borrow<Q>,
        Q: std::hash::Hash + Eq + ?Sized,
    {
        self.map.contains_key(key)
    }

    fn get<Q>(&self, key: &Q) -> Option<&V>
    where
        K: std::borrow::Borrow<Q>,
        Q: std::hash::Hash + Eq + ?Sized,
    {
        self.map.get(key)
    }

    /// Insert `key → value`.  If `key` already exists, the value is updated
    /// in place without changing insertion order.  If the cache is at capacity
    /// and `key` is new, the oldest entry is evicted first.
    fn insert(&mut self, key: K, value: V) {
        if !self.map.contains_key(&key) {
            if self.map.len() >= self.capacity
                && let Some(evicted) = self.order.pop_front()
            {
                self.map.remove(&evicted);
            }
            self.order.push_back(key.clone());
        }
        self.map.insert(key, value);
    }
}

pub struct Action {
    pub title: String,
    pub shortcut: Option<String>,
    pub icon: Option<String>,
    /// The action string that will be passed to `App::execute_action()` when
    /// this action is triggered. Empty string means use the `handler` closure.
    pub action_str: String,
    pub handler: Box<dyn Fn() + Send + Sync>,
}

pub struct ActionPanel {
    pub actions: Vec<Action>,
    pub selected_index: Option<usize>,
    /// Filled by `execute_selected`; drained each frame by `App::update`.
    pending_action: Option<String>,
    pub is_open: bool,
}

impl ActionPanel {
    pub fn new() -> Self {
        Self {
            actions: Vec::new(),
            selected_index: None,
            is_open: false,
            pending_action: None,
        }
    }

    pub fn set_actions(&mut self, actions: Vec<Action>) {
        self.actions = actions;
        self.selected_index = if !self.actions.is_empty() {
            Some(0)
        } else {
            None
        };
    }

    pub fn open(&mut self) {
        self.is_open = true;
        self.selected_index = if !self.actions.is_empty() {
            Some(0)
        } else {
            None
        };
    }

    pub fn close(&mut self) {
        self.is_open = false;
        self.selected_index = None;
    }

    pub fn select_next(&mut self) {
        if !self.actions.is_empty() {
            let new_idx = self
                .selected_index
                .map(|idx| (idx + 1).min(self.actions.len() - 1))
                .unwrap_or(0);
            self.selected_index = Some(new_idx);
        }
    }

    pub fn select_prev(&mut self) {
        if !self.actions.is_empty() {
            let new_idx = self
                .selected_index
                .map(|idx| idx.saturating_sub(1))
                .unwrap_or(0);
            self.selected_index = Some(new_idx);
        }
    }

    pub fn execute_selected(&mut self) {
        if let Some(index) = self.selected_index
            && let Some(action) = self.actions.get(index)
        {
            if !action.action_str.is_empty() {
                self.pending_action = Some(action.action_str.clone());
            }
            (action.handler)();
        }
    }

    /// Returns the action string queued by the most recent `execute_selected`
    /// call, clearing it in the process. Called each frame by `App::update`.
    pub fn take_pending_action(&mut self) -> Option<String> {
        self.pending_action.take()
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        if !self.is_open {
            return;
        }

        let _area = egui::Area::new(egui::Id::new("action_panel"))
            .fixed_pos(egui::pos2(
                ui.ctx().screen_rect().center().x - 200.0,
                ui.ctx().screen_rect().center().y - 150.0,
            ))
            .interactable(true)
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                let frame = egui::Frame::none()
                    .fill(egui::Color32::from_gray(30))
                    .stroke(egui::Stroke::new(1.0, egui::Color32::from_gray(60)))
                    .rounding(8.0)
                    .inner_margin(egui::Margin::same(8.0));

                frame.show(ui, |ui| {
                    ui.set_width(400.0);
                    ui.set_height(300.0);

                    ui.heading("Actions");
                    ui.separator();

                    egui::ScrollArea::vertical()
                        .max_height(250.0)
                        .show(ui, |ui| {
                            let mut clicked_index: Option<usize> = None;
                            let mut scroll_to: Option<usize> = None;
                            for (index, action) in self.actions.iter().enumerate() {
                                let is_selected = Some(index) == self.selected_index;
                                let response = self.render_action_item(ui, action, is_selected);
                                if response.clicked() {
                                    clicked_index = Some(index);
                                }
                                if is_selected {
                                    scroll_to = Some(index);
                                }
                                let _ = scroll_to; // used below
                            }
                            if let Some(idx) = scroll_to {
                                // Re-query the response via id to scroll — egui doesn't
                                // easily let us scroll after the loop, so we skip this
                                // for now and handle it in a future pass.
                                let _ = idx;
                            }
                            if let Some(idx) = clicked_index {
                                self.selected_index = Some(idx);
                                self.execute_selected();
                                self.close();
                            }
                        });
                });
            });
    }

    fn render_action_item(
        &self,
        ui: &mut Ui,
        action: &Action,
        is_selected: bool,
    ) -> egui::Response {
        let background_color = if is_selected {
            ui.visuals().selection.bg_fill
        } else {
            ui.visuals().extreme_bg_color
        };

        let response =
            ui.allocate_response(egui::vec2(ui.available_width(), 40.0), egui::Sense::click());

        let rect = response.rect;

        ui.painter().rect_filled(rect, 4.0, background_color);

        if let Some(icon) = &action.icon {
            ui.painter().text(
                rect.min + egui::vec2(8.0, 8.0),
                egui::Align2::LEFT_TOP,
                icon,
                egui::FontId::monospace(16.0),
                ui.visuals().text_color(),
            );
        }

        let title_pos = if action.icon.is_some() {
            rect.min + egui::vec2(40.0, 8.0)
        } else {
            rect.min + egui::vec2(8.0, 8.0)
        };

        ui.painter().text(
            title_pos,
            egui::Align2::LEFT_TOP,
            &action.title,
            egui::FontId::monospace(14.0),
            ui.visuals().text_color(),
        );

        if let Some(shortcut) = &action.shortcut {
            ui.painter().text(
                rect.max - egui::vec2(8.0, 8.0),
                egui::Align2::RIGHT_BOTTOM,
                shortcut,
                egui::FontId::monospace(12.0),
                egui::Color32::GRAY,
            );
        }

        response
    }
}

impl Default for ActionPanel {
    fn default() -> Self {
        Self::new()
    }
}

pub struct List {
    items: Vec<ExtensionItem>,
    selected_index: Option<usize>,
    /// Cached GPU textures keyed by item ID or icon string.
    /// Bounded to 256 entries (FIFO eviction) to prevent unbounded VRAM growth.
    texture_cache: BoundedCache<String, egui::TextureHandle>,
    /// Set to `true` by programmatic selection changes (`select_next`, `select_prev`,
    /// `set_items`). Cleared to `false` after each `ui()` call so that mouse-wheel
    /// scrolling is never overridden by an automatic re-center.
    scroll_to_selected: bool,
    /// Set by `ui()` when the user clicks a list item. Drained by
    /// `take_activated_action()` so the caller can execute it — mirrors the
    /// Enter-key path without duplicating `execute_action` logic inside the
    /// rendering closure.
    pending_action: Option<String>,
    /// URLs currently being fetched in background threads (deduplicate in-flight requests).
    pending_url_fetches: std::collections::HashSet<String>,
    /// Completed background URL fetches waiting to be decoded and uploaded to the GPU.
    /// Shared with spawned fetch threads; each entry is `(url, raw_image_bytes)`.
    url_fetch_results: std::sync::Arc<std::sync::Mutex<UrlFetchQueue>>,
}

/// Return the column count for grid rendering, or `None` if items are in list mode.
///
/// Checks the first non-section-header item for a non-zero `grid_columns` value.
/// All Grid items in a result set share the same column count (set by the parent
/// `<Grid>` component), so inspecting the first real item is sufficient.
pub(crate) fn grid_columns_for_items(items: &[ExtensionItem]) -> Option<u8> {
    items
        .iter()
        .filter(|i| i.action != "::section::")
        .find_map(|i| i.grid_columns.filter(|&c| c > 0))
}

impl List {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            selected_index: None,
            texture_cache: BoundedCache::with_capacity(256),
            scroll_to_selected: false,
            pending_action: None,
            pending_url_fetches: std::collections::HashSet::new(),
            url_fetch_results: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }

    /// Returns and clears the action string of the last item the user clicked,
    /// or `None` if no click occurred this frame.  Call this after `ui()`.
    pub fn take_activated_action(&mut self) -> Option<String> {
        self.pending_action.take()
    }

    /// Returns `true` if the item is a section-header sentinel (not selectable).
    fn is_section_header(item: &ExtensionItem) -> bool {
        item.action == "::section::"
    }

    /// Returns the `List.EmptyView` sentinel if it is the only non-section item
    /// in the list, or `None` if there are real items to display.
    fn empty_view(items: &[ExtensionItem]) -> Option<&ExtensionItem> {
        let real: Vec<&ExtensionItem> = items
            .iter()
            .filter(|i| !Self::is_section_header(i))
            .collect();
        if real.len() == 1 && real[0].id.as_deref() == Some("::empty::") {
            Some(real[0])
        } else {
            None
        }
    }

    pub fn set_items(&mut self, items: Vec<ExtensionItem>) {
        self.items = items;
        // Select the first *selectable* (non-section-header) item.
        self.selected_index = self.items.iter().position(|i| !Self::is_section_header(i));
        self.scroll_to_selected = true;
    }

    /// Return a copy of the current item list and selection index for saving
    /// into a [`NavFrame`] before pushing a new navigation level.
    pub fn snapshot(&self) -> (Vec<ExtensionItem>, Option<usize>) {
        (self.items.clone(), self.selected_index)
    }

    /// Restore item list and selection index from a previously saved snapshot.
    /// Unlike [`set_items`], this does not reset the selection to the first item.
    pub fn restore_snapshot(&mut self, items: Vec<ExtensionItem>, selected_index: Option<usize>) {
        self.items = items;
        self.selected_index = selected_index;
        self.scroll_to_selected = true;
    }

    pub fn selected_item(&self) -> Option<&ExtensionItem> {
        self.selected_index.and_then(|idx| self.items.get(idx))
    }

    pub fn select_next(&mut self) {
        let len = self.items.len();
        if len == 0 {
            return;
        }
        let start = self.selected_index.map(|i| i + 1).unwrap_or(0);
        // Find the next selectable item after the current one.
        let new_idx = (start..len)
            .find(|&i| !Self::is_section_header(&self.items[i]))
            .or(self.selected_index); // stay put if none found
        if let Some(idx) = new_idx {
            self.selected_index = Some(idx);
            self.scroll_to_selected = true;
        }
    }

    pub fn select_prev(&mut self) {
        let len = self.items.len();
        if len == 0 {
            return;
        }
        let end = self.selected_index.unwrap_or(0);
        // Find the previous selectable item before the current one.
        let new_idx = (0..end)
            .rev()
            .find(|&i| !Self::is_section_header(&self.items[i]))
            .or(self.selected_index); // stay put if none found
        if let Some(idx) = new_idx {
            self.selected_index = Some(idx);
            self.scroll_to_selected = true;
        }
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        // Pre-pass: collect only the data needed to upload new image thumbnails to
        // the GPU.  Most frames this list is empty (everything already cached), so
        // we avoid cloning the entire items vec while still handling new images.
        //
        // Two sources of textures:
        // 1. `thumbnail_rgba` — pre-decoded pixel data provided by the extension, keyed by item id.
        // 2. Icon strings that need image loading (file://, absolute path, data:image/ URI),
        //    decoded here on first appearance, keyed by the icon string itself.
        let to_upload: Vec<(usize, usize, Vec<u8>, String)> = self
            .items
            .iter()
            .flat_map(|item| {
                let mut candidates: Vec<(usize, usize, Vec<u8>, String)> = Vec::new();
                // Source 1: pre-decoded thumbnail
                if let (Some((w, h, rgba)), Some(id)) = (&item.thumbnail_rgba, &item.id)
                    && !self.texture_cache.contains_key(id)
                {
                    candidates.push((*w, *h, rgba.clone(), id.clone()));
                }
                // Source 2: icon string that encodes image data
                if item.thumbnail_rgba.is_none()
                    && let Some(icon) = &item.icon
                    && needs_image_load(icon)
                    && !self.texture_cache.contains_key(icon)
                    && let Some((w, h, rgba)) = resolve_icon_data(icon)
                {
                    candidates.push((w, h, rgba, icon.clone()));
                }
                candidates
            })
            .collect();

        for (w, h, rgba, key) in to_upload {
            let color_image = egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba);
            let texture =
                ui.ctx()
                    .load_texture(key.clone(), color_image, egui::TextureOptions::LINEAR);
            self.texture_cache.insert(key, texture);
        }

        // Drain completed URL fetches from background threads → upload as textures.
        {
            let completed: Vec<(String, Vec<u8>)> = self
                .url_fetch_results
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .drain(..)
                .collect();
            for (url, bytes) in completed {
                self.pending_url_fetches.remove(&url);
                if let Some((w, h, rgba)) = decode_image_bytes(&bytes) {
                    let color_image = egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba);
                    let texture = ui.ctx().load_texture(
                        url.clone(),
                        color_image,
                        egui::TextureOptions::LINEAR,
                    );
                    self.texture_cache.insert(url, texture);
                }
            }
        }

        // Dispatch background fetches for any URL icons not yet cached or in-flight.
        for item in &self.items {
            if let Some(icon) = &item.icon
                && is_url_icon(icon)
                && !self.texture_cache.contains_key(icon)
                && !self.pending_url_fetches.contains(icon)
            {
                self.pending_url_fetches.insert(icon.clone());
                let url = icon.clone();
                let results_arc = self.url_fetch_results.clone();
                let ctx = ui.ctx().clone();
                std::thread::spawn(move || {
                    if let Some(bytes) = fetch_url_icon(&url) {
                        results_arc
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .push((url, bytes));
                        ctx.request_repaint();
                    }
                });
            }
        }

        let selected_index = self.selected_index;
        let mut new_selected = selected_index;
        let mut new_pending_action: Option<String> = None;
        // Consume the flag so that only programmatic selection changes (arrow
        // keys, set_items) trigger auto-scroll.  Mouse-wheel scrolling must
        // never be overridden by a per-frame scroll_to_me call.
        let scroll_to_selected = std::mem::replace(&mut self.scroll_to_selected, false);

        // Bind individual fields before the closure so the borrow checker can
        // see that `items` and `texture_cache` are separate borrows of `self`.
        let items = &self.items;
        let texture_cache = &self.texture_cache;

        // If the extension provided a List.EmptyView sentinel as the only real
        // item, render a centred message instead of the normal scroll list.
        if let Some(ev) = Self::empty_view(items) {
            let title = ev.title.clone();
            let subtitle = ev.subtitle.clone();
            ui.centered_and_justified(|ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(20.0);
                    ui.label(egui::RichText::new(&title).strong());
                    if let Some(sub) = &subtitle {
                        ui.label(egui::RichText::new(sub).weak());
                    }
                });
            });
            return;
        }

        // Branch: multi-column grid or linear list.
        let cols = grid_columns_for_items(items).unwrap_or(0) as usize;
        if cols >= 2 {
            Self::render_grid(
                ui,
                items,
                cols,
                texture_cache,
                selected_index,
                scroll_to_selected,
                &mut new_selected,
                &mut new_pending_action,
            );
        } else {
            ScrollArea::vertical()
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    ui.vertical(|ui| {
                        for (index, item) in items.iter().enumerate() {
                            if Self::is_section_header(item) {
                                // Render a non-interactive dimmed section label.
                                ui.add_space(4.0);
                                ui.label(
                                    egui::RichText::new(&item.title)
                                        .small()
                                        .color(ui.visuals().weak_text_color()),
                                );
                                ui.add_space(2.0);
                                continue;
                            }

                            let is_selected = Some(index) == selected_index;
                            let response = Self::render_item(ui, item, is_selected, texture_cache);

                            if response.clicked() {
                                new_selected = Some(index);
                                new_pending_action = Some(item.action.clone());
                            }

                            if is_selected && scroll_to_selected {
                                response.scroll_to_me(Some(Align::Center));
                            }
                        }
                    });
                });
        }

        self.selected_index = new_selected;
        self.pending_action = new_pending_action;
    }

    /// Render items as a multi-column tile grid (used when `grid_columns` is set).
    ///
    /// Section headers span full width; selectable items are laid out in rows of
    /// `cols` tiles.  Each tile is square: icon/thumbnail fills the upper portion
    /// and a truncated title sits at the bottom.
    #[allow(clippy::too_many_arguments)]
    fn render_grid(
        ui: &mut Ui,
        items: &[ExtensionItem],
        cols: usize,
        texture_cache: &BoundedCache<String, egui::TextureHandle>,
        selected_index: Option<usize>,
        scroll_to_selected: bool,
        new_selected: &mut Option<usize>,
        new_pending_action: &mut Option<String>,
    ) {
        const GAP: f32 = 8.0;
        let available_w = ui.available_width() - GAP; // leave a small right margin
        let tile_size = ((available_w - GAP * (cols as f32 - 1.0)) / cols as f32).max(40.0);

        ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    // Process items in chunks: section headers flush-left,
                    // then groups of `cols` grid tiles per row.
                    let mut pending_row: Vec<usize> = Vec::with_capacity(cols);

                    let flush_row =
                        |ui: &mut Ui,
                         row: &[usize],
                         items: &[ExtensionItem],
                         texture_cache: &BoundedCache<String, egui::TextureHandle>,
                         selected_index: Option<usize>,
                         scroll_to_selected: bool,
                         new_selected: &mut Option<usize>,
                         new_pending_action: &mut Option<String>| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = GAP;
                                for &idx in row {
                                    let item = &items[idx];
                                    let is_selected = Some(idx) == selected_index;
                                    let resp = Self::render_tile(
                                        ui,
                                        item,
                                        is_selected,
                                        tile_size,
                                        texture_cache,
                                    );
                                    if resp.clicked() {
                                        *new_selected = Some(idx);
                                        *new_pending_action = Some(item.action.clone());
                                    }
                                    if is_selected && scroll_to_selected {
                                        resp.scroll_to_me(Some(Align::Center));
                                    }
                                }
                            });
                            ui.add_space(GAP);
                        };

                    for (index, item) in items.iter().enumerate() {
                        if Self::is_section_header(item) {
                            // Flush pending row before the section break.
                            if !pending_row.is_empty() {
                                flush_row(
                                    ui,
                                    &pending_row,
                                    items,
                                    texture_cache,
                                    selected_index,
                                    scroll_to_selected,
                                    new_selected,
                                    new_pending_action,
                                );
                                pending_row.clear();
                            }
                            ui.add_space(4.0);
                            ui.label(
                                egui::RichText::new(&item.title)
                                    .small()
                                    .color(ui.visuals().weak_text_color()),
                            );
                            ui.add_space(2.0);
                            continue;
                        }

                        pending_row.push(index);
                        if pending_row.len() == cols {
                            flush_row(
                                ui,
                                &pending_row,
                                items,
                                texture_cache,
                                selected_index,
                                scroll_to_selected,
                                new_selected,
                                new_pending_action,
                            );
                            pending_row.clear();
                        }
                    }

                    // Flush any partial last row.
                    if !pending_row.is_empty() {
                        flush_row(
                            ui,
                            &pending_row,
                            items,
                            texture_cache,
                            selected_index,
                            scroll_to_selected,
                            new_selected,
                            new_pending_action,
                        );
                    }
                });
            });
    }

    /// Render a single square grid tile.
    fn render_tile(
        ui: &mut Ui,
        item: &ExtensionItem,
        is_selected: bool,
        tile_size: f32,
        texture_cache: &BoundedCache<String, egui::TextureHandle>,
    ) -> Response {
        let bg = if is_selected {
            ui.visuals().selection.bg_fill
        } else {
            ui.visuals().extreme_bg_color
        };

        let response = ui.allocate_response(Vec2::splat(tile_size), Sense::click());
        let rect = response.rect;

        ui.painter().rect_filled(rect, 6.0, bg);

        let title_h = 18.0; // reserved for title text at the bottom
        let icon_area = rect.shrink2(Vec2::new(8.0, 4.0));
        let icon_rect = Rect::from_min_size(
            icon_area.min,
            Vec2::new(icon_area.width(), icon_area.height() - title_h),
        );

        // Draw thumbnail or icon glyph.
        // Prefer thumbnail keyed by item id; fall back to icon-string texture.
        let grid_texture: Option<&egui::TextureHandle> = item
            .id
            .as_ref()
            .and_then(|id| texture_cache.get(id))
            .or_else(|| {
                item.icon
                    .as_deref()
                    .filter(|s| needs_image_load(s) || is_url_icon(s))
                    .and_then(|s| texture_cache.get(s))
            });

        if let Some(texture) = grid_texture {
            let (tw, th) = if let Some((w, h, _)) = &item.thumbnail_rgba {
                let aspect = *w as f32 / (*h as f32).max(0.001);
                let max_dim = icon_rect.width().min(icon_rect.height());
                if aspect > 1.0 {
                    (max_dim, max_dim / aspect)
                } else {
                    (max_dim * aspect, max_dim)
                }
            } else {
                let s = icon_rect.width().min(icon_rect.height());
                (s, s)
            };
            let center = icon_rect.center();
            let thumb_rect = Rect::from_center_size(center, Vec2::new(tw, th));
            ui.painter().image(
                texture.id(),
                thumb_rect,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        } else if let Some(icon) = &item.icon
            && icon.chars().count() <= 2
        {
            ui.painter().text(
                icon_rect.center(),
                Align2::CENTER_CENTER,
                icon,
                FontId::proportional((icon_rect.height() * 0.6).min(36.0)),
                ui.visuals().text_color(),
            );
        }

        // Title at bottom, clipped to tile width.
        let title_pos = Pos2::new(rect.min.x + 4.0, rect.max.y - title_h + 2.0);
        let max_title_w = tile_size - 8.0;
        let galley = ui.fonts(|f| {
            f.layout(
                item.title.clone(),
                FontId::monospace(11.0),
                ui.visuals().text_color(),
                max_title_w,
            )
        });
        ui.painter()
            .galley(title_pos, galley, ui.visuals().text_color());

        response
    }

    fn render_item(
        ui: &mut Ui,
        item: &ExtensionItem,
        is_selected: bool,
        texture_cache: &BoundedCache<String, egui::TextureHandle>,
    ) -> Response {
        let has_thumb = item.thumbnail_rgba.is_some();
        // Also treat icon-string images (file://, absolute path, data:image/, http/https) as
        // thumbnails once they've been uploaded to the texture cache.
        let icon_is_image = item
            .icon
            .as_deref()
            .is_some_and(|s| (needs_image_load(s) || is_url_icon(s)) && texture_cache.contains_key(s));
        let row_height = if has_thumb || icon_is_image { 64.0 } else { 50.0 };
        let icon_col_width = if has_thumb || icon_is_image { 72.0 } else { 40.0 };

        let background_color = if is_selected {
            ui.visuals().selection.bg_fill
        } else {
            ui.visuals().extreme_bg_color
        };

        let response =
            ui.allocate_response(Vec2::new(ui.available_width(), row_height), Sense::click());
        let rect = response.rect;

        ui.painter().rect_filled(rect, 0.0, background_color);

        let text_color = ui.visuals().text_color();

        // Look up the texture: prefer thumbnail (keyed by item id), then icon string
        // (file://, absolute path, data:image URI, or http/https URL once fetched).
        let thumb_texture: Option<&egui::TextureHandle> = item
            .id
            .as_ref()
            .and_then(|id| texture_cache.get(id))
            .or_else(|| {
                item.icon
                    .as_deref()
                    .filter(|s| needs_image_load(s) || is_url_icon(s))
                    .and_then(|s| texture_cache.get(s))
            });

        if has_thumb || icon_is_image {
            if let Some(texture) = thumb_texture {
                let thumb_area = row_height - 12.0; // 6 px padding top + bottom
                let (tw, th) = if let Some((w, h, _)) = &item.thumbnail_rgba {
                    let aspect = *w as f32 / (*h as f32).max(0.001);
                    if aspect > 1.0 {
                        (thumb_area, thumb_area / aspect)
                    } else {
                        (thumb_area * aspect, thumb_area)
                    }
                } else {
                    (thumb_area, thumb_area)
                };
                let center = Pos2::new(rect.min.x + 6.0 + thumb_area / 2.0, rect.center().y);
                let thumb_rect = Rect::from_center_size(center, Vec2::new(tw, th));
                ui.painter().image(
                    texture.id(),
                    thumb_rect,
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
        } else if let Some(icon) = &item.icon {
            // Only render as a glyph if it looks like an emoji/symbol (≤ 2 chars).
            // Longer strings are system icon theme names and cannot be painted as text.
            if icon.chars().count() <= 2 {
                let icon_center = Pos2::new(rect.min.x + icon_col_width / 2.0, rect.center().y);
                ui.painter().text(
                    icon_center,
                    Align2::CENTER_CENTER,
                    icon,
                    FontId::proportional(20.0),
                    text_color,
                );
            }
        }

        // Title and subtitle
        let text_x = rect.min.x + icon_col_width;

        let title_pos = Pos2::new(text_x, rect.min.y + 8.0);
        ui.painter().text(
            title_pos,
            Align2::LEFT_TOP,
            &item.title,
            FontId::monospace(14.0),
            text_color,
        );

        if let Some(subtitle) = &item.subtitle {
            let subtitle_pos = Pos2::new(text_x, rect.center().y);
            ui.painter().text(
                subtitle_pos,
                Align2::LEFT_TOP,
                subtitle,
                FontId::monospace(12.0),
                Color32::GRAY,
            );
        }

        // Accessories: right-aligned chips rendered from right edge inward.
        if !item.accessories.is_empty() {
            let acc_color = ui.visuals().weak_text_color();
            let acc_font = FontId::monospace(11.0);
            let right_edge = rect.max.x - 8.0;
            let mut cursor_x = right_edge;
            for label in item.accessories.iter().rev() {
                let text_shape =
                    ui.fonts(|f| f.layout_no_wrap(label.clone(), acc_font.clone(), acc_color));
                cursor_x -= text_shape.size().x;
                ui.painter().galley(
                    Pos2::new(cursor_x, rect.center().y - text_shape.size().y / 2.0),
                    text_shape,
                    acc_color,
                );
                cursor_x -= 8.0; // gap between chips
            }
        }

        response
    }
}

impl Default for List {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod list_tests {
    use super::*;
    use crate::extension_trait::ExtensionItem;

    fn make_item(title: &str) -> ExtensionItem {
        ExtensionItem {
            title: title.to_string(),
            subtitle: None,
            icon: None,
            action: format!("action-for-{title}"),
            id: None,
            detail: None,
            accessories: vec![],
            extra_actions: vec![],
            detail_metadata: vec![],
            thumbnail_rgba: None,
            grid_columns: None,
        }
    }

    fn items(n: usize) -> Vec<ExtensionItem> {
        (0..n).map(|i| make_item(&format!("item {i}"))).collect()
    }

    #[test]
    fn new_list_does_not_request_scroll() {
        let list = List::new();
        assert!(
            !list.scroll_to_selected,
            "fresh List must not request an automatic scroll"
        );
    }

    #[test]
    fn set_items_requests_scroll_to_show_new_selection() {
        let mut list = List::new();
        list.scroll_to_selected = false;
        list.set_items(items(5));
        assert!(
            list.scroll_to_selected,
            "set_items should mark the new selection for scrolling"
        );
    }

    #[test]
    fn select_next_requests_scroll() {
        let mut list = List::new();
        list.set_items(items(5));
        list.scroll_to_selected = false; // simulate: flag was consumed by a previous render
        list.select_next();
        assert!(
            list.scroll_to_selected,
            "keyboard forward-navigation must request scroll"
        );
    }

    #[test]
    fn select_prev_requests_scroll() {
        let mut list = List::new();
        list.set_items(items(5));
        list.select_next(); // move off index 0
        list.scroll_to_selected = false; // simulate consumed
        list.select_prev();
        assert!(
            list.scroll_to_selected,
            "keyboard back-navigation must request scroll"
        );
    }

    #[test]
    fn scroll_flag_is_false_when_selection_unchanged() {
        // After items load and flag is consumed, no further scroll is requested
        // unless selection changes via keyboard.
        let mut list = List::new();
        list.set_items(items(5));
        list.scroll_to_selected = false; // consumed by render
        // No select_next / select_prev called — user is just mouse-scrolling.
        assert!(
            !list.scroll_to_selected,
            "mouse scrolling must not be overridden by auto-scroll"
        );
    }

    // ── click-to-activate tests ───────────────────────────────────────────────

    #[test]
    fn no_activated_action_on_fresh_list() {
        let mut list = List::new();
        assert!(list.take_activated_action().is_none());
    }

    #[test]
    fn take_activated_action_returns_pending_action() {
        let mut list = List::new();
        // Simulate what ui() does when an item is clicked.
        list.pending_action = Some("open-url:https://example.com".to_string());
        assert_eq!(
            list.take_activated_action(),
            Some("open-url:https://example.com".to_string()),
        );
    }

    #[test]
    fn take_activated_action_clears_after_consume() {
        let mut list = List::new();
        list.pending_action = Some("some-action".to_string());
        let _ = list.take_activated_action();
        assert!(
            list.take_activated_action().is_none(),
            "action must be consumed — a second take should return None"
        );
    }

    #[test]
    fn clicking_item_with_empty_action_still_stored() {
        // The List stores whatever action string the item has; empty-action
        // filtering is the caller's responsibility (mirrors Enter behaviour).
        let mut list = List::new();
        list.pending_action = Some(String::new());
        assert_eq!(list.take_activated_action(), Some(String::new()));
    }

    // ── section header tests ──────────────────────────────────────────────────

    fn make_section(title: &str) -> ExtensionItem {
        ExtensionItem {
            title: title.to_string(),
            subtitle: None,
            icon: None,
            action: "::section::".to_string(),
            id: Some(format!("::section::{title}")),
            detail: None,
            accessories: vec![],
            extra_actions: vec![],
            detail_metadata: vec![],
            thumbnail_rgba: None,
            grid_columns: None,
        }
    }

    #[test]
    fn set_items_skips_section_header_for_initial_selection() {
        let mut list = List::new();
        list.set_items(vec![make_section("Group A"), make_item("item 0")]);
        assert_eq!(
            list.selected_index,
            Some(1),
            "first selectable item (index 1) should be selected, not the section header"
        );
    }

    #[test]
    fn select_next_skips_section_header() {
        let mut list = List::new();
        // item0, header, item1
        list.set_items(vec![
            make_item("item 0"),
            make_section("Group"),
            make_item("item 1"),
        ]);
        assert_eq!(list.selected_index, Some(0));
        list.select_next();
        assert_eq!(
            list.selected_index,
            Some(2),
            "select_next must skip the section header at index 1"
        );
    }

    #[test]
    fn select_prev_skips_section_header() {
        let mut list = List::new();
        // item0, header, item1
        list.set_items(vec![
            make_item("item 0"),
            make_section("Group"),
            make_item("item 1"),
        ]);
        // Start at item 1 (index 2)
        list.selected_index = Some(2);
        list.select_prev();
        assert_eq!(
            list.selected_index,
            Some(0),
            "select_prev must skip the section header at index 1"
        );
    }

    #[test]
    fn select_next_at_end_stays_put() {
        let mut list = List::new();
        list.set_items(vec![make_item("item 0"), make_item("item 1")]);
        list.selected_index = Some(1);
        list.select_next();
        assert_eq!(
            list.selected_index,
            Some(1),
            "should not wrap past the last item"
        );
    }

    #[test]
    fn all_section_headers_yields_no_selection() {
        let mut list = List::new();
        list.set_items(vec![make_section("A"), make_section("B")]);
        assert_eq!(
            list.selected_index, None,
            "a list with only section headers has no selectable item"
        );
    }

    // ── empty view tests ──────────────────────────────────────────────────────

    fn make_empty_view(title: &str) -> ExtensionItem {
        ExtensionItem {
            title: title.to_string(),
            subtitle: Some("Try a different query".to_string()),
            icon: None,
            action: "::empty::".to_string(),
            id: Some("::empty::".to_string()),
            detail: None,
            accessories: vec![],
            extra_actions: vec![],
            detail_metadata: vec![],
            thumbnail_rgba: None,
            grid_columns: None,
        }
    }

    #[test]
    fn empty_view_detected_when_only_item() {
        let items = vec![make_empty_view("No Results")];
        assert!(
            List::empty_view(&items).is_some(),
            "single empty-view sentinel should be detected"
        );
    }

    #[test]
    fn empty_view_not_detected_when_real_items_present() {
        let items = vec![make_empty_view("No Results"), make_item("real item")];
        assert!(
            List::empty_view(&items).is_none(),
            "real items alongside empty view should suppress the empty view"
        );
    }

    #[test]
    fn empty_view_not_detected_for_normal_list() {
        let items = vec![make_item("a"), make_item("b")];
        assert!(List::empty_view(&items).is_none());
    }

    #[test]
    fn empty_view_detected_with_section_headers_alongside() {
        // A section header + an empty view sentinel = the empty view sentinel is
        // the only *real* item → empty view should be detected.
        let items = vec![make_section("Group"), make_empty_view("Nothing here")];
        assert!(
            List::empty_view(&items).is_some(),
            "empty view alongside a section header should still be detected"
        );
    }

    // ── grid_columns_for_items tests ─────────────────────────────────────────

    fn make_grid_item(title: &str, cols: u8) -> ExtensionItem {
        ExtensionItem {
            grid_columns: Some(cols),
            ..make_item(title)
        }
    }

    #[test]
    fn grid_columns_none_for_regular_list_items() {
        let items = vec![make_item("a"), make_item("b")];
        assert_eq!(
            grid_columns_for_items(&items),
            None,
            "regular list items have no grid_columns"
        );
    }

    #[test]
    fn grid_columns_returns_value_from_first_real_item() {
        let items = vec![make_grid_item("a", 4), make_grid_item("b", 4)];
        assert_eq!(grid_columns_for_items(&items), Some(4));
    }

    #[test]
    fn grid_columns_skips_section_headers() {
        let items = vec![make_section("Sec"), make_grid_item("a", 3)];
        assert_eq!(
            grid_columns_for_items(&items),
            Some(3),
            "section header must be skipped; grid item following it carries the columns"
        );
    }

    #[test]
    fn grid_columns_zero_treated_as_none() {
        // A malformed item with grid_columns = 0 should not activate grid mode.
        let mut item = make_item("x");
        item.grid_columns = Some(0);
        assert_eq!(grid_columns_for_items(&[item]), None);
    }

    #[test]
    fn grid_columns_empty_slice_is_none() {
        assert_eq!(grid_columns_for_items(&[]), None);
    }

    // ── snapshot / restore_snapshot tests ────────────────────────────────────

    #[test]
    fn snapshot_captures_items_and_selection() {
        let mut list = List::new();
        list.set_items(items(3));
        list.selected_index = Some(2);
        let (snapped_items, snapped_idx) = list.snapshot();
        assert_eq!(snapped_items.len(), 3);
        assert_eq!(snapped_idx, Some(2));
    }

    #[test]
    fn restore_snapshot_sets_items_and_selection() {
        let mut list = List::new();
        list.set_items(items(5));
        list.selected_index = Some(4);
        let (saved_items, saved_idx) = list.snapshot();

        // Push a new view (clear list)
        list.set_items(vec![]);

        // Pop back: restore
        list.restore_snapshot(saved_items, saved_idx);
        assert_eq!(list.items.len(), 5);
        assert_eq!(list.selected_index, Some(4));
        assert!(list.scroll_to_selected);
    }

    #[test]
    fn restore_snapshot_does_not_reset_selection_to_first() {
        let mut list = List::new();
        let saved = items(4);
        list.restore_snapshot(saved, Some(3));
        // restore_snapshot must NOT reset to the first item the way set_items does
        assert_eq!(list.selected_index, Some(3));
    }

    #[test]
    fn snapshot_on_empty_list_returns_empty_vec_and_none() {
        let list = List::new();
        let (snapped_items, snapped_idx) = list.snapshot();
        assert!(snapped_items.is_empty());
        assert_eq!(snapped_idx, None);
    }
}

#[cfg(test)]
mod bounded_cache_tests {
    use super::BoundedCache;

    #[test]
    fn evicts_oldest_entry_when_at_capacity() {
        let mut cache: BoundedCache<String, u32> = BoundedCache::with_capacity(3);
        cache.insert("a".to_string(), 1);
        cache.insert("b".to_string(), 2);
        cache.insert("c".to_string(), 3);
        // At capacity; inserting "d" should evict "a" (oldest).
        cache.insert("d".to_string(), 4);
        assert!(
            !cache.contains_key(&"a".to_string()),
            "oldest entry must be evicted"
        );
        assert!(cache.contains_key(&"b".to_string()));
        assert!(cache.contains_key(&"c".to_string()));
        assert_eq!(cache.get(&"d".to_string()), Some(&4));
    }

    #[test]
    fn size_never_exceeds_capacity() {
        let mut cache: BoundedCache<i32, i32> = BoundedCache::with_capacity(5);
        for i in 0..100 {
            cache.insert(i, i * 2);
            assert!(cache.map.len() <= 5, "cache grew beyond capacity at i={i}");
        }
    }

    #[test]
    fn updating_existing_key_does_not_increase_length() {
        let mut cache: BoundedCache<String, u32> = BoundedCache::with_capacity(2);
        cache.insert("a".to_string(), 1);
        cache.insert("b".to_string(), 2);
        // Update "a" — must not evict anything.
        cache.insert("a".to_string(), 99);
        assert_eq!(cache.map.len(), 2);
        assert_eq!(cache.get(&"a".to_string()), Some(&99));
        assert!(cache.contains_key(&"b".to_string()));
    }

    #[test]
    fn get_returns_none_for_missing_key() {
        let cache: BoundedCache<String, u32> = BoundedCache::with_capacity(4);
        assert_eq!(cache.get(&"missing".to_string()), None);
    }
}

#[cfg(test)]
mod action_panel_tests {
    use super::*;

    fn make_action(title: &str) -> Action {
        Action {
            title: title.to_string(),
            shortcut: None,
            icon: None,
            action_str: String::new(),
            handler: Box::new(|| {}),
        }
    }

    #[test]
    fn new_panel_is_closed_with_no_selection() {
        let panel = ActionPanel::new();
        assert!(!panel.is_open);
        assert!(panel.selected_index.is_none());
        assert!(panel.actions.is_empty());
    }

    #[test]
    fn set_actions_empty_has_no_selection() {
        let mut panel = ActionPanel::new();
        panel.set_actions(vec![]);
        assert!(panel.selected_index.is_none());
    }

    #[test]
    fn set_actions_nonempty_selects_first() {
        let mut panel = ActionPanel::new();
        panel.set_actions(vec![make_action("A"), make_action("B")]);
        assert_eq!(panel.selected_index, Some(0));
    }

    #[test]
    fn open_sets_open_and_selects_first() {
        let mut panel = ActionPanel::new();
        panel.set_actions(vec![make_action("A"), make_action("B")]);
        panel.open();
        assert!(panel.is_open);
        assert_eq!(panel.selected_index, Some(0));
    }

    #[test]
    fn open_on_empty_panel_has_no_selection() {
        let mut panel = ActionPanel::new();
        panel.open();
        assert!(panel.is_open);
        assert!(panel.selected_index.is_none());
    }

    #[test]
    fn close_clears_open_and_selection() {
        let mut panel = ActionPanel::new();
        panel.set_actions(vec![make_action("A")]);
        panel.open();
        panel.close();
        assert!(!panel.is_open);
        assert!(panel.selected_index.is_none());
    }

    #[test]
    fn select_next_advances_index() {
        let mut panel = ActionPanel::new();
        panel.set_actions(vec![make_action("A"), make_action("B"), make_action("C")]);
        panel.select_next();
        assert_eq!(panel.selected_index, Some(1));
    }

    #[test]
    fn select_next_clamps_at_last() {
        let mut panel = ActionPanel::new();
        panel.set_actions(vec![make_action("A"), make_action("B")]);
        panel.select_next(); // 0 → 1
        panel.select_next(); // 1 → 1 (clamped)
        assert_eq!(panel.selected_index, Some(1));
    }

    #[test]
    fn select_prev_retreats_index() {
        let mut panel = ActionPanel::new();
        panel.set_actions(vec![make_action("A"), make_action("B"), make_action("C")]);
        panel.select_next(); // index 1
        panel.select_prev();
        assert_eq!(panel.selected_index, Some(0));
    }

    #[test]
    fn select_prev_clamps_at_zero() {
        let mut panel = ActionPanel::new();
        panel.set_actions(vec![make_action("A"), make_action("B")]);
        panel.select_prev(); // already at 0
        assert_eq!(panel.selected_index, Some(0));
    }

    #[test]
    fn execute_selected_calls_handler() {
        use std::sync::{Arc, Mutex};
        let called = Arc::new(Mutex::new(false));
        let called_clone = called.clone();
        let mut panel = ActionPanel::new();
        panel.actions = vec![Action {
            title: "action".to_string(),
            shortcut: None,
            icon: None,
            action_str: String::new(),
            handler: Box::new(move || {
                *called_clone.lock().unwrap() = true;
            }),
        }];
        panel.selected_index = Some(0);
        panel.execute_selected();
        assert!(
            *called.lock().unwrap(),
            "handler must be called when action is selected"
        );
    }

    #[test]
    fn execute_selected_on_empty_panel_does_nothing() {
        let mut panel = ActionPanel::new();
        // Must not panic
        panel.execute_selected();
    }

    #[test]
    fn execute_selected_stores_action_str_in_pending() {
        let mut panel = ActionPanel::new();
        panel.set_actions(vec![Action {
            title: "Go".to_string(),
            shortcut: None,
            icon: None,
            action_str: "open-url:https://example.com".to_string(),
            handler: Box::new(|| {}),
        }]);
        panel.execute_selected();
        assert_eq!(
            panel.take_pending_action().as_deref(),
            Some("open-url:https://example.com")
        );
    }

    #[test]
    fn take_pending_action_clears_after_first_call() {
        let mut panel = ActionPanel::new();
        panel.set_actions(vec![make_action("A")]);
        // action_str is empty for make_action, so pending stays None
        panel.execute_selected();
        assert!(panel.take_pending_action().is_none());
    }

    #[test]
    fn empty_action_str_does_not_set_pending() {
        let mut panel = ActionPanel::new();
        panel.set_actions(vec![Action {
            title: "noop".to_string(),
            shortcut: None,
            icon: None,
            action_str: String::new(),
            handler: Box::new(|| {}),
        }]);
        panel.execute_selected();
        assert!(panel.take_pending_action().is_none());
    }
}

// ── Toast notifications ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ToastKind {
    Info,
    Success,
    Error,
}

struct Toast {
    message: String,
    kind: ToastKind,
    created_at: Instant,
    duration: Duration,
}

impl Toast {
    fn alpha(&self) -> f32 {
        let elapsed = self.created_at.elapsed().as_secs_f32();
        let total = self.duration.as_secs_f32();
        // Fade in over 0.15 s, stay opaque, fade out over the last 0.4 s.
        let fade_in = (elapsed / 0.15).min(1.0);
        let fade_out = ((total - elapsed) / 0.4).clamp(0.0, 1.0);
        fade_in * fade_out
    }

    fn is_expired(&self) -> bool {
        self.created_at.elapsed() >= self.duration
    }
}

pub struct ToastManager {
    toasts: Vec<Toast>,
}

impl Default for ToastManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ToastManager {
    pub fn new() -> Self {
        Self { toasts: Vec::new() }
    }

    pub fn push(&mut self, message: impl Into<String>, kind: ToastKind) {
        self.push_with_duration(message, kind, Duration::from_secs(3));
    }

    pub fn push_with_duration(
        &mut self,
        message: impl Into<String>,
        kind: ToastKind,
        duration: Duration,
    ) {
        self.toasts.push(Toast {
            message: message.into(),
            kind,
            created_at: Instant::now(),
            duration,
        });
    }

    /// Remove expired toasts. Returns true if any live toasts remain (caller should request_repaint).
    pub fn retain_live(&mut self) -> bool {
        self.toasts.retain(|t| !t.is_expired());
        !self.toasts.is_empty()
    }

    /// Render all active toasts overlaid at the bottom-centre of the screen.
    pub fn ui(&mut self, ctx: &egui::Context) {
        self.toasts.retain(|t| !t.is_expired());

        if self.toasts.is_empty() {
            return;
        }

        // Request continuous repaints while toasts are animating.
        ctx.request_repaint();

        let screen = ctx.screen_rect();
        let mut y_offset = screen.max.y - 16.0;

        for toast in self.toasts.iter().rev() {
            let alpha = toast.alpha();
            if alpha <= 0.0 {
                continue;
            }

            let bg_color = match toast.kind {
                ToastKind::Info => {
                    Color32::from_rgba_unmultiplied(50, 50, 80, (230.0 * alpha) as u8)
                }
                ToastKind::Success => {
                    Color32::from_rgba_unmultiplied(30, 100, 50, (230.0 * alpha) as u8)
                }
                ToastKind::Error => {
                    Color32::from_rgba_unmultiplied(120, 30, 30, (230.0 * alpha) as u8)
                }
            };
            let text_color = Color32::from_rgba_unmultiplied(230, 230, 230, (255.0 * alpha) as u8);

            // Measure text width so we can centre the toast.
            let font_id = FontId::monospace(13.0);
            let galley =
                ctx.fonts(|f| f.layout_no_wrap(toast.message.clone(), font_id.clone(), text_color));
            let padding = Vec2::new(16.0, 8.0);
            let toast_w = galley.size().x + padding.x * 2.0;
            let toast_h = galley.size().y + padding.y * 2.0;

            let x = screen.center().x - toast_w / 2.0;
            y_offset -= toast_h;
            let rect = Rect::from_min_size(Pos2::new(x, y_offset), Vec2::new(toast_w, toast_h));
            y_offset -= 6.0; // gap between stacked toasts

            egui::Area::new(egui::Id::new(format!("toast_{:p}", &toast.message)))
                .fixed_pos(rect.min)
                .interactable(false)
                .order(Order::Tooltip)
                .show(ctx, |ui| {
                    let painter = ui.painter();
                    painter.rect_filled(rect, 8.0, bg_color);
                    painter.galley(rect.min + padding, galley, text_color);
                });
        }
    }
}

/// Side panel that shows the `detail` field of the currently selected item.
pub struct Detail;

impl Detail {
    /// Render the detail panel with optional structured metadata rows.
    pub fn ui(ui: &mut Ui, title: &str, content: &str, metadata: &[DetailMetadataRow]) {
        let frame = egui::Frame::none()
            .fill(ui.visuals().faint_bg_color)
            .stroke(egui::Stroke::new(1.0, egui::Color32::from_gray(50)))
            .rounding(6.0)
            .inner_margin(egui::Margin::same(12.0));

        frame.show(ui, |ui| {
            ui.set_min_width(ui.available_width());

            ui.label(
                egui::RichText::new(title)
                    .font(egui::FontId::monospace(14.0))
                    .color(ui.visuals().text_color()),
            );
            ui.add_space(6.0);
            ui.separator();
            ui.add_space(6.0);

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if !content.is_empty() {
                        ui.label(
                            egui::RichText::new(content)
                                .font(egui::FontId::monospace(12.0))
                                .color(egui::Color32::LIGHT_GRAY),
                        );
                    }
                    if !metadata.is_empty() {
                        if !content.is_empty() {
                            ui.add_space(8.0);
                            ui.separator();
                            ui.add_space(4.0);
                        }
                        Self::render_metadata(ui, metadata);
                    }
                });
        });
    }

    fn render_metadata(ui: &mut Ui, metadata: &[DetailMetadataRow]) {
        let dim = ui.visuals().weak_text_color();
        let bright = ui.visuals().text_color();
        for row in metadata {
            match row {
                DetailMetadataRow::Separator => {
                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(4.0);
                }
                DetailMetadataRow::Label { title, text, .. } => {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(title).color(dim).small());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if let Some(t) = text {
                                ui.label(egui::RichText::new(t).color(bright).small());
                            }
                        });
                    });
                }
                DetailMetadataRow::Link {
                    title,
                    text,
                    target,
                } => {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(title).color(dim).small());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.hyperlink_to(egui::RichText::new(text).small(), target);
                        });
                    });
                }
                DetailMetadataRow::TagList { title, tags } => {
                    ui.label(egui::RichText::new(title).color(dim).small());
                    ui.horizontal_wrapped(|ui| {
                        for tag in tags {
                            Self::render_tag(ui, tag);
                        }
                    });
                    ui.add_space(2.0);
                }
            }
        }
    }

    fn render_tag(ui: &mut Ui, tag: &TagItem) {
        let color = tag
            .color
            .as_deref()
            .and_then(parse_hex_color)
            .unwrap_or(egui::Color32::from_rgb(60, 100, 160));
        let frame = egui::Frame::none()
            .fill(color)
            .rounding(4.0)
            .inner_margin(egui::Margin::symmetric(6.0, 2.0));
        frame.show(ui, |ui| {
            ui.label(
                egui::RichText::new(&tag.text)
                    .small()
                    .color(egui::Color32::WHITE),
            );
        });
    }
}

/// Returns `true` if this icon string is an HTTP/HTTPS URL that needs async fetching.
pub fn is_url_icon(icon: &str) -> bool {
    icon.starts_with("http://") || icon.starts_with("https://")
}

/// Compute a short hex cache key for a URL (FNV-1a 64-bit hash, 16 hex chars).
pub fn url_cache_key(url: &str) -> String {
    let mut hash: u64 = 14_695_981_039_346_656_037;
    for byte in url.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(1_099_511_628_211);
    }
    format!("{hash:016x}")
}

/// Returns the on-disk cache path for a URL icon:
/// `~/.pterry/icon_cache/<fnv-hex-of-url>`.
fn icon_cache_path(url: &str) -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".pterry")
        .join("icon_cache")
        .join(url_cache_key(url))
}

/// Fetch a URL icon, consulting and populating the on-disk cache.
///
/// Returns `Some(raw_image_bytes)` on success, `None` on any error.
/// The returned bytes are in the image's native format (PNG/JPEG/…); pass
/// to `decode_image_bytes()` to get RGBA pixels.
pub fn fetch_url_icon(url: &str) -> Option<Vec<u8>> {
    let cache_path = icon_cache_path(url);

    // Cache hit — return without a network request.
    if let Ok(bytes) = std::fs::read(&cache_path)
        && !bytes.is_empty()
    {
        return Some(bytes);
    }

    // Network fetch via ureq (synchronous, intended for background threads).
    let response = ureq::get(url).call().ok()?;
    let mut bytes = Vec::new();
    response.into_reader().read_to_end(&mut bytes).ok()?;

    // Write to cache (best-effort; ignore errors).
    if let Some(parent) = cache_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&cache_path, &bytes);

    Some(bytes)
}

/// Decode an icon string that requires image loading into raw RGBA bytes.
///
/// Returns `Some((width, height, rgba_bytes))` for:
/// - `data:image/<type>;base64,<data>` — Base64-encoded inline image
/// - `file://<path>` — local file path (strips the `file://` prefix)
/// - Absolute paths starting with `/` — read directly from disk
///
/// Returns `None` for emoji/symbol strings (≤ 2 chars), strings without a
/// recognised prefix, or any decode/IO error (so the caller can fall back
/// to a glyph or no icon).
pub fn resolve_icon_data(icon: &str) -> Option<(usize, usize, Vec<u8>)> {
    use base64::Engine as _;

    if let Some(rest) = icon.strip_prefix("data:image/") {
        // data:image/<mime>;base64,<data>
        let b64 = rest.split(';').nth(1)?.strip_prefix("base64,")?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64.trim())
            .ok()?;
        return decode_image_bytes(&bytes);
    }

    let path = if let Some(p) = icon.strip_prefix("file://") {
        p
    } else if icon.starts_with('/') {
        icon
    } else {
        return None;
    };

    let bytes = std::fs::read(path).ok()?;
    decode_image_bytes(&bytes)
}

fn decode_image_bytes(bytes: &[u8]) -> Option<(usize, usize, Vec<u8>)> {
    let img = image::load_from_memory(bytes).ok()?;
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    Some((w as usize, h as usize, rgba.into_raw()))
}

/// Returns `true` if this icon string needs image loading (as opposed to
/// being rendered as a glyph or left blank).
pub fn needs_image_load(icon: &str) -> bool {
    icon.starts_with("data:image/") || icon.starts_with("file://") || icon.starts_with('/')
}

fn parse_hex_color(s: &str) -> Option<egui::Color32> {
    let s = s.strip_prefix('#').unwrap_or(s);
    if s.len() == 6 {
        let r = u8::from_str_radix(&s[0..2], 16).ok()?;
        let g = u8::from_str_radix(&s[2..4], 16).ok()?;
        let b = u8::from_str_radix(&s[4..6], 16).ok()?;
        Some(egui::Color32::from_rgb(r, g, b))
    } else {
        None
    }
}

#[cfg(test)]
mod url_icon_tests {
    use super::{fetch_url_icon, icon_cache_path, is_url_icon, url_cache_key};

    #[test]
    fn is_url_icon_true_for_http() {
        assert!(is_url_icon("http://example.com/icon.png"));
        assert!(is_url_icon("https://example.com/icon.png"));
    }

    #[test]
    fn is_url_icon_false_for_non_url() {
        assert!(!is_url_icon("data:image/png;base64,abc"));
        assert!(!is_url_icon("/usr/share/icons/foo.png"));
        assert!(!is_url_icon("file:///usr/share/icons/foo.png"));
        assert!(!is_url_icon("🚀"));
    }

    #[test]
    fn url_cache_key_is_16_hex_chars() {
        let key = url_cache_key("https://example.com/icon.png");
        assert_eq!(key.len(), 16);
        assert!(key.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn url_cache_key_differs_per_url() {
        let k1 = url_cache_key("https://a.com/a.png");
        let k2 = url_cache_key("https://b.com/b.png");
        assert_ne!(k1, k2);
    }

    #[test]
    fn fetch_url_icon_hits_disk_cache_when_present() {
        // Write fake image bytes to the cache path; fetch_url_icon should return them
        // without making a network request.
        use base64::Engine as _;
        let png = {
            let img = image::RgbaImage::from_pixel(1, 1, image::Rgba([0, 255, 0, 255]));
            let mut buf = Vec::new();
            img.write_to(
                &mut std::io::Cursor::new(&mut buf),
                image::ImageFormat::Png,
            )
            .unwrap();
            buf
        };
        // Use a fake URL that won't be reached over the network.
        let url = "https://test.invalid/cached_icon.png";
        let cache = icon_cache_path(url);
        if let Some(parent) = cache.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&cache, &png).unwrap();

        let result = fetch_url_icon(url);
        // Clean up before asserting to avoid test pollution.
        let _ = std::fs::remove_file(&cache);

        assert!(result.is_some(), "should return cached bytes");
        assert_eq!(result.unwrap(), png);
        let _ = base64::engine::general_purpose::STANDARD.encode("");  // keep import used
    }

    #[test]
    fn fetch_url_icon_returns_none_for_unreachable_url() {
        // A truly unreachable URL (no DNS, no cache) must return None gracefully.
        let result = fetch_url_icon("https://this.host.does.not.exist.invalid/icon.png");
        assert!(result.is_none());
    }
}

#[cfg(test)]
mod icon_resolution_tests {
    use super::{needs_image_load, resolve_icon_data};
    use base64::Engine as _;

    /// Build a minimal 1×1 RGBA PNG in memory using the `image` crate.
    fn make_png_bytes() -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(1, 1, image::Rgba([255, 0, 0, 255]));
        let mut buf = Vec::new();
        img.write_to(
            &mut std::io::Cursor::new(&mut buf),
            image::ImageFormat::Png,
        )
        .expect("encode 1×1 PNG");
        buf
    }

    #[test]
    fn data_uri_png_decodes_to_rgba() {
        let png = make_png_bytes();
        let b64 = base64::engine::general_purpose::STANDARD.encode(&png);
        let data_uri = format!("data:image/png;base64,{b64}");
        let result = resolve_icon_data(&data_uri);
        assert!(result.is_some(), "Base64 PNG data URI must decode successfully");
        let (w, h, rgba) = result.unwrap();
        assert_eq!((w, h), (1, 1));
        assert_eq!(rgba.len(), 4, "1×1 RGBA = 4 bytes");
    }

    #[test]
    fn absolute_path_png_loads_correctly() {
        let png = make_png_bytes();
        let tmp = tempfile::NamedTempFile::new().expect("tempfile");
        std::fs::write(tmp.path(), &png).expect("write png");
        let path = tmp.path().to_string_lossy().to_string();
        assert!(path.starts_with('/'), "temp path must be absolute");
        let result = resolve_icon_data(&path);
        assert!(result.is_some(), "absolute path PNG must resolve");
        let (w, h, _) = result.unwrap();
        assert_eq!((w, h), (1, 1));
    }

    #[test]
    fn file_uri_strips_prefix_and_loads() {
        let png = make_png_bytes();
        let tmp = tempfile::NamedTempFile::new().expect("tempfile");
        std::fs::write(tmp.path(), &png).expect("write png");
        let uri = format!("file://{}", tmp.path().display());
        let result = resolve_icon_data(&uri);
        assert!(result.is_some(), "file:// URI must resolve");
        let (w, h, _) = result.unwrap();
        assert_eq!((w, h), (1, 1));
    }

    #[test]
    fn emoji_returns_none() {
        assert!(resolve_icon_data("🚀").is_none());
        assert!(resolve_icon_data("🌍").is_none());
    }

    #[test]
    fn http_url_returns_none() {
        // HTTP fetching is not yet implemented; must return None gracefully.
        assert!(resolve_icon_data("https://example.com/icon.png").is_none());
    }

    #[test]
    fn needs_image_load_true_for_data_uri() {
        assert!(needs_image_load("data:image/png;base64,abc"));
    }

    #[test]
    fn needs_image_load_true_for_file_uri() {
        assert!(needs_image_load("file:///usr/share/icons/foo.png"));
    }

    #[test]
    fn needs_image_load_true_for_absolute_path() {
        assert!(needs_image_load("/usr/share/icons/foo.png"));
    }

    #[test]
    fn needs_image_load_false_for_emoji() {
        assert!(!needs_image_load("🚀"));
        assert!(!needs_image_load("🌍"));
    }

    #[test]
    fn needs_image_load_false_for_http() {
        assert!(!needs_image_load("https://example.com/icon.png"));
    }
}
