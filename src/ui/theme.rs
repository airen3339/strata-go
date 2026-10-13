// SPDX-License-Identifier: MIT

use std::{
    cell::{Cell, RefCell},
    collections::BTreeSet,
    fs, io,
    path::{Path, PathBuf},
    rc::{Rc, Weak},
    time::Duration,
};

use gtk::{gdk, gio, glib, prelude::*};
use serde::{Deserialize, Serialize};
use sourceview5::prelude::BufferExt as _;

use super::preferences::{
    FileIconColors, OmarchyVariant, PreferenceManager, TextSize, desktop_text_scale_factor,
    notify_live, snapped_root_font_px,
};

mod omarchy;

thread_local! {
    static SHARED_MANAGER: RefCell<Option<Rc<ThemeManager>>> = const { RefCell::new(None) };
    static SOURCE_STYLE_PATH_INSTALLED: Cell<bool> = const { Cell::new(false) };
    static SOURCE_BUFFERS: RefCell<Vec<glib::WeakRef<sourceview5::Buffer>>> = const { RefCell::new(Vec::new()) };
    static DOCUMENT_BUFFERS: RefCell<Vec<glib::WeakRef<gtk::TextBuffer>>> = const { RefCell::new(Vec::new()) };
    static DOCUMENT_VIEWS: RefCell<Vec<glib::WeakRef<super::document_view::DocumentTextView>>> = const { RefCell::new(Vec::new()) };
    /// Installed on the first source preview buffer, so startup performs no SourceView I/O.
    static PENDING_STYLE_TOKENS: RefCell<Option<(ThemeTokens, Option<SourcePalette>)>> = const { RefCell::new(None) };
    static STYLE_SCHEME_DIRTY: Cell<bool> = const { Cell::new(true) };
}

const THEME_CATALOG: &str = include_str!("../../data/themes/catalog.toml");

/// Qualified ids keep installed Omarchy themes distinct from built-in and custom ones.
pub(crate) const OMARCHY_THEME_ID_PREFIX: &str = "omarchy:";
/// How often a Follow session rechecks for an unresolvable Omarchy theme.
const OMARCHY_RETRY_INTERVAL: Duration = Duration::from_secs(2);
/// Used only when `$OMARCHY_PATH` is unset; matches `omarchy-theme-list`'s default.
const OMARCHY_BUNDLED_DIR_FALLBACK: &str = "/usr/share/omarchy";

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ThemeTokens {
    pub name: String,
    pub background: String,
    pub surface: String,
    pub text: String,
    pub accent: String,
    #[serde(default = "default_danger")]
    pub danger: String,
    pub muted: String,
    pub highlight: String,
    pub border: String,
    pub dim_text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub syntax_keyword: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub syntax_string: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub syntax_constant: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub syntax_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub syntax_preprocessor: Option<String>,
}

#[derive(Clone)]
struct SourcePalette {
    statement: String,
    string: String,
    constant: String,
    type_color: String,
    preprocessor: String,
}

#[derive(Clone, Debug)]
pub struct Theme {
    pub id: String,
    pub tokens: ThemeTokens,
    pub custom: bool,
}

pub(crate) fn is_omarchy_theme_id(id: &str) -> bool {
    id.starts_with(OMARCHY_THEME_ID_PREFIX)
}

#[derive(Deserialize)]
struct ThemeCatalog {
    themes: Vec<CatalogTheme>,
}

#[derive(Deserialize)]
struct CatalogTheme {
    id: String,
    #[serde(flatten)]
    tokens: ThemeTokens,
}

/// Appearance preferences that change the shared CSS or icon rendering the manager applies.
#[derive(Clone, Debug, PartialEq)]
struct AppearancePreferences {
    mode: String,
    theme: String,
    omarchy_variant: OmarchyVariant,
    text_size: TextSize,
    element_glow: bool,
    file_icon_colors: FileIconColors,
}

type ThemeRefreshPreference = dyn Fn(&gtk::Widget, &ThemeManager);

struct ThemePreferenceListener {
    active: Cell<bool>,
    anchor: glib::WeakRef<gtk::Widget>,
    refresh: Box<ThemeRefreshPreference>,
}

#[derive(Default)]
struct ThemeListeners {
    notifying: Cell<bool>,
    listeners: Rc<RefCell<Vec<Rc<ThemePreferenceListener>>>>,
}

impl ThemeListeners {
    fn bind<T: PartialEq + Clone + 'static>(
        &self,
        manager: &ThemeManager,
        anchor: &impl IsA<gtk::Widget>,
        read: impl Fn(&ThemeManager) -> T + 'static,
        apply: impl Fn(&gtk::Widget, T) + 'static,
    ) {
        let previous = RefCell::new(None);
        let listener = Rc::new(ThemePreferenceListener {
            active: Cell::new(true),
            anchor: anchor.as_ref().downgrade(),
            refresh: Box::new(move |widget, manager| {
                let value = read(manager);
                if previous.borrow().as_ref() == Some(&value) {
                    return;
                }
                previous.replace(Some(value.clone()));
                apply(widget, value);
            }),
        });
        self.listeners.borrow_mut().push(listener.clone());
        let weak_listeners = Rc::downgrade(&self.listeners);
        let weak_listener = Rc::downgrade(&listener);
        anchor.connect_destroy(move |_| {
            if let Some(listener) = weak_listener.upgrade() {
                listener.active.set(false);
            }
            if let Some(listeners) = weak_listeners.upgrade() {
                listeners.borrow_mut().retain(|candidate| {
                    !std::rc::Weak::ptr_eq(&Rc::downgrade(candidate), &weak_listener)
                });
            }
        });
        (listener.refresh)(anchor.as_ref(), manager);
    }

    fn notify(&self, manager: &ThemeManager) {
        if self.notifying.replace(true) {
            return;
        }
        notify_live(
            &self.listeners,
            |listener| listener.active.get() && listener.anchor.upgrade().is_some(),
            |listener| {
                if listener.active.get()
                    && let Some(anchor) = listener.anchor.upgrade()
                {
                    (listener.refresh)(&anchor, manager);
                }
            },
        );
        self.notifying.set(false);
    }
}

pub struct ThemeManager {
    provider: gtk::CssProvider,
    themes: RefCell<Vec<Theme>>,
    preferences: Rc<PreferenceManager>,
    self_weak: RefCell<Weak<ThemeManager>>,
    omarchy_available: Cell<bool>,
    /// Follow is on but the active Omarchy theme cannot be resolved; the saved
    /// independent theme shows until the retry succeeds.
    omarchy_degraded: Cell<bool>,
    /// The single Follow retry timer, if one is armed.
    omarchy_retry: RefCell<Option<glib::SourceId>>,
    #[cfg(test)]
    retry_armed_total: Cell<u32>,
    /// Last successfully applied independent palette; the fallback when the saved
    /// theme id has no valid theme, with the stored id kept.
    last_independent_tokens: RefCell<Option<ThemeTokens>>,
    omarchy_monitors: RefCell<Vec<gio::FileMonitor>>,
    pending_omarchy_refresh: RefCell<Option<glib::SourceId>>,
    preview: RefCell<Option<ThemeTokens>>,
    preview_generation: Cell<u64>,
    appearance: RefCell<AppearancePreferences>,
    theme_listeners: ThemeListeners,
    active_model_palette: Cell<crate::services::ModelPalette>,
}

