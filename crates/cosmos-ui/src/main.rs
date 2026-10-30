// SPDX-License-Identifier: GPL-3.0-or-later
//! The Cosmos window: pick a wallpaper, see the palette, nudge it, save it.
//! It sets no colours of its own, so it follows whatever GTK theme the desktop uses.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use cosmos_core::color::Hex;
use cosmos_core::palette::{Mode, Palette};
use cosmos_core::sample::{Limits, Thumbnail, load, read_file};
use cosmos_out::hypora::{self, Applied, Request, ThemeName, Wallpaper};
use cosmos_out::json::to_json;
use cosmos_out::template::variables;
use gtk::prelude::*;
use gtk::{gdk, gio, glib};

const THUMB_SIDE: u32 = 960;

/// Everything worked out from one wallpaper; built on a worker thread.
struct Loaded {
    file_name: String,
    bytes: Vec<u8>,
    pixels: Vec<Hex>,
    thumb: Thumbnail,
}

#[derive(Default)]
struct State {
    file_name: String,
    bytes: Vec<u8>,
    pixels: Vec<Hex>,
    /// None follows the wallpaper.
    mode: Option<Mode>,
    /// The palette before the accent was nudged.
    base: Option<Palette>,
    /// Bumped on every request, so a slow answer for an old choice is dropped.
    generation: u64,
}

struct Ui {
    window: gtk::ApplicationWindow,
    picture: gtk::Picture,
    swatches: gtk::DrawingArea,
    hue: gtk::Scale,
    chroma: gtk::Scale,
    name: gtk::Entry,
    apply: gtk::CheckButton,
    force: gtk::CheckButton,
    status: gtk::Label,
    controls: gtk::Box,
    state: RefCell<State>,
    shown: Rc<RefCell<Option<Palette>>>,
    /// True while the sliders are being set by code, so that does not count as a nudge.
    syncing: Cell<bool>,
}

fn main() -> glib::ExitCode {
    let initial = std::env::args().nth(1).map(PathBuf::from);
    let app = gtk::Application::builder()
        .application_id("io.github.Misfitxtm.Cosmos")
        .build();
    app.connect_activate(move |app| {
        let ui = build(app);
        if let Some(path) = initial.clone() {
            open(&ui, path);
        }
    });
    // The command line was read above; GTK should not try to parse it as well
    app.run_with_args::<&str>(&[])
}

fn build(app: &gtk::Application) -> Rc<Ui> {
    gtk::Window::set_default_icon_name("io.github.Misfitxtm.Cosmos");

    let picture = gtk::Picture::builder()
        .content_fit(gtk::ContentFit::Contain)
        .hexpand(true)
        .vexpand(true)
        .width_request(280)
        .height_request(200)
        .build();

    let shown: Rc<RefCell<Option<Palette>>> = Rc::new(RefCell::new(None));
    let swatches = gtk::DrawingArea::builder()
        .content_height(150)
        .hexpand(true)
        .build();
    let for_draw = shown.clone();
    swatches.set_draw_func(move |_, cr, w, h| draw_swatches(cr, w, h, for_draw.borrow().as_ref()));

    let mode_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    mode_box.add_css_class("linked");
    let auto = gtk::ToggleButton::with_label("Auto");
    let dark = gtk::ToggleButton::with_label("Dark");
    let light = gtk::ToggleButton::with_label("Light");
    dark.set_group(Some(&auto));
    light.set_group(Some(&auto));
    auto.set_active(true);
    for b in [&auto, &dark, &light] {
        mode_box.append(b);
    }

    let hue = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 360.0, 1.0);
    let chroma = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 0.4, 0.005);
    for s in [&hue, &chroma] {
        s.set_hexpand(true);
    }

    let name = gtk::Entry::builder().placeholder_text("Theme name").build();
    let apply = gtk::CheckButton::with_label("Switch to it now");
    let force = gtk::CheckButton::with_label("Replace a theme with this name");
    let save = gtk::Button::with_label("Save Hypora theme");
    save.add_css_class("suggested-action");
    let copy = gtk::Button::with_label("Copy palette as JSON");
    let status = gtk::Label::builder()
        .wrap(true)
        .xalign(0.0)
        .selectable(true)
        .build();

    let controls = gtk::Box::new(gtk::Orientation::Vertical, 10);
    controls.append(&labelled("Mode", &mode_box));
    controls.append(&labelled("Accent hue", &hue));
    controls.append(&labelled("Chroma", &chroma));

    let hypora_here = hypora::config_root().is_some_and(|r| r.is_dir());
    let hypora_box = gtk::Box::new(gtk::Orientation::Vertical, 10);
    hypora_box.append(&name);
    hypora_box.append(&apply);
    hypora_box.append(&force);
    hypora_box.append(&save);
    hypora_box.set_visible(hypora_here);

    let side = gtk::Box::new(gtk::Orientation::Vertical, 14);
    side.append(&swatches);
    side.append(&controls);
    side.append(&hypora_box);
    side.append(&copy);
    side.append(&status);

    // One column that scrolls, so it works at any width or height (tiling window managers
    // hand out whatever size they like)
    let content = gtk::Box::new(gtk::Orientation::Vertical, 14);
    content.set_margin_top(18);
    content.set_margin_bottom(18);
    content.set_margin_start(18);
    content.set_margin_end(18);
    content.append(&picture);
    content.append(&side);
    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&content)
        .build();

    let open_btn = gtk::Button::with_label("Choose wallpaper…");
    let header = gtk::HeaderBar::new();
    header.pack_start(&open_btn);

    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("Cosmos")
        .default_width(560)
        .default_height(820)
        .child(&scroller)
        .build();
    window.set_titlebar(Some(&header));
    controls.set_sensitive(false);

    let ui = Rc::new(Ui {
        window,
        picture,
        swatches,
        hue,
        chroma,
        name,
        apply,
        force,
        status,
        controls,
        state: RefCell::new(State::default()),
        shown,
        syncing: Cell::new(false),
    });
    ui.say(if hypora_here {
        "Choose a wallpaper to begin."
    } else {
        "Choose a wallpaper to begin. Hypora was not found, so saving a Hypora theme is hidden."
    });

    let u = ui.clone();
    open_btn.connect_clicked(move |_| choose_file(&u));
    for (btn, mode) in [
        (&auto, None),
        (&dark, Some(Mode::Dark)),
        (&light, Some(Mode::Light)),
    ] {
        let u = ui.clone();
        btn.connect_toggled(move |b| {
            if b.is_active() {
                u.state.borrow_mut().mode = mode;
                recompute(&u);
            }
        });
    }
    for s in [&ui.hue, &ui.chroma] {
        let u = ui.clone();
        s.connect_value_changed(move |_| nudge(&u));
    }
    let u = ui.clone();
    save.connect_clicked(move |_| save_theme(&u));
    let u = ui.clone();
    copy.connect_clicked(move |_| {
        if let Some(p) = u.shown.borrow().as_ref() {
            u.window.clipboard().set_text(&to_json(p));
            u.say("Copied the palette as JSON.");
        }
    });

    ui.window.present();
    ui
}