impl ThemeManager {
    pub fn shared() -> Rc<Self> {
        let existing = SHARED_MANAGER.with(|shared| shared.borrow().clone());
        if let Some(manager) = existing {
            return manager;
        }
        let manager = Self::load();
        SHARED_MANAGER.with(|shared| shared.replace(Some(manager.clone())));
        manager
    }

    fn load() -> Rc<Self> {
        let preferences = PreferenceManager::shared();
        let themes = merge_builtin_custom_and_omarchy(
            builtins(),
            load_custom_themes(),
            discover_omarchy_themes(),
        );
        let omarchy_available = load_omarchy_theme(OmarchyVariant::Original).is_some();
        preferences.normalize_loaded_theme_selection(
            // A stored `omarchy:*` id is the independent selection even while its
            // theme is not installed, so it is restored if the theme returns.
            |id| is_omarchy_theme_id(id) || themes.iter().any(|theme| theme.id == id),
            omarchy_available,
        );
        let appearance = AppearancePreferences {
            mode: preferences.theme_mode(),
            theme: preferences.selected_theme_id(),
            omarchy_variant: preferences.omarchy_variant(),
            text_size: preferences.text_size(),
            element_glow: preferences.element_glow(),
            file_icon_colors: preferences.file_icon_colors(),
        };

        let manager = Rc::new(Self {
            provider: gtk::CssProvider::new(),
            themes: RefCell::new(themes),
            preferences: preferences.clone(),
            self_weak: RefCell::new(Weak::new()),
            omarchy_available: Cell::new(omarchy_available),
            omarchy_degraded: Cell::new(false),
            omarchy_retry: RefCell::new(None),
            #[cfg(test)]
            retry_armed_total: Cell::new(0),
            last_independent_tokens: RefCell::new(None),
            omarchy_monitors: RefCell::new(Vec::new()),
            pending_omarchy_refresh: RefCell::new(None),
            preview: RefCell::new(None),
            preview_generation: Cell::new(0),
            appearance: RefCell::new(appearance),
            theme_listeners: ThemeListeners::default(),
            active_model_palette: Cell::new(crate::services::ModelPalette {
                accent: 0,
                surface: 0,
            }),
        });
        manager.self_weak.replace(Rc::downgrade(&manager));
        let weak = Rc::downgrade(&manager);
        preferences.observe(Rc::new(move || {
            if let Some(manager) = weak.upgrade() {
                manager.on_preferences_changed();
            }
        }));
        pin_manual_font_rendering();
        manager.install_provider();
        manager.apply_selected();
        manager.monitor_text_scaling();
        // The active-theme watcher only runs while following; nothing may switch
        // themes with Follow off.
        if manager.follows_omarchy() {
            manager.monitor_omarchy();
        }
        // A restored Follow session without a resolvable theme still retries.
        manager.reconcile_omarchy_retry();
        manager
    }

    pub fn themes(&self) -> Vec<Theme> {
        self.themes.borrow().clone()
    }

    pub fn is_omarchy_available(&self) -> bool {
        self.omarchy_available.get()
    }

    pub fn follows_omarchy(&self) -> bool {
        self.preferences.theme_mode() == "omarchy"
    }

    pub fn selected_id(&self) -> String {
        self.preferences.selected_theme_id()
    }

    pub(crate) fn active_model_palette(&self) -> crate::services::ModelPalette {
        self.active_model_palette.get()
    }

    /// Applies the current value immediately, then only changes to that value.
    /// Capture weak references to owned widgets/state; the anchor owns the binding's lifetime.
    pub(crate) fn bind_theme_preference<T: PartialEq + Clone + 'static>(
        &self,
        anchor: &impl IsA<gtk::Widget>,
        read: impl Fn(&Self) -> T + 'static,
        apply: impl Fn(&gtk::Widget, T) + 'static,
    ) {
        self.theme_listeners.bind(self, anchor, read, apply);
    }

    pub fn select_theme(&self, id: &str) {
        // The library is insensitive while following; a selection must never flip
        // modes or rewrite the preserved independent id.
        if self.follows_omarchy() {
            return;
        }
        if !self.themes.borrow().iter().any(|theme| theme.id == id) {
            return;
        }
        self.preview.take();
        let changed = self.selected_id() != id;
        self.preferences.set_theme_selection("theme", Some(id));
        if !changed {
            // Re-selecting the current theme restores it after a live preview.
            self.apply_selected();
        }
    }

    pub fn set_follow_omarchy(&self, enabled: bool) {
        if enabled {
            // A theme installed or removed while manual changes availability.
            self.update_omarchy_availability();
            if !self.is_omarchy_available() {
                return;
            }
        }
        self.preview.take();
        let changed = self.follows_omarchy() != enabled;
        let mode = if enabled { "omarchy" } else { "theme" };
        self.preferences.set_theme_selection(mode, None);
        if enabled {
            if let Some(manager) = self.self_weak.borrow().upgrade() {
                manager.monitor_omarchy();
            }
        } else {
            self.stop_omarchy_monitoring();
            self.cancel_omarchy_retry();
            if self.omarchy_degraded.replace(false) {
                self.preferences.notify_changes();
            }
        }
        if !changed {
            self.apply_selected();
        }
    }

    /// Applies `tokens` until [`Self::cancel_preview`] or a theme selection; appearance
    /// changes in the meantime re-apply the preview rather than the saved theme.
    /// Returns the preview's generation for [`Self::cancel_preview_from`].
    pub fn preview(&self, tokens: &ThemeTokens) -> Option<u64> {
        validate_tokens(tokens).ok()?;
        self.preview.replace(Some(tokens.clone()));
        self.preview_generation
            .set(self.preview_generation.get().wrapping_add(1));
        self.apply_tokens(tokens, None);
        Some(self.preview_generation.get())
    }

    pub fn cancel_preview(&self) {
        if self.preview.take().is_some() {
            self.apply_selected();
        }
    }

    /// Cancels the preview only if no later one has replaced it.
    pub fn cancel_preview_from(&self, generation: u64) {
        if self.preview_generation.get() == generation {
            self.cancel_preview();
        }
    }

    #[cfg(test)]
    pub(crate) fn is_previewing(&self) -> bool {
        self.preview.borrow().is_some()
    }

    pub fn save_custom_theme(&self, tokens: ThemeTokens) -> io::Result<String> {
        validate_tokens(&tokens)
            .map_err(|message| io::Error::new(io::ErrorKind::InvalidInput, message))?;
        let directory = themes_directory();
        fs::create_dir_all(&directory)?;
        // Never replace a file already there, even one that did not load: it may be
        // a broken or newly linked dotfile the user still wants.
        let id = custom_theme_id(&tokens.name, |candidate| {
            self.themes
                .borrow()
                .iter()
                .any(|theme| theme.id == candidate)
                || fs::symlink_metadata(directory.join(format!("{candidate}.toml"))).is_ok()
        });
        let path = directory.join(format!("{id}.toml"));
        let value = toml::to_string_pretty(&tokens).map_err(io::Error::other)?;
        crate::storage::atomic_write_config(&path, value.as_bytes())?;

        self.themes.borrow_mut().push(Theme {
            id: id.clone(),
            tokens,
            custom: true,
        });
        self.select_theme(&id);
        Ok(id)
    }

    pub fn appearance_tokens(&self) -> ThemeTokens {
        if self.follows_omarchy() {
            if let Some(tokens) = load_omarchy_theme(self.preferences.omarchy_variant()) {
                return tokens;
            }
            // Degraded: mirror the applied fallback, never a stale Omarchy palette.
            if let Some(tokens) = self.current_tokens() {
                return tokens;
            }
            if let Some(tokens) = self.last_independent_tokens.borrow().clone() {
                return tokens;
            }
            return azure_tokens();
        }
        // Mirrors the missing-selection fallback in `apply_selected`.
        self.current_tokens()
            .or_else(|| self.last_independent_tokens.borrow().clone())
            .unwrap_or_else(azure_tokens)
    }

    pub fn starter_tokens(&self) -> ThemeTokens {
        self.current_tokens().unwrap_or_else(azure_tokens)
    }

    fn install_provider(&self) {
        if let Some(display) = gdk::Display::default() {
            gtk::style_context_add_provider_for_display(
                &display,
                &self.provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
            );
        }
    }

    fn apply_selected(&self) {
        let preview = self.preview.borrow().clone();
        if let Some(tokens) = preview {
            self.apply_tokens(&tokens, None);
            return;
        }
        if self.follows_omarchy() {
            if let Some(tokens) = load_omarchy_theme(self.preferences.omarchy_variant()) {
                let was_degraded = self.omarchy_degraded.replace(false);
                self.cancel_omarchy_retry();
                if was_degraded {
                    tracing::info!("Omarchy theme resolved again; resuming Follow Omarchy");
                    self.preferences.notify_changes();
                }
                let palette = load_omarchy_source_palette();
                self.apply_tokens(&tokens, palette.as_ref());
            } else {
                if !self.omarchy_degraded.replace(true) {
                    tracing::warn!(
                        "Omarchy theme unavailable; showing the saved theme until it returns"
                    );
                    self.preferences.notify_changes();
                }
                self.ensure_omarchy_retry();
                // Temporary fallback only: mode and the stored id are preserved.
                self.apply_independent_fallback();
            }
            self.update_omarchy_availability();
            return;
        }
        if let Some(tokens) = self.current_tokens() {
            self.last_independent_tokens.replace(Some(tokens.clone()));
            self.apply_tokens(&tokens, None);
        } else {
            self.apply_independent_fallback();
        }
    }

    /// Applies the saved independent theme without persisting anything, keeping the
    /// stored id so a removed theme is restored if it returns.
    fn apply_independent_fallback(&self) {
        if let Some(tokens) = self.current_tokens() {
            self.last_independent_tokens.replace(Some(tokens.clone()));
            self.apply_tokens(&tokens, None);
            return;
        }
        if self.follows_omarchy() {
            tracing::debug!(
                theme = %self.preferences.selected_theme_id(),
                "saved theme unavailable while following; using a fallback"
            );
        } else {
            tracing::warn!(
                theme = %self.preferences.selected_theme_id(),
                "selected theme unavailable; keeping the saved choice and using a fallback"
            );
        }
        let fallback = self
            .last_independent_tokens
            .borrow()
            .clone()
            .unwrap_or_else(azure_tokens);
        self.apply_tokens(&fallback, None);
    }

    pub(in crate::ui) fn current_tokens(&self) -> Option<ThemeTokens> {
        let id = self.preferences.selected_theme_id();
        self.themes
            .borrow()
            .iter()
            .find(|theme| theme.id == id)
            .map(|theme| theme.tokens.clone())
    }

    fn apply_tokens(&self, tokens: &ThemeTokens, source_palette: Option<&SourcePalette>) {
        let color = |value: &str| {
            let [red, green, blue] = parse_rgb_channels(value).unwrap_or_default();
            (u32::from(red) << 16) | (u32::from(green) << 8) | u32::from(blue)
        };
        self.active_model_palette
            .set(crate::services::ModelPalette {
                accent: color(&tokens.accent),
                surface: color(&tokens.surface),
            });
        super::document_media::apply_theme(tokens);
        super::browser::find::apply_theme(&tokens.accent, &tokens.background);
        super::preview::audio::apply_theme(tokens);
        let root_font_px = snapped_root_font_px(
            self.preferences.text_size().root_font_px(),
            desktop_text_scale_factor(),
        );
        let glow = if self.preferences.element_glow() {
            "@strata_accent"
        } else {
            "transparent"
        };
        self.provider.load_from_string(&format!(
            "{}\n@define-color strata_glow {glow};\n",
            tokens_css(tokens, root_font_px)
        ));
        apply_interface_font(root_font_px);
        crate::assets::set_interface_icon_scale(root_font_px / 13.0);
        let dark_background =
            parse_rgb_channels(&tokens.background).is_none_or(|[red, green, blue]| {
                u32::from(red) * 299 + u32::from(green) * 587 + u32::from(blue) * 114 < 128_000
            });
        crate::assets::set_catppuccin_flavor(crate::assets::CatppuccinFlavor::for_dark_background(
            dark_background,
        ));
        crate::assets::set_catppuccin_theme_colors(
            self.preferences.file_icon_colors() == FileIconColors::Theme,
        );
        // SVG strokes accept fewer color forms than GTK does.
        crate::assets::set_primary_icon_color(&color_to_hex(&tokens.accent));
        crate::assets::set_text_icon_color(&color_to_hex(&tokens.text));
        crate::assets::set_danger_icon_color(&color_to_hex(&tokens.danger));
        super::thumbnail::refresh_all_customized_icons();
        stage_source_style_scheme(tokens, source_palette);
        style_document_buffers(tokens);
        style_document_views(tokens);
        self.theme_listeners.notify(self);
    }

    fn on_preferences_changed(&self) {
        let appearance = AppearancePreferences {
            mode: self.preferences.theme_mode(),
            theme: self.preferences.selected_theme_id(),
            omarchy_variant: self.preferences.omarchy_variant(),
            text_size: self.preferences.text_size(),
            element_glow: self.preferences.element_glow(),
            file_icon_colors: self.preferences.file_icon_colors(),
        };
        if *self.appearance.borrow() != appearance {
            self.appearance.replace(appearance);
            self.apply_selected();
        }
        self.theme_listeners.notify(self);
    }

    fn monitor_text_scaling(self: &Rc<Self>) {
        let Some(settings) = gtk::Settings::default() else {
            return;
        };
        let weak = Rc::downgrade(self);
        settings.connect_gtk_xft_dpi_notify(move |_| {
            let Some(manager) = weak.upgrade() else {
                return;
            };
            manager.apply_selected();
        });
    }

    fn monitor_omarchy(self: &Rc<Self>) {
        self.stop_omarchy_monitoring();
        let state = omarchy_state_dir();
        let home = glib::home_dir();
        // Ancestor watches survive moving away or replacing the current state tree.
        let theme = state.join("theme");
        for path in std::iter::once(theme.as_path())
            .chain(state.ancestors().take_while(|path| path.starts_with(&home)))
        {
            if !path.is_dir() {
                continue;
            }
            let file = gio::File::for_path(path);
            let Ok(monitor) =
                file.monitor_directory(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE)
            else {
                continue;
            };
            let weak = Rc::downgrade(self);
            monitor.connect_changed(move |_, file, other_file, _| {
                if !is_omarchy_theme_event(file)
                    && !other_file
                        .as_ref()
                        .is_some_and(|file| is_omarchy_theme_event(file))
                {
                    return;
                }
                let Some(manager) = weak.upgrade() else {
                    return;
                };
                if let Some(pending) = manager.pending_omarchy_refresh.borrow_mut().take() {
                    pending.remove();
                }
                let weak = weak.clone();
                let refresh = glib::timeout_add_local_once(Duration::from_millis(75), move || {
                    let Some(manager) = weak.upgrade() else {
                        return;
                    };
                    manager.pending_omarchy_refresh.borrow_mut().take();
                    manager.refresh_omarchy_state();
                });
                manager.pending_omarchy_refresh.replace(Some(refresh));
            });
            self.omarchy_monitors.borrow_mut().push(monitor);
        }
    }

    pub(in crate::ui) fn refresh_omarchy_state(self: &Rc<Self>) {
        // A pending event that fires after toggling off is a no-op: nothing may
        // switch themes while Follow is off.
        if !self.follows_omarchy() {
            return;
        }
        // Re-establishes monitoring when the state directory appears late.
        self.monitor_omarchy();
        self.update_omarchy_availability();
        // Single path: resolves the active theme, degrades with a retry, or resumes
        // following when the state is valid again.
        self.apply_selected();
    }

    /// Stops the active-theme watcher and drops a pending refresh.
    fn stop_omarchy_monitoring(&self) {
        for monitor in self.omarchy_monitors.take() {
            monitor.cancel();
        }
        if let Some(pending) = self.pending_omarchy_refresh.borrow_mut().take() {
            pending.remove();
        }
    }

    /// Arms the single Follow retry timer unless one is already armed.
    fn ensure_omarchy_retry(&self) {
        if self.omarchy_retry.borrow().is_some() {
            return;
        }
        let Some(manager) = self.self_weak.borrow().upgrade() else {
            return;
        };
        let weak = Rc::downgrade(&manager);
        let source = glib::timeout_add_local(OMARCHY_RETRY_INTERVAL, move || {
            let Some(manager) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if !manager.follows_omarchy() {
                manager.omarchy_retry.borrow_mut().take();
                return glib::ControlFlow::Break;
            }
            manager.refresh_omarchy_state();
            if manager.omarchy_degraded.get() && manager.omarchy_retry.borrow().is_some() {
                glib::ControlFlow::Continue
            } else {
                // Resolution succeeded: the timer ends here, releasing the source
                // without removing the currently dispatching one twice.
                manager.omarchy_retry.borrow_mut().take();
                glib::ControlFlow::Break
            }
        });
        #[cfg(test)]
        self.retry_armed_total.set(self.retry_armed_total.get() + 1);
        self.omarchy_retry.replace(Some(source));
    }

    fn cancel_omarchy_retry(&self) {
        if let Some(source) = self.omarchy_retry.borrow_mut().take() {
            source.remove();
        }
    }

    /// Arms the retry when a restored Follow session is already degraded.
    fn reconcile_omarchy_retry(&self) {
        if self.follows_omarchy() && self.omarchy_degraded.get() {
            self.ensure_omarchy_retry();
        }
    }

    fn update_omarchy_availability(&self) {
        let available = load_omarchy_theme(OmarchyVariant::Original).is_some()
            || (self.is_omarchy_available() && omarchy_state_dir().join("theme.name").is_file());
        if self.omarchy_available.replace(available) != available {
            self.preferences.notify_changes();
        }
    }

    /// Re-runs Omarchy discovery and merges the result into the theme list, then
    /// re-applies through the single theme-application path. Failures yield an
    /// empty Omarchy set without touching built-in or custom themes. Returns
    /// whether the Omarchy set changed.
    pub(crate) fn refresh_omarchy_catalog(&self) -> bool {
        let changed = self.set_omarchy_themes(discover_omarchy_themes());
        self.update_omarchy_availability();
        if changed {
            self.apply_selected();
        }
        changed
    }

    /// Off-thread discovery for focus and watcher refreshes; the merge and
    /// re-apply run back on the UI thread.
    pub(crate) fn refresh_omarchy_catalog_async(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        glib::MainContext::default().spawn_local(async move {
            let discovered = match gio::spawn_blocking(discover_omarchy_themes).await {
                Ok(themes) => themes,
                Err(error) => {
                    tracing::warn!(?error, "Omarchy catalog refresh failed");
                    Vec::new()
                }
            };
            let Some(manager) = weak.upgrade() else {
                return;
            };
            if manager.set_omarchy_themes(discovered) {
                manager.apply_selected();
            }
            manager.update_omarchy_availability();
        });
    }

    fn set_omarchy_themes(&self, discovered: Vec<Theme>) -> bool {
        let mut themes = self.themes.borrow_mut();
        let previous: Vec<(String, ThemeTokens)> = themes
            .iter()
            .filter(|theme| is_omarchy_theme_id(&theme.id))
            .map(|theme| (theme.id.clone(), theme.tokens.clone()))
            .collect();
        themes.retain(|theme| !is_omarchy_theme_id(&theme.id));
        append_omarchy_themes(&mut themes, discovered);
        let current: Vec<(String, ThemeTokens)> = themes
            .iter()
            .filter(|theme| is_omarchy_theme_id(&theme.id))
            .map(|theme| (theme.id.clone(), theme.tokens.clone()))
            .collect();
        drop(themes);
        if previous == current {
            return false;
        }
        self.theme_listeners.notify(self);
        true
    }

    /// Whether Follow is on but the active Omarchy theme cannot be resolved.
    pub fn omarchy_unavailable(&self) -> bool {
        self.omarchy_degraded.get()
    }

    /// Qualified ids of detected Omarchy themes, for catalog bindings.
    pub(crate) fn omarchy_theme_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self
            .themes
            .borrow()
            .iter()
            .filter(|theme| is_omarchy_theme_id(&theme.id))
            .map(|theme| theme.id.clone())
            .collect();
        ids.sort();
        ids
    }

    #[cfg(test)]
    pub(crate) fn omarchy_retry_armed(&self) -> bool {
        self.omarchy_retry.borrow().is_some()
    }

    #[cfg(test)]
    pub(crate) fn retry_armed_total(&self) -> u32 {
        self.retry_armed_total.get()
    }

    #[cfg(test)]
    pub(crate) fn omarchy_monitor_count(&self) -> usize {
        self.omarchy_monitors.borrow().len()
    }
}