impl Ui {
    fn say(&self, text: &str) {
        self.status.set_text(text);
    }

    fn show(&self, palette: Palette) {
        *self.shown.borrow_mut() = Some(palette);
        self.swatches.queue_draw();
    }
}

/// A caption and a control side by side.
fn labelled(text: &str, w: &impl IsA<gtk::Widget>) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row.append(
        &gtk::Label::builder()
            .label(text)
            .xalign(0.0)
            .width_chars(11)
            .build(),
    );
    row.append(w);
    row
}

fn choose_file(ui: &Rc<Ui>) {
    let filter = gtk::FileFilter::new();
    filter.set_name(Some("Images"));
    for mime in ["image/png", "image/jpeg", "image/webp"] {
        filter.add_mime_type(mime);
    }
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&filter);
    let dialog = gtk::FileDialog::builder()
        .title("Choose a wallpaper")
        .filters(&filters)
        .build();
    let u = ui.clone();
    dialog.open(Some(&ui.window), gio::Cancellable::NONE, move |res| {
        if let Some(path) = res.ok().and_then(|f| f.path()) {
            open(&u, path);
        }
    });
}

/// Reads and analyses the wallpaper off the main thread so the window never freezes.
fn open(ui: &Rc<Ui>, path: PathBuf) {
    ui.say("Reading the wallpaper…");
    let u = ui.clone();
    glib::spawn_future_local(async move {
        let work = gio::spawn_blocking(move || -> Result<Loaded, String> {
            let bytes = read_file(&path, Limits::default()).map_err(|e| e.to_string())?;
            let (pixels, thumb) =
                load(&bytes, Limits::default(), THUMB_SIDE).map_err(|e| e.to_string())?;
            let file_name = path.file_name().map(|n| n.to_string_lossy().into_owned());
            Ok(Loaded {
                file_name: file_name.unwrap_or_default(),
                bytes,
                pixels,
                thumb,
            })
        });
        match work.await {
            Ok(Ok(loaded)) => {
                let t = &loaded.thumb;
                let texture = gdk::MemoryTexture::new(
                    t.width as i32,
                    t.height as i32,
                    gdk::MemoryFormat::R8g8b8a8,
                    &glib::Bytes::from(&t.rgba),
                    t.width as usize * 4,
                );
                u.picture.set_paintable(Some(&texture));
                {
                    let mut s = u.state.borrow_mut();
                    s.file_name = loaded.file_name;
                    s.bytes = loaded.bytes;
                    s.pixels = loaded.pixels;
                }
                u.controls.set_sensitive(true);
                recompute(&u);
            }
            Ok(Err(e)) => u.say(&format!("Could not use that image: {e}")),
            Err(_) => u.say("Something went wrong while reading the image."),
        }
    });
}