impl Drop for ThemeManager {
    fn drop(&mut self) {
        self.stop_omarchy_monitoring();
        self.cancel_omarchy_retry();
    }
}

fn is_omarchy_theme_event(file: &gio::File) -> bool {
    file.path().is_some_and(|path| {
        let state = omarchy_state_dir();
        state.starts_with(&path)
            || path == state.join("theme")
            || path == state.join("theme.name")
            || path == state.join("theme/colors.toml")
    })
}

fn builtins() -> Vec<Theme> {
    let mut themes: Vec<_> = toml::from_str::<ThemeCatalog>(THEME_CATALOG)
        .map(|catalog| {
            catalog
                .themes
                .into_iter()
                .map(|theme| Theme {
                    id: theme.id,
                    tokens: theme.tokens,
                    custom: false,
                })
                .collect()
        })
        .unwrap_or_default();
    themes.sort_by_key(|theme| theme.tokens.name.to_lowercase());
    themes
}

fn merge_builtin_and_custom_themes(mut builtins: Vec<Theme>, custom: Vec<Theme>) -> Vec<Theme> {
    builtins.retain(|builtin| !custom.iter().any(|theme| theme.id == builtin.id));
    builtins.extend(custom);
    builtins
}

fn merge_builtin_custom_and_omarchy(
    builtins: Vec<Theme>,
    custom: Vec<Theme>,
    omarchy: Vec<Theme>,
) -> Vec<Theme> {
    let mut themes = merge_builtin_and_custom_themes(builtins, custom);
    append_omarchy_themes(&mut themes, omarchy);
    themes
}