/// Rebuilds the palette for the chosen mode on a worker thread.
fn recompute(ui: &Rc<Ui>) {
    let (pixels, mode, generation) = {
        let mut s = ui.state.borrow_mut();
        s.generation += 1;
        (s.pixels.clone(), s.mode, s.generation)
    };
    if pixels.is_empty() {
        return;
    }
    let u = ui.clone();
    glib::spawn_future_local(async move {
        let work = gio::spawn_blocking(move || {
            let mode = mode.unwrap_or_else(|| Mode::detect(&pixels));
            Palette::from_pixels(&pixels, mode)
        });
        let Ok(Some(palette)) = work.await else {
            return;
        };
        if u.state.borrow().generation != generation {
            return; // a newer choice is already on its way
        }
        // Put the sliders where the accent is, without that counting as a nudge
        let accent = palette.accent.to_oklch();
        u.syncing.set(true);
        u.hue.set_value(accent.h);
        u.chroma.set_value(accent.c);
        u.syncing.set(false);
        u.state.borrow_mut().base = Some(palette.clone());
        u.say(&describe(&palette));
        u.show(palette);
    });
}

fn describe(p: &Palette) -> String {
    let mode = if p.mode == Mode::Dark {
        "dark"
    } else {
        "light"
    };
    if p.synthesized.is_empty() {
        format!("A {mode} palette taken from the wallpaper.")
    } else {
        format!(
            "A {mode} palette. Invented because the wallpaper lacks them: {}.",
            p.synthesized.join(", ")
        )
    }
}

fn nudge(ui: &Rc<Ui>) {
    if ui.syncing.get() {
        return;
    }
    let Some(base) = ui.state.borrow().base.clone() else {
        return;
    };
    ui.show(base.with_accent(ui.hue.value(), ui.chroma.value()));
}

fn save_theme(ui: &Rc<Ui>) {
    let name = match ThemeName::parse(ui.name.text().as_str()) {
        Ok(n) => n,
        Err(e) => return ui.say(&e.to_string()),
    };
    let Some(palette) = ui.shown.borrow().clone() else {
        return ui.say("Choose a wallpaper first.");
    };
    let Some(root) = hypora::config_root() else {
        return ui.say("HOME is not set.");
    };
    let (bytes, file_name) = {
        let s = ui.state.borrow();
        (s.bytes.clone(), s.file_name.clone())
    };
    let (apply, force) = (ui.apply.is_active(), ui.force.is_active());
    ui.say("Saving…");

    let u = ui.clone();
    glib::spawn_future_local(async move {
        let work = gio::spawn_blocking(move || -> Result<String, String> {
            let req = Request {
                name: &name,
                palette: &palette,
                wallpaper: Some(Wallpaper {
                    file_name: &file_name,
                    bytes: &bytes,
                }),
                base: None,
                force,
            };
            let dir = hypora::install(&root, &req).map_err(|e| e.to_string())?;
            let mut msg = format!("Saved {}.", dir.display());
            if apply {
                match hypora::apply(&hypora::theme_program(), &root, &name) {
                    Applied::Done { previous } => {
                        msg.push_str(&format!(" Switched to {}.", name.as_str()));
                        if let Some(p) = previous.filter(|p| *p != name) {
                            msg.push_str(&format!(" To go back: hypora-theme {}.", p.as_str()));
                        }
                        if let Some(cmd) =
                            hypora::boot_screen_hint(&root, Path::new(hypora::BOOT_THEME))
                        {
                            msg.push_str(&format!(
                                " The boot screen is still the old theme; run: {cmd}"
                            ));
                        }
                    }
                    Applied::FellBack { previous, error } => {
                        return Err(format!(
                            "Could not apply it ({error}); switched back to {}.",
                            previous.as_str()
                        ));
                    }
                    Applied::Failed { error } => {
                        return Err(format!("Could not apply it: {error}"));
                    }
                }
            }
            Ok(msg)
        });
        match work.await {
            Ok(Ok(msg)) => u.say(&msg),
            Ok(Err(msg)) => u.say(&msg),
            Err(_) => u.say("Something went wrong while saving."),
        }
    });
}

/// Eight interface colours on top, then the 16 terminal colours in two rows.
fn draw_swatches(cr: &gtk::cairo::Context, w: i32, h: i32, palette: Option<&Palette>) {
    let Some(p) = palette else { return };
    let vars = variables(p);
    let (cell_w, cell_h) = (f64::from(w) / 8.0, f64::from(h) / 3.0);
    for (i, (_, c)) in vars.iter().take(24).enumerate() {
        let [r, g, b] = c.rgb();
        cr.set_source_rgb(
            f64::from(r) / 255.0,
            f64::from(g) / 255.0,
            f64::from(b) / 255.0,
        );
        cr.rectangle(
            (i % 8) as f64 * cell_w,
            (i / 8) as f64 * cell_h,
            cell_w,
            cell_h,
        );
        let _ = cr.fill();
    }
}