fn append_omarchy_themes(themes: &mut Vec<Theme>, omarchy: Vec<Theme>) {
    for theme in omarchy {
        // Qualified ids cannot collide with built-in or custom ids; a hand-made
        // `omarchy:<slug>.toml` custom file still wins over the detected copy.
        if !themes.iter().any(|existing| existing.id == theme.id) {
            themes.push(theme);
        }
    }
}

fn omarchy_user_themes_dir() -> PathBuf {
    glib::user_config_dir().join("omarchy").join("themes")
}

fn omarchy_bundled_themes_dir() -> PathBuf {
    omarchy_bundled_themes_dir_with(std::env::var_os("OMARCHY_PATH"))
}

fn omarchy_bundled_themes_dir_with(override_dir: Option<std::ffi::OsString>) -> PathBuf {
    override_dir
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(OMARCHY_BUNDLED_DIR_FALLBACK))
        .join("themes")
}

fn is_valid_omarchy_slug(slug: &str) -> bool {
    !slug.is_empty()
        && slug != "."
        && slug != ".."
        && !slug.contains('/')
        && !slug.contains('\\')
        && !slug.contains('\0')
}

/// Every installed Omarchy theme as an independent `omarchy:<slug>` entry. Reads the
/// same two sources `omarchy-theme-list` enumerates without shelling out. Missing
/// directories or failures yield an empty list and only a log; Strata never writes
/// to Omarchy directories.
fn discover_omarchy_themes() -> Vec<Theme> {
    discover_omarchy_themes_in(&omarchy_user_themes_dir(), &omarchy_bundled_themes_dir())
}

fn discover_omarchy_themes_in(user_dir: &Path, bundled_dir: &Path) -> Vec<Theme> {
    let mut slugs = BTreeSet::new();
    for dir in [user_dir, bundled_dir] {
        match fs::read_dir(dir) {
            Ok(entries) => {
                for entry in entries.filter_map(Result::ok) {
                    let slug = entry.file_name().to_string_lossy().into_owned();
                    if is_valid_omarchy_slug(&slug) {
                        slugs.insert(slug);
                    } else {
                        tracing::debug!(slug = %slug, dir = %dir.display(), "skipping invalid Omarchy theme name");
                    }
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                tracing::debug!(dir = %dir.display(), "no Omarchy theme directory");
            }
            Err(error) => {
                tracing::debug!(%error, dir = %dir.display(), "unable to list Omarchy theme directory");
            }
        }
    }
    let mut themes = Vec::new();
    let mut disagreements = 0u32;
    for slug in slugs {
        if let Some((tokens, declared_light)) = resolve_omarchy_theme(&slug, user_dir, bundled_dir)
        {
            // Luminance classification stays the default; a declared `mode` that
            // disagrees is only noted.
            if declared_light
                .is_some_and(|declared| declared != background_is_light(&tokens.background))
            {
                disagreements += 1;
                tracing::debug!(slug = %slug, "Omarchy theme declares a mode that disagrees with its luminance; using luminance");
            }
            themes.push(Theme {
                id: format!("{OMARCHY_THEME_ID_PREFIX}{slug}"),
                tokens,
                custom: false,
            });
        }
    }
    if disagreements > 0 {
        tracing::debug!(
            count = disagreements,
            "Omarchy themes declare a mode that disagrees with luminance"
        );
    }
    themes.sort_by(|left, right| {
        left.tokens
            .name
            .to_lowercase()
            .cmp(&right.tokens.name.to_lowercase())
            .then(left.id.cmp(&right.id))
    });
    themes
}

enum OmarchyCopy {
    Absent,
    Unreadable(io::Error),
    Source(String),
}

fn read_omarchy_copy(dir: &Path) -> OmarchyCopy {
    let path = dir.join("colors.toml");
    match fs::read_to_string(&path) {
        Ok(source) => OmarchyCopy::Source(source),
        Err(error) => match fs::symlink_metadata(&path) {
            // The entry exists (file, directory, or link, including a broken one)
            // but cannot be read.
            Ok(_) => OmarchyCopy::Unreadable(error),
            Err(_) => OmarchyCopy::Absent,
        },
    }
}

/// The user `colors.toml` replaces the bundled one wholesale; an empty or broken
/// user folder never hides a valid bundled theme. A slug is skipped only when
/// neither location holds a valid `colors.toml`.
fn resolve_omarchy_theme(
    slug: &str,
    user_dir: &Path,
    bundled_dir: &Path,
) -> Option<(ThemeTokens, Option<bool>)> {
    match read_omarchy_copy(&user_dir.join(slug)) {
        OmarchyCopy::Source(source) => {
            if let Some(theme) = adapt_omarchy_copy(slug, &source) {
                return Some(theme);
            }
            tracing::warn!(
                slug,
                "skipping invalid Omarchy user theme; trying the bundled copy"
            );
        }
        OmarchyCopy::Unreadable(error) => {
            tracing::warn!(%error, slug, "skipping unreadable Omarchy user theme; trying the bundled copy");
        }
        OmarchyCopy::Absent => {}
    }
    match read_omarchy_copy(&bundled_dir.join(slug)) {
        OmarchyCopy::Source(source) => {
            if let Some(theme) = adapt_omarchy_copy(slug, &source) {
                return Some(theme);
            }
            tracing::warn!(slug, "skipping invalid bundled Omarchy theme");
        }
        OmarchyCopy::Unreadable(error) => {
            tracing::warn!(%error, slug, "skipping unreadable bundled Omarchy theme");
        }
        OmarchyCopy::Absent => {}
    }
    None
}

fn adapt_omarchy_copy(slug: &str, source: &str) -> Option<(ThemeTokens, Option<bool>)> {
    let mut tokens = tokens_from_quattro(slug, source, OmarchyVariant::Original)?;
    validate_tokens(&tokens).ok()?;
    // Canonical in memory only; Omarchy files stay as written.
    for color in tokens.color_fields_mut() {
        *color = canonical_color(color)?;
    }
    let declared = omarchy_declared_light(source);
    Some((tokens, declared))
}

fn omarchy_declared_light(source: &str) -> Option<bool> {
    let values: toml::Value = toml::from_str(source).ok()?;
    match values.get("mode")?.as_str()? {
        "light" => Some(true),
        "dark" => Some(false),
        _ => None,
    }
}

/// Luminance classification shared by the library filter and Omarchy discovery.
pub(crate) fn background_is_light(background: &str) -> bool {
    let Some(channels) = parse_rgb_channels(background) else {
        return false;
    };
    let channel = |index: usize| {
        let value = f64::from(channels[index]) / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(0) + 0.7152 * channel(1) + 0.0722 * channel(2) > 0.4
}

fn azure_tokens() -> ThemeTokens {
    builtins()
        .into_iter()
        .find(|theme| theme.id == "azure-glow")
        .map(|theme| theme.tokens)
        .unwrap_or_else(|| ThemeTokens {
            name: "Azure Glow".to_owned(),
            background: "#0c1a2b".to_owned(),
            surface: "#122438".to_owned(),
            text: "#c9deed".to_owned(),
            accent: "#4fd6ff".to_owned(),
            danger: default_danger(),
            muted: "#1e3a52".to_owned(),
            highlight: "#244d68".to_owned(),
            border: "#315b75".to_owned(),
            dim_text: "#6f8da3".to_owned(),
            syntax_keyword: None,
            syntax_string: None,
            syntax_constant: None,
            syntax_type: None,
            syntax_preprocessor: None,
        })
}

fn load_custom_themes() -> Vec<Theme> {
    let Ok(entries) = fs::read_dir(themes_directory()) else {
        return Vec::new();
    };
    let mut themes: Vec<_> = entries
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "toml")
        })
        .filter_map(|entry| {
            let id = entry.path().file_stem()?.to_string_lossy().into_owned();
            let source = fs::read_to_string(entry.path()).ok()?;
            let mut tokens: ThemeTokens = toml::from_str(&source).ok()?;
            validate_tokens(&tokens).ok()?;
            // Canonical in memory only; the user's file stays as written.
            for color in tokens.color_fields_mut() {
                *color = canonical_color(color)?;
            }
            Some(Theme {
                id,
                tokens,
                custom: true,
            })
        })
        .collect();
    themes.sort_by(|left, right| left.tokens.name.cmp(&right.tokens.name));
    themes
}

fn load_omarchy_theme(variant: OmarchyVariant) -> Option<ThemeTokens> {
    let state = omarchy_state_dir();
    let name = fs::read_to_string(state.join("theme.name")).ok()?;
    let colors = fs::read_to_string(state.join("theme/colors.toml")).ok()?;
    tokens_from_quattro(name.trim(), &colors, variant)
}

fn load_omarchy_source_palette() -> Option<SourcePalette> {
    let colors = fs::read_to_string(omarchy_state_dir().join("theme/colors.toml")).ok()?;
    source_palette_from_quattro(&colors)
}

fn tokens_from_quattro(name: &str, source: &str, variant: OmarchyVariant) -> Option<ThemeTokens> {
    let values: toml::Value = toml::from_str(source).ok()?;
    let get = |key: &str| quattro_color(&values, key);
    let source_background = get("background")?;
    let text = get("foreground")?;
    let accent = get("accent")?;
    let selection = get("selection").unwrap_or_else(|| accent.clone());
    let shadow = get("color8").unwrap_or_else(|| source_background.clone());
    let mut tokens = ThemeTokens {
        name: title_case_slug(name),
        background: blend(&source_background, &shadow, 0.35),
        surface: blend(&source_background, &shadow, 0.65),
        muted: blend(&shadow, &text, 0.10),
        highlight: selection,
        border: blend(&shadow, &text, 0.36),
        dim_text: blend(&source_background, &text, 0.62),
        text,
        accent,
        danger: get("color1").unwrap_or_else(default_danger),
        syntax_keyword: get("magenta").or_else(|| get("color5")),
        syntax_string: get("green").or_else(|| get("color2")),
        syntax_constant: get("orange").or_else(|| get("color9")),
        syntax_type: get("cyan").or_else(|| get("color3")),
        syntax_preprocessor: get("yellow"),
    };
    if variant != OmarchyVariant::Original {
        let palette = source_palette_from_quattro(source);
        omarchy::apply_variant(&mut tokens, &source_background, variant, palette.as_ref());
    }
    Some(tokens)
}

fn source_palette_from_quattro(source: &str) -> Option<SourcePalette> {
    let values: toml::Value = toml::from_str(source).ok()?;
    let get = |key: &str| quattro_color(&values, key);
    Some(SourcePalette {
        statement: get("magenta").or_else(|| get("blue"))?,
        string: get("green")?,
        constant: get("orange").or_else(|| get("yellow"))?,
        type_color: get("cyan").or_else(|| get("blue"))?,
        preprocessor: get("yellow")?,
    })
}

/// Quattro values reach GTK CSS, GtkSourceView XML and SVG strokes, so a value GTK
/// cannot parse is treated as missing; callers decide whether that is fatal.
fn quattro_color(values: &toml::Value, key: &str) -> Option<String> {
    canonical_color(values.get(key)?.as_str()?)
}

fn default_danger() -> String {
    "#e5484d".to_owned()
}

fn validate_tokens(tokens: &ThemeTokens) -> Result<(), &'static str> {
    if tokens.name.trim().is_empty() {
        return Err("Enter a theme name");
    }
    if tokens
        .color_fields()
        .any(|color| gdk::RGBA::parse(color).is_err())
    {
        return Err("Every color must be a valid CSS color");
    }
    Ok(())
}

fn source_style_scheme() -> Option<sourceview5::StyleScheme> {
    sourceview5::StyleSchemeManager::default().scheme("strata-current")
}

pub(super) fn register_source_buffer(buffer: &sourceview5::Buffer) {
    ensure_source_style_scheme_installed();
    buffer.set_style_scheme(source_style_scheme().as_ref());
    SOURCE_BUFFERS.with(|buffers| {
        let mut buffers = buffers.borrow_mut();
        buffers.retain(|buffer| buffer.upgrade().is_some());
        let weak = glib::WeakRef::new();
        weak.set(Some(buffer));
        buffers.push(weak);
    });
}

pub(super) fn register_document_buffer(buffer: &gtk::TextBuffer) {
    let tokens = ThemeManager::shared().appearance_tokens();
    style_document_buffer(buffer, &tokens);
    DOCUMENT_BUFFERS.with(|buffers| {
        let mut buffers = buffers.borrow_mut();
        buffers.retain(|buffer| buffer.upgrade().is_some());
        let weak = glib::WeakRef::new();
        weak.set(Some(buffer));
        buffers.push(weak);
    });
}

pub(super) fn register_document_view(view: &super::document_view::DocumentTextView) {
    let tokens = ThemeManager::shared().appearance_tokens();
    style_document_view(view, &tokens);
    DOCUMENT_VIEWS.with(|views| {
        let mut views = views.borrow_mut();
        views.retain(|view| view.upgrade().is_some());
        let weak = glib::WeakRef::new();
        weak.set(Some(view));
        views.push(weak);
    });
}

fn style_document_buffers(tokens: &ThemeTokens) {
    DOCUMENT_BUFFERS.with(|buffers| {
        buffers.borrow_mut().retain(|buffer| {
            let Some(buffer) = buffer.upgrade() else {
                return false;
            };
            style_document_buffer(&buffer, tokens);
            true
        });
    });
}

fn style_document_views(tokens: &ThemeTokens) {
    DOCUMENT_VIEWS.with(|views| {
        views.borrow_mut().retain(|view| {
            let Some(view) = view.upgrade() else {
                return false;
            };
            style_document_view(&view, tokens);
            true
        });
    });
}

fn style_document_view(view: &super::document_view::DocumentTextView, tokens: &ThemeTokens) {
    if let (Ok(fill), Ok(border), Ok(selection)) = (
        gdk::RGBA::parse(blend(&tokens.surface, &tokens.muted, 0.54)),
        gdk::RGBA::parse(blend(&tokens.surface, &tokens.border, 0.5)),
        gdk::RGBA::parse(&tokens.accent),
    ) {
        view.set_colors(fill, border, selection);
    }
}

fn style_document_buffer(buffer: &gtk::TextBuffer, tokens: &ThemeTokens) {
    let parse = |value: &str| gdk::RGBA::parse(value).ok();
    let table = buffer.tag_table();
    if let Some(color) = parse(&tokens.accent) {
        for name in ["document-accent", "document-link"] {
            if let Some(tag) = table.lookup(name) {
                tag.set_foreground_rgba(Some(&color));
            }
        }
    }
    if let Some(color) = parse(&tokens.dim_text)
        && let Some(tag) = table.lookup("document-dim")
    {
        tag.set_foreground_rgba(Some(&color));
    }
    if let Some(color) = parse(&tokens.text)
        && let Some(tag) = table.lookup("document-link-hover")
    {
        tag.set_foreground_rgba(Some(&color));
    }
    if let Some(color) = parse(&tokens.background)
        && let Some(tag) = table.lookup("document-selection")
    {
        tag.set_foreground_rgba(Some(&color));
    }
    if let Some(mut color) = parse(&tokens.highlight) {
        color.set_alpha(0.36);
        if let Some(tag) = table.lookup("document-quote") {
            tag.set_paragraph_background_rgba(Some(&color));
        }
        color.set_alpha(0.5);
        if let Some(tag) = table.lookup("document-link-hover") {
            tag.set_background_rgba(Some(&color));
        }
    }
}

fn stage_source_style_scheme(tokens: &ThemeTokens, source_palette: Option<&SourcePalette>) {
    PENDING_STYLE_TOKENS
        .with(|pending| pending.replace(Some((tokens.clone(), source_palette.cloned()))));
    STYLE_SCHEME_DIRTY.with(|dirty| dirty.set(true));
    let live = SOURCE_BUFFERS.with(|buffers| {
        buffers
            .borrow_mut()
            .retain(|buffer| buffer.upgrade().is_some());
        !buffers.borrow().is_empty()
    });
    if live {
        ensure_source_style_scheme_installed();
    }
}

/// Writes the staged scheme and rescans the style manager, once per staged token set.
fn ensure_source_style_scheme_installed() {
    if !STYLE_SCHEME_DIRTY.with(|dirty| dirty.get()) {
        return;
    }
    let pending = PENDING_STYLE_TOKENS.with(|pending| pending.borrow().clone());
    let Some((tokens, palette)) = pending else {
        return;
    };
    let directory = glib::user_cache_dir().join("strata").join("source-styles");
    if let Err(error) = fs::create_dir_all(&directory).and_then(|()| {
        let value = source_style_scheme_xml(&tokens, palette.as_ref());
        crate::storage::atomic_write(&directory.join("strata-current.xml"), value.as_bytes())
    }) {
        tracing::warn!(%error, "unable to write preview syntax style");
        return;
    }

    let manager = sourceview5::StyleSchemeManager::default();
    SOURCE_STYLE_PATH_INSTALLED.with(|installed| {
        if !installed.replace(true) {
            manager.append_search_path(&directory.to_string_lossy());
        }
    });
    manager.force_rescan();
    STYLE_SCHEME_DIRTY.with(|dirty| dirty.set(false));
    let scheme = manager.scheme("strata-current");
    SOURCE_BUFFERS.with(|buffers| {
        buffers.borrow_mut().retain(|buffer| {
            let Some(buffer) = buffer.upgrade() else {
                return false;
            };
            buffer.set_style_scheme(scheme.as_ref());
            true
        });
    });
}

fn resolved_source_palette(tokens: &ThemeTokens, palette: Option<&SourcePalette>) -> SourcePalette {
    let fallback = SourcePalette {
        statement: tokens.accent.clone(),
        string: blend(&tokens.accent, &tokens.text, 0.48),
        constant: blend(&tokens.accent, &tokens.text, 0.18),
        type_color: blend(&tokens.accent, &tokens.text, 0.24),
        preprocessor: blend(&tokens.accent, &tokens.text, 0.32),
    };
    let palette = palette.unwrap_or(&fallback);
    let resolve = |token: &Option<String>, fallback: &str| {
        token
            .as_deref()
            .filter(|value| gdk::RGBA::parse(*value).is_ok())
            .unwrap_or(fallback)
            .to_owned()
    };
    SourcePalette {
        statement: resolve(&tokens.syntax_keyword, &palette.statement),
        string: resolve(&tokens.syntax_string, &palette.string),
        constant: resolve(&tokens.syntax_constant, &palette.constant),
        type_color: resolve(&tokens.syntax_type, &palette.type_color),
        preprocessor: resolve(&tokens.syntax_preprocessor, &palette.preprocessor),
    }
}

impl ThemeTokens {
    // Exhaustive destructuring makes a new field fail to compile until it is classified.
    fn color_fields(&self) -> impl Iterator<Item = &String> {
        let Self {
            name: _,
            background,
            surface,
            text,
            accent,
            danger,
            muted,
            highlight,
            border,
            dim_text,
            syntax_keyword,
            syntax_string,
            syntax_constant,
            syntax_type,
            syntax_preprocessor,
        } = self;
        [
            background, surface, text, accent, danger, muted, highlight, border, dim_text,
        ]
        .into_iter()
        .chain(
            [
                syntax_keyword,
                syntax_string,
                syntax_constant,
                syntax_type,
                syntax_preprocessor,
            ]
            .into_iter()
            .flatten(),
        )
    }

    fn color_fields_mut(&mut self) -> impl Iterator<Item = &mut String> {
        let Self {
            name: _,
            background,
            surface,
            text,
            accent,
            danger,
            muted,
            highlight,
            border,
            dim_text,
            syntax_keyword,
            syntax_string,
            syntax_constant,
            syntax_type,
            syntax_preprocessor,
        } = self;
        [
            background, surface, text, accent, danger, muted, highlight, border, dim_text,
        ]
        .into_iter()
        .chain(
            [
                syntax_keyword,
                syntax_string,
                syntax_constant,
                syntax_type,
                syntax_preprocessor,
            ]
            .into_iter()
            .flatten(),
        )
    }

    pub(super) fn initialize_syntax_colors(&mut self) {
        let palette = resolved_source_palette(self, None);
        self.syntax_keyword = Some(palette.statement);
        self.syntax_string = Some(palette.string);
        self.syntax_constant = Some(palette.constant);
        self.syntax_type = Some(palette.type_color);
        self.syntax_preprocessor = Some(palette.preprocessor);
    }
}

fn source_style_scheme_xml(tokens: &ThemeTokens, palette: Option<&SourcePalette>) -> String {
    let palette = resolved_source_palette(tokens, palette);
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<style-scheme id="strata-current" _name="Strata Current Theme" version="1.0">
  <color name="background" value="{}"/>
  <color name="surface" value="{}"/>
  <color name="text" value="{}"/>
  <color name="accent" value="{}"/>
  <color name="danger" value="{}"/>
  <color name="selection" value="{}"/>
  <color name="dim" value="{}"/>
  <color name="string" value="{}"/>
  <color name="constant" value="{}"/>
  <color name="type" value="{}"/>
  <color name="statement" value="{}"/>
  <color name="preprocessor" value="{}"/>
  <style name="text" foreground="text" background="surface"/>
  <style name="selection" foreground="background" background="accent"/>
  <style name="cursor" foreground="accent"/>
  <style name="current-line" background="background"/>
  <style name="line-numbers" foreground="dim" background="background"/>
  <style name="def:comment" foreground="dim" italic="true"/>
  <style name="def:shebang" foreground="dim" bold="true"/>
  <style name="def:string" foreground="string"/>
  <style name="def:constant" foreground="constant"/>
  <style name="def:special-char" foreground="constant"/>
  <style name="def:identifier" foreground="text"/>
  <style name="def:statement" foreground="statement" bold="true"/>
  <style name="def:type" foreground="type" bold="true"/>
  <style name="def:preprocessor" foreground="preprocessor"/>
  <style name="def:heading" foreground="accent" bold="true"/>
  <style name="def:link-destination" foreground="string" underline="single"/>
  <style name="def:error" foreground="background" background="danger" bold="true"/>
</style-scheme>
"#,
        color_to_hex(&tokens.background),
        color_to_hex(&tokens.surface),
        color_to_hex(&tokens.text),
        color_to_hex(&tokens.accent),
        color_to_hex(&tokens.danger),
        color_to_hex(&tokens.highlight),
        color_to_hex(&tokens.dim_text),
        color_to_hex(&palette.string),
        color_to_hex(&palette.constant),
        color_to_hex(&palette.type_color),
        color_to_hex(&palette.statement),
        color_to_hex(&palette.preprocessor),
    )
}

const INTERFACE_FONT_FAMILY: &str = "JetBrains Mono";

fn interface_font_name(root_font_px: f64) -> String {
    format!("{INTERFACE_FONT_FAMILY} {root_font_px:.6}px")
}

/// GTK's automatic font rendering hints glyph outlines but not their metrics,
/// so GPU glyph atlas slots are sized from unhinted extents and lose the top
/// pixel row at some sizes (GNOME/gtk#8395). Manual rendering hints metrics
/// too and follows the desktop's hinting settings. Application values outrank
/// the settings portal, so this holds for the whole process.
fn pin_manual_font_rendering() {
    let Some(settings) = gtk::Settings::default() else {
        return;
    };
    let Some(property) = settings.find_property("gtk-font-rendering") else {
        return;
    };
    let Some(manual) = glib::EnumClass::with_type(property.value_type())
        .and_then(|class| class.to_value_by_nick("manual"))
    else {
        return;
    };
    settings.set_property("gtk-font-rendering", manual);
}

fn apply_interface_font(root_font_px: f64) {
    if let Some(settings) = gtk::Settings::default() {
        settings.set_gtk_font_name(Some(&interface_font_name(root_font_px)));
    }
}

fn tokens_css(tokens: &ThemeTokens, root_font_px: f64) -> String {
    let scale = root_font_px / 13.0;
    let header = (40.0 * scale).round();
    // Column headers add six pixels of padding and three extra border pixels.
    let column_header = header - 9.0;
    let control = (24.0 * scale).round();
    let browser_header = (control + 8.0).max(36.0);
    // Miller headers keep their three-pixel indicator, bottom border and balancing inset.
    let browser_column_header = browser_header - 6.0;
    let sizing = format!(
        "headerbar, headerbar > windowhandle > box, .mode-pane-header, .preview-header {{ min-height: {header}px; }}\n.column-header {{ min-height: {column_header}px; }}\nheaderbar .sidebar-toggle, button.header-action, headerbar menubutton.header-action > button, .preview-header-action, button.column-header-action, menubutton.column-header-action > button {{ min-width: {control}px; min-height: {control}px; }}\n.file-operation-card button.progress-card-action {{ min-width: 0; min-height: 0; }}\n.tab-strip, headerbar.browser-header, headerbar.browser-header > windowhandle > box, .mode-pane-header, .preview-header {{ min-height: {browser_header}px; }}\n.directory-column .column-header {{ min-height: {browser_column_header}px; }}\n.mode-pane-header .list-navigation-button, .sidebar-row > box > image, .sidebar-device-action {{ min-width: {control}px; min-height: {control}px; }}\n"
    );
    let colors = format!(
        "@define-color strata_bg {};\n@define-color strata_surface {};\n@define-color strata_text {};\n@define-color strata_accent {};\n@define-color strata_danger {};\n@define-color strata_muted {};\n@define-color strata_highlight {};\n@define-color strata_border {};\n@define-color strata_dim_text {};\nwindow, popover, popover.background {{ font-size: {root_font_px:.6}px; }}\n",
        tokens.background,
        tokens.surface,
        tokens.text,
        tokens.accent,
        tokens.danger,
        tokens.muted,
        tokens.highlight,
        tokens.border,
        tokens.dim_text,
    );
    colors + &sizing
}

/// Parses colours GTK accepts (`#rgb`, `#rrggbb`, `rgb(...)`, names) into 8-bit
/// channels. Strata emits these channels as `#rrggbb` in GtkSourceView schemes.
pub(crate) fn parse_rgb_channels(value: &str) -> Option<[u8; 3]> {
    parse_rgba_channels(value).map(|[red, green, blue, _]| [red, green, blue])
}

fn parse_rgba_channels(value: &str) -> Option<[u8; 4]> {
    let color = gdk::RGBA::parse(value).ok()?;
    let channel = |component: f32| (f64::from(component).clamp(0.0, 1.0) * 255.0).round() as u8;
    Some([
        channel(color.red()),
        channel(color.green()),
        channel(color.blue()),
        channel(color.alpha()),
    ])
}

/// Rewrites a color GTK can parse as `#rrggbb`, or `#rrggbbaa` when translucent.
/// `gdk::RGBA::parse` also accepts 9-, 12- and 16-digit hex, which GTK CSS rejects.
fn canonical_color(value: &str) -> Option<String> {
    let [red, green, blue, alpha] = parse_rgba_channels(value)?;
    let rgb = hex_from_channels([red, green, blue]);
    Some(if alpha == u8::MAX {
        rgb
    } else {
        format!("{rgb}{alpha:02x}")
    })
}

fn hex_from_channels(channels: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", channels[0], channels[1], channels[2])
}

/// Canonicalizes a colour token to Strata's `#rrggbb` scheme representation.
pub(crate) fn color_to_hex(value: &str) -> String {
    parse_rgb_channels(value)
        .map(hex_from_channels)
        .unwrap_or_else(|| value.to_owned())
}

fn blend(left: &str, right: &str, amount: f64) -> String {
    let (Some(left), Some(right)) = (parse_rgb_channels(left), parse_rgb_channels(right)) else {
        return right.to_owned();
    };
    let channel = |index: usize| {
        let a = f64::from(left[index]);
        let b = f64::from(right[index]);
        (a + (b - a) * amount).round() as u32
    };
    format!("#{:02x}{:02x}{:02x}", channel(0), channel(1), channel(2))
}

fn slugify(name: &str) -> String {
    name.trim()
        .to_lowercase()
        .chars()
        .fold(String::new(), |mut slug, character| {
            if character.is_ascii_alphanumeric() {
                slug.push(character);
            } else if !slug.is_empty() && !slug.ends_with('-') {
                slug.push('-');
            }
            slug
        })
        .trim_end_matches('-')
        .to_owned()
}

const FALLBACK_CUSTOM_THEME_ID: &str = "custom-theme";

/// File id for a new custom theme: the ASCII slug of `name`, or a generic id when the
/// name has no ASCII letters or digits, suffixed until `taken` no longer matches.
fn custom_theme_id(name: &str, taken: impl Fn(&str) -> bool) -> String {
    let slug = slugify(name);
    let base = if slug.is_empty() {
        FALLBACK_CUSTOM_THEME_ID
    } else {
        slug.as_str()
    };
    let mut id = base.to_owned();
    let mut suffix = 2;
    while taken(&id) {
        id = format!("{base}-{suffix}");
        suffix += 1;
    }
    id
}

fn title_case_slug(slug: &str) -> String {
    slug.split('-')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn themes_directory() -> PathBuf {
    super::preferences::config_directory().join("themes")
}

pub(in crate::ui) fn omarchy_state_dir() -> PathBuf {
    gtk::glib::home_dir().join(".local/state/omarchy/current")
}

#[cfg(test)]
mod tests;
