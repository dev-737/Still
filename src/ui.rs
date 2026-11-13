use crate::frames::{self, DefaultFrame};
use crate::{
    capture::{self, Result, Settings},
    render::{self, Color, Edit, Frame, Gradient, Mark, Mode, Point, Rect, Tool},
};
use adw::prelude::*;
use cairo::{Context, Format, ImageSurface};
use gtk::{gdk, gio, glib};
use std::{
    cell::{Cell, RefCell},
    io::Cursor,
    path::{Path, PathBuf},
    rc::Rc,
    sync::mpsc,
};

const TOOLS: [(Tool, &str, &str, &str); 9] = [
    (Tool::Crop, "Crop", "C", "crop"),
    (Tool::Pen, "Freehand", "P", "pen"),
    (Tool::Line, "Line", "L", "line"),
    (Tool::Arrow, "Arrow", "A", "arrow"),
    (Tool::Rectangle, "Rectangle", "U", "rect"),
    (Tool::Ellipse, "Ellipse", "O", "circle"),
    (Tool::Text, "Text", "T", "text"),
    (Tool::Blur, "Blur", "B", "blur"),
    (Tool::Redact, "Redact", "R", "redact"),
];

struct Document {
    image: ImageSurface,
    edit: Edit,
    past: Vec<Edit>,
    future: Vec<Edit>,
    draft: Option<Mark>,
}
impl Document {
    fn new(image: ImageSurface) -> Self {
        let edit = Edit::new(&image);
        Self {
            image,
            edit,
            past: vec![],
            future: vec![],
            draft: None,
        }
    }
    fn checkpoint(&mut self) {
        self.past.push(self.edit.clone());
        if self.past.len() > 80 {
            self.past.remove(0);
        }
        self.future.clear();
    }
}
struct Ui {
    window: adw::ApplicationWindow,
    toast: adw::ToastOverlay,
    canvas: gtk::DrawingArea,
    body: gtk::Stack,
    options: gtk::Box,
    framing: gtk::Box,
    frame_controls: gtk::Box,
    saved_frames: gtk::Box,
    saved_buttons: RefCell<Vec<(DefaultFrame, gtk::Button)>>,
    default_frame: RefCell<DefaultFrame>,
    default_button: gtk::Button,
    tool: Cell<Tool>,
    buttons: Vec<(Tool, gtk::Button)>,
    status: gtk::Label,
    color: gtk::ColorDialogButton,
    width: gtk::SpinButton,
    text: gtk::Entry,
    doc: RefCell<Option<Document>>,
    actual: Cell<bool>,
    start: Cell<Point>,
    frame_editing: Cell<bool>,
    preview_cache: RefCell<Option<(i32, i32, ImageSurface)>>,
}
fn vbox(spacing: i32) -> gtk::Box {
    gtk::Box::new(gtk::Orientation::Vertical, spacing)
}
fn hbox(spacing: i32) -> gtk::Box {
    gtk::Box::new(gtk::Orientation::Horizontal, spacing)
}
fn margin(w: &impl IsA<gtk::Widget>, value: i32) {
    w.set_margin_start(value);
    w.set_margin_end(value);
    w.set_margin_top(value);
    w.set_margin_bottom(value);
}
fn heading(text: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.set_xalign(0.);
    l.add_css_class("heading");
    l
}
fn row(label: &str, control: &impl IsA<gtk::Widget>) -> gtk::Box {
    let r = hbox(6);
    let l = gtk::Label::new(Some(label));
    l.set_hexpand(true);
    l.set_xalign(0.);
    r.append(&l);
    r.append(control);
    r
}
fn clear(b: &gtk::Box) {
    while let Some(child) = b.first_child() {
        b.remove(&child);
    }
}
fn weak<T>(ui: &Rc<Ui>, f: impl Fn(&Rc<Ui>, T) + 'static) -> impl Fn(T) + 'static {
    let weak = Rc::downgrade(ui);
    move |value| {
        if let Some(ui) = weak.upgrade() {
            f(&ui, value);
        }
    }
}
fn icon(kind: &str) -> gtk::DrawingArea {
    let icon = gtk::DrawingArea::new();
    icon.set_content_width(20);
    icon.set_content_height(20);
    let kind = kind.to_owned();
    icon.set_draw_func(move |widget, c, w, h| {
        let color = widget.color();
        c.set_source_rgba(
            color.red() as f64,
            color.green() as f64,
            color.blue() as f64,
            1.,
        );
        c.scale(w as f64 / 24., h as f64 / 24.);
        c.set_line_width(1.65);
        c.set_line_cap(cairo::LineCap::Round);
        c.set_line_join(cairo::LineJoin::Round);
        let lines = |points: &[(f64, f64)]| {
            c.move_to(points[0].0, points[0].1);
            for &(x, y) in &points[1..] {
                c.line_to(x, y);
            }
        };
        match kind.as_str() {
            "crop" => {
                lines(&[(6., 3.), (6., 18.), (21., 18.)]);
                lines(&[(3., 6.), (18., 6.), (18., 21.)]);
            }
            "pen" => {
                lines(&[
                    (4., 16.),
                    (15., 5.),
                    (19., 9.),
                    (8., 20.),
                    (4., 20.),
                    (4., 16.),
                ]);
                lines(&[(13., 7.), (17., 11.)]);
            }
            "line" => lines(&[(5., 12.), (19., 12.)]),
            "arrow" => {
                lines(&[(6., 18.), (18., 6.), (18., 14.)]);
                lines(&[(10., 6.), (18., 6.)]);
            }
            "rect" => c.rectangle(3., 6., 18., 12.),
            "circle" => c.arc(12., 12., 8., 0., std::f64::consts::TAU),
            "text" => {
                lines(&[(5., 8.), (5., 5.), (19., 5.), (19., 8.)]);
                lines(&[(12., 5.), (12., 19.)]);
                lines(&[(9., 19.), (15., 19.)]);
            }
            "blur" => {
                c.move_to(12., 3.);
                c.curve_to(9., 8., 5., 12., 5., 15.);
                c.curve_to(5., 23., 19., 23., 19., 15.);
                c.curve_to(19., 12., 15., 8., 12., 3.);
            }
            _ => {
                c.rectangle(4., 4., 16., 16.);
                let _ = c.fill();
                return;
            }
        }
        let _ = c.stroke();
    });
    icon
}

pub fn launch(bytes: Option<Vec<u8>>, path: Option<PathBuf>, preferences: bool) -> Result<()> {
    let app = adw::Application::builder()
        .application_id("io.github.still.Editor")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    let initial = RefCell::new(Some((bytes, path)));
    app.connect_activate(move |app| {
        if let Some(window) = app.active_window() {
            window.present();
            return;
        }
        let provider = gtk::CssProvider::new();
        provider.load_from_string(include_str!("style.css"));
        if let Some(display) = gdk::Display::default() {
            gtk::style_context_add_provider_for_display(
                &display,
                &provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }
        let ui = build(app);
        if let Some((bytes, path)) = initial.borrow_mut().take() {
            let loaded = if let Some(data) = bytes {
                ImageSurface::create_from_png(&mut Cursor::new(data))
                    .map_err(Into::into)
                    .map(Some)
            } else if let Some(path) = path {
                open_image(&path).map(Some)
            } else {
                Ok(None)
            };
            match loaded {
                Ok(Some(image)) => ui.load(image),
                Ok(None) => {}
                Err(e) => ui.message(&e.to_string()),
            }
        }
        ui.window.present();
        if preferences {
            settings(&ui.window);
        }
    });
    let code = app.run_with_args(&["still"]);
    if code == glib::ExitCode::SUCCESS {
        Ok(())
    } else {
        Err("Could not start the GTK editor.".into())
    }
}

fn build(app: &adw::Application) -> Rc<Ui> {
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Still")
        .default_width(1110)
        .default_height(730)
        .build();
    let toast = adw::ToastOverlay::new();
    let view = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    header.set_title_widget(Some(&adw::WindowTitle::new("Still", "Screenshot editor")));
    let button = |label: &str, action: &str| {
        gtk::Button::builder()
            .label(label)
            .action_name(action)
            .build()
    };
    header.pack_start(&button("Open", "win.open"));
    header.pack_start(
        &gtk::Button::builder()
            .icon_name("camera-photo-symbolic")
            .tooltip_text("Capture a region")
            .action_name("win.capture")
            .build(),
    );
    header.pack_end(
        &gtk::Button::builder()
            .icon_name("emblem-system-symbolic")
            .tooltip_text("Settings")
            .action_name("win.settings")
            .build(),
    );
    let save = button("Save PNG", "win.save");
    save.add_css_class("suggested-action");
    header.pack_end(&save);
    header.pack_end(&button("Copy", "win.copy"));
    view.add_top_bar(&header);
    let middle = hbox(0);
    middle.set_vexpand(true);
    let rail = vbox(6);
    rail.add_css_class("tool-rail");
    margin(&rail, 8);
    let mut buttons = vec![];
    for (i, (tool, label, key, shape)) in TOOLS.iter().enumerate() {
        if i == 1 || i == 7 {
            let separator = gtk::Separator::new(gtk::Orientation::Horizontal);
            rail.append(&separator);
        }
        let description = match tool {
            Tool::Crop => "Drag around the area to keep.",
            Tool::Pen => "Drag to draw freehand.",
            Tool::Line | Tool::Arrow => "Drag to draw. Hold Shift to snap the angle.",
            Tool::Rectangle => "Drag to draw. Hold Shift for a square.",
            Tool::Ellipse => "Drag to draw. Hold Shift for a circle.",
            Tool::Text => "Type a label, then click to place it.",
            Tool::Blur => "Drag over the area to soften.",
            Tool::Redact => "Drag to cover an area with an opaque block.",
        };
        let b = gtk::Button::builder()
            .child(&icon(shape))
            .tooltip_text(format!("{label} ({key})\n{description}"))
            .build();
        b.update_property(&[gtk::accessible::Property::Label(label)]);
        b.add_css_class("flat");
        b.add_css_class("rail-button");
        rail.append(&b);
        buttons.push((*tool, b));
    }
    let rail_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .min_content_width(61)
        .child(&rail)
        .build();
    middle.append(&rail_scroll);
    let center = vbox(0);
    center.set_hexpand(true);
    let options = hbox(8);
    options.add_css_class("tool-options");
    margin(&options, 8);
    let color =
        gtk::ColorDialogButton::new(Some(gtk::ColorDialog::builder().with_alpha(false).build()));
    color.set_rgba(&gdk::RGBA::new(1., 0.36, 0.19, 1.));
    color.set_tooltip_text(Some("Annotation color"));
    options.append(&color);
    let width = gtk::SpinButton::with_range(1., 160., 1.);
    width.set_value(6.);
    width.set_tooltip_text(Some("Size in source pixels"));
    options.append(&width);
    let text = gtk::Entry::builder()
        .placeholder_text("Type a label, then click the image")
        .hexpand(true)
        .build();
    options.append(&text);
    center.append(&options);
    let body = gtk::Stack::builder().hexpand(true).vexpand(true).build();
    let empty = adw::StatusPage::builder()
        .icon_name("image-x-generic-symbolic")
        .title("Open a screenshot")
        .description("Open an image or capture a region to start editing.")
        .child(&button("Open image", "win.open"))
        .build();
    body.add_named(&empty, Some("empty"));
    let canvas = gtk::DrawingArea::builder()
        .hexpand(true)
        .vexpand(true)
        .focusable(true)
        .build();
    canvas.add_css_class("canvas");
    let scroll = gtk::ScrolledWindow::builder()
        .hexpand(true)
        .vexpand(true)
        .child(&canvas)
        .build();
    body.add_named(&scroll, Some("image"));
    center.append(&body);
    center.set_width_request(260);
    let framing = vbox(12);
    margin(&framing, 13);
    framing.add_css_class("framing");
    framing.append(&heading("Framing"));
    let modes = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .min_children_per_line(2)
        .max_children_per_line(2)
        .column_spacing(5)
        .row_spacing(5)
        .homogeneous(true)
        .build();
    let mut frame_buttons = vec![];
    for (name, mode) in [
        ("Original", Mode::Original),
        ("Window shadow", Mode::Shadow),
        ("Gradient", Mode::Gradient),
        ("Image", Mode::Image),
    ] {
        let tile = vbox(5);
        let swatch = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        swatch.add_css_class("frame-swatch");
        swatch.add_css_class(match mode {
            Mode::Original => "original-swatch",
            Mode::Shadow => "shadow-swatch",
            Mode::Gradient => "gradient-swatch",
            Mode::Image => "image-swatch",
        });
        let sample = gtk::Image::from_icon_name(if mode == Mode::Image {
            "image-x-generic-symbolic"
        } else {
            "application-x-executable-symbolic"
        });
        if mode != Mode::Image {
            sample.add_css_class("window-sample");
        }
        swatch.set_halign(gtk::Align::Fill);
        sample.set_hexpand(true);
        sample.set_halign(gtk::Align::Center);
        swatch.append(&sample);
        tile.append(&swatch);
        let label = gtk::Label::new(Some(name));
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        tile.append(&label);
        let b = gtk::Button::builder()
            .child(&tile)
            .hexpand(true)
            .tooltip_text(name)
            .build();
        b.add_css_class("frame-preset");
        b.update_property(&[gtk::accessible::Property::Label(name)]);
        modes.insert(&b, -1);
        frame_buttons.push((b, mode));
    }
    framing.append(&modes);
    let saved_frames = vbox(7);
    framing.append(&saved_frames);
    let default_button = button("Make default", "win.frame-default");
    default_button.set_tooltip_text(Some("Use this frame when opening new screenshots"));
    framing.append(&default_button);
    let frame_controls = vbox(9);
    framing.append(&frame_controls);
    framing.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    framing.append(&button("Import ShareX preset…", "win.sharex"));
    framing.append(&button("Try ShareX sample", "win.sample"));
    let inspector = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .min_content_width(218)
        .propagate_natural_width(false)
        .child(&framing)
        .build();
    inspector.add_css_class("inspector");
    let split = gtk::Paned::new(gtk::Orientation::Horizontal);
    split.add_css_class("editor-split");
    split.set_hexpand(true);
    split.set_vexpand(true);
    split.set_wide_handle(true);
    split.set_start_child(Some(&center));
    split.set_end_child(Some(&inspector));
    split.set_resize_start_child(true);
    split.set_resize_end_child(false);
    split.set_shrink_start_child(false);
    split.set_shrink_end_child(false);
    split.add_tick_callback(|pane, _| {
        if pane.width() <= 1 {
            return glib::ControlFlow::Continue;
        }
        pane.set_position((pane.width() - 258).max(260));
        glib::ControlFlow::Break
    });
    middle.append(&split);
    let footer = hbox(8);
    margin(&footer, 8);
    footer.append(
        &gtk::Button::builder()
            .icon_name("edit-undo-symbolic")
            .tooltip_text("Undo")
            .action_name("win.undo")
            .build(),
    );
    footer.append(
        &gtk::Button::builder()
            .icon_name("edit-redo-symbolic")
            .tooltip_text("Redo")
            .action_name("win.redo")
            .build(),
    );
    let status = gtk::Label::new(Some("No image"));
    status.set_xalign(0.);
    status.set_hexpand(true);
    status.set_ellipsize(gtk::pango::EllipsizeMode::End);
    footer.append(&status);
    footer.append(&button("Fit", "win.fit"));
    footer.append(&button("100%", "win.actual"));
    view.set_content(Some(&middle));
    view.add_bottom_bar(&footer);
    toast.set_child(Some(&view));
    window.set_content(Some(&toast));
    let ui = Rc::new(Ui {
        window,
        toast,
        canvas,
        body,
        options,
        framing,
        frame_controls,
        saved_frames,
        saved_buttons: RefCell::new(Vec::new()),
        default_frame: RefCell::new(frames::default_frame().unwrap_or_default()),
        default_button,
        tool: Cell::new(Tool::Crop),
        buttons,
        status,
        color,
        width,
        text,
        doc: RefCell::new(None),
        actual: Cell::new(false),
        start: Cell::new((0., 0.)),
        frame_editing: Cell::new(false),
        preview_cache: RefCell::new(None),
    });
    ui.window.connect_close_request({
        let ui = ui.clone();
        move |window| {
            window.set_content(gtk::Widget::NONE);
            let _ = &ui;
            glib::Propagation::Proceed
        }
    });
    for (tool, button) in &ui.buttons {
        let tool = *tool;
        let f = weak(&ui, move |ui, ()| ui.select(tool));
        button.connect_clicked(move |_| f(()));
    }
    for (button, mode) in frame_buttons {
        let f = weak(&ui, move |ui, ()| {
            let selected = ui
                .doc
                .borrow()
                .as_ref()
                .is_some_and(|d| d.edit.frame.mode == mode && d.edit.frame.preset.is_none());
            if !selected {
                ui.change_frame(|frame| {
                    if frame.preset.is_some()
                        || !frame.overlays.is_empty()
                        || frame.canvas_color.is_some()
                    {
                        *frame = Frame::default();
                    }
                    frame.mode = mode;
                });
            }
        });
        button.connect_clicked(move |_| f(()));
    }
    ui.rebuild_saved_frames();
    action(&ui, app, "frame-default", &[], |ui| ui.save_default());
    wire_canvas(&ui);
    action(&ui, app, "open", &["<Primary>o"], |ui| ui.open(false));
    action(&ui, app, "capture", &["<Primary><Shift>s"], Ui::capture);
    action(&ui, app, "settings", &["<Primary>comma"], |ui| {
        settings(&ui.window)
    });
    action(&ui, app, "save", &["<Primary>s"], Ui::save);
    action(&ui, app, "copy", &["<Primary>c"], |ui| ui.export(None));
    action(&ui, app, "undo", &["<Primary>z"], |ui| ui.history(false));
    action(
        &ui,
        app,
        "redo",
        &["<Primary><Shift>z", "<Primary>y"],
        |ui| ui.history(true),
    );
    action(&ui, app, "fit", &["0"], |ui| {
        ui.actual.set(false);
        ui.refresh();
    });
    action(&ui, app, "actual", &["1"], |ui| {
        ui.actual.set(true);
        ui.refresh();
    });
    action(&ui, app, "sharex", &[], Ui::import);
    action(&ui, app, "sample", &[], |ui| {
        ui.apply_saved_import(frames::save_preset(include_bytes!(
            "../assets/BackgroundGradient.sxie"
        )));
    });
    ui.select(Tool::Crop);
    ui.rebuild_frame();
    ui.refresh();
    ui
}
fn action(
    ui: &Rc<Ui>,
    app: &adw::Application,
    name: &str,
    keys: &[&str],
    f: impl Fn(&Rc<Ui>) + 'static,
) {
    let action = gio::SimpleAction::new(name, None);
    let call = weak(ui, move |ui, ()| f(ui));
    action.connect_activate(move |_, _| call(()));
    ui.window.add_action(&action);
    app.set_accels_for_action(&format!("win.{name}"), keys);
}

fn open_image(path: &Path) -> Result<ImageSurface> {
    let pix = gtk::gdk_pixbuf::Pixbuf::from_file(path)?;
    let surface = ImageSurface::create(Format::ARgb32, pix.width(), pix.height())?;
    let ctx = Context::new(&surface)?;
    gdk::prelude::GdkCairoContextExt::set_source_pixbuf(&ctx, &pix, 0., 0.);
    ctx.paint()?;
    Ok(surface)
}

impl Ui {
    fn message(&self, text: &str) {
        self.toast.add_toast(adw::Toast::new(text));
    }
    fn load(self: &Rc<Self>, image: ImageSurface) {
        let mut doc = Document::new(image);
        match frames::default_frame() {
            Ok(selection) => {
                self.default_frame.replace(selection.clone());
                match frames::load(&selection) {
                    Ok(frame) => doc.edit.frame = frame,
                    Err(err) => self.message(&format!("Default frame unavailable: {err}")),
                }
            }
            Err(err) => self.message(&format!("Could not read default frame: {err}")),
        }
        *self.doc.borrow_mut() = Some(doc);
        self.rebuild_frame();
        self.refresh();
    }
    fn refresh(&self) {
        self.preview_cache.borrow_mut().take();
        let doc = self.doc.borrow();
        self.body
            .set_visible_child_name(if doc.is_some() { "image" } else { "empty" });
        self.options.set_visible(
            doc.is_some() && self.tool.get() != Tool::Crop && self.tool.get() != Tool::Redact,
        );
        self.framing.set_sensitive(doc.is_some());
        let selected = doc.as_ref().map(|d| frames::selection(&d.edit.frame));
        let is_default = selected.as_ref() == Some(&*self.default_frame.borrow());
        self.default_button.set_sensitive(
            selected.is_some() && (!is_default || selected == Some(DefaultFrame::Image)),
        );
        self.default_button.set_label(if is_default {
            "Default frame"
        } else {
            "Make default"
        });
        for (selection, button) in self.saved_buttons.borrow().iter() {
            if selected.as_ref() == Some(selection) {
                button.add_css_class("selected-frame");
            } else {
                button.remove_css_class("selected-frame");
            }
            button.update_property(&[gtk::accessible::Property::Description(
                if selection == &*self.default_frame.borrow() {
                    "Default frame"
                } else {
                    ""
                },
            )]);
        }
        if doc.is_none() {
            self.status.set_label("No image");
        }
        if let Some(d) = doc.as_ref() {
            let (_, _, w, h) = d.edit.geometry();
            self.canvas
                .set_content_width(if self.actual.get() { w + 36 } else { 1 });
            self.canvas
                .set_content_height(if self.actual.get() { h + 36 } else { 1 });
            let note = d
                .edit
                .frame
                .preset
                .as_ref()
                .map_or("", |(_, note)| note.as_str());
            self.status.set_label(&if note.is_empty() {
                format!("{w} × {h} px · PNG")
            } else {
                format!("{w} × {h} px · {note}")
            });
            self.status
                .set_tooltip_text((!note.is_empty()).then_some(note));
        }
        for (name, enabled) in [
            ("undo", doc.as_ref().is_some_and(|d| !d.past.is_empty())),
            ("redo", doc.as_ref().is_some_and(|d| !d.future.is_empty())),
            ("save", doc.is_some()),
            ("copy", doc.is_some()),
        ] {
            if let Some(action) = self
                .window
                .lookup_action(name)
                .and_downcast::<gio::SimpleAction>()
            {
                action.set_enabled(enabled);
            }
        }
        self.canvas.queue_draw();
    }
    fn select(&self, tool: Tool) {
        self.tool.set(tool);
        if let Some(doc) = self.doc.borrow_mut().as_mut() {
            doc.draft = None;
        }
        for (which, b) in &self.buttons {
            if *which == tool {
                b.add_css_class("active-tool");
            } else {
                b.remove_css_class("active-tool");
            }
        }
        self.color
            .set_visible(!matches!(tool, Tool::Blur | Tool::Crop | Tool::Redact));
        self.text.set_visible(tool == Tool::Text);
        self.width.set_value(match tool {
            Tool::Text => 36.,
            Tool::Blur => 14.,
            _ => 6.,
        });
        self.width.set_tooltip_text(Some(match tool {
            Tool::Text => "Text size (px)",
            Tool::Blur => "Blur radius (px)",
            _ => "Stroke width (px)",
        }));
        self.refresh();
        if tool == Tool::Text && self.doc.borrow().is_some() {
            self.text.grab_focus();
        }
    }
    fn history(self: &Rc<Self>, redo: bool) {
        if let Some(d) = self.doc.borrow_mut().as_mut() {
            let (from, to) = if redo {
                (&mut d.future, &mut d.past)
            } else {
                (&mut d.past, &mut d.future)
            };
            if let Some(edit) = from.pop() {
                to.push(std::mem::replace(&mut d.edit, edit));
                d.draft = None;
            }
        }
        self.frame_editing.set(false);
        self.rebuild_frame();
        self.refresh();
    }
    fn open(self: &Rc<Self>, background: bool) {
        let filter = gtk::FileFilter::new();
        filter.add_pixbuf_formats();
        let dialog = gtk::FileDialog::builder()
            .title(if background {
                "Choose background image"
            } else {
                "Open screenshot"
            })
            .default_filter(&filter)
            .build();
        let f = weak(
            self,
            move |ui, result: std::result::Result<gio::File, glib::Error>| {
                let Ok(file) = result else {
                    return;
                };
                let Some(path) = file.path() else {
                    ui.message("Choose a local image file.");
                    return;
                };
                match open_image(&path) {
                    Ok(image) if background => ui.change_frame(|f| {
                        f.mode = Mode::Image;
                        f.background = Some(image);
                    }),
                    Ok(image) => ui.load(image),
                    Err(e) => ui.message(&e.to_string()),
                }
            },
        );
        dialog.open(Some(&self.window), gio::Cancellable::NONE, f);
    }
    fn save(self: &Rc<Self>) {
        let dialog = gtk::FileDialog::builder()
            .title("Save screenshot")
            .initial_name("Still.png")
            .build();
        let f = weak(
            self,
            |ui, result: std::result::Result<gio::File, glib::Error>| {
                if let Ok(file) = result {
                    if let Some(mut path) = file.path() {
                        if path.extension().is_none() {
                            path.set_extension("png");
                        }
                        ui.export(Some(path));
                    }
                }
            },
        );
        dialog.save(Some(&self.window), gio::Cancellable::NONE, f);
    }
    fn export(&self, path: Option<PathBuf>) {
        let result = (|| -> Result<()> {
            let doc = self.doc.borrow();
            let d = doc.as_ref().ok_or("Open an image first.")?;
            let bytes = render::png(&d.image, &d.edit)?;
            if let Some(path) = &path {
                capture::png_to(path, &bytes)
            } else {
                capture::copy(&bytes)
            }
        })();
        match result {
            Ok(()) => self.message(if path.is_some() {
                "Saved PNG"
            } else {
                "Copied PNG"
            }),
            Err(e) => self.message(&e.to_string()),
        }
    }
    fn capture(self: &Rc<Self>) {
        let (tx, rx) = mpsc::channel();
        self.window.set_visible(false);
        glib::timeout_add_local_once(std::time::Duration::from_millis(120), move || {
            std::thread::spawn(move || {
                let result = capture::run_actions()
                    .map(|(bytes, errors)| (bytes, errors.join(" · ")))
                    .map_err(|error| error.to_string());
                let _ = tx.send(result);
            });
        });
        let ui = self.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(30), move || {
            match rx.try_recv() {
                Ok(Ok((bytes, note))) => {
                    if let Some(png) = bytes {
                        match ImageSurface::create_from_png(&mut Cursor::new(png)) {
                            Ok(image) => ui.load(image),
                            Err(e) => ui.message(&e.to_string()),
                        }
                    }
                    if !note.is_empty() {
                        ui.message(&note);
                    }
                }
                Ok(Err(e)) => ui.message(&e),
                Err(mpsc::TryRecvError::Empty) => return glib::ControlFlow::Continue,
                Err(mpsc::TryRecvError::Disconnected) => {
                    ui.message("Capture stopped unexpectedly.")
                }
            }
            ui.window.present();
            glib::ControlFlow::Break
        });
    }
    fn import(self: &Rc<Self>) {
        let filter = gtk::FileFilter::new();
        filter.set_name(Some("ShareX image effect"));
        filter.add_suffix("sxie");
        let dialog = gtk::FileDialog::builder()
            .title("Import ShareX preset")
            .default_filter(&filter)
            .build();
        let f = weak(
            self,
            |ui, result: std::result::Result<gio::File, glib::Error>| {
                if let Ok(file) = result {
                    if let Some(path) = file.path() {
                        ui.apply_saved_import(frames::import(&path));
                    }
                }
            },
        );
        dialog.open(Some(&self.window), gio::Cancellable::NONE, f);
    }
    fn rebuild_saved_frames(self: &Rc<Self>) {
        self.saved_buttons.borrow_mut().clear();
        clear(&self.saved_frames);
        let ids = match frames::list() {
            Ok(ids) => ids,
            Err(error) => {
                self.message(&error.to_string());
                return;
            }
        };
        if ids.is_empty() {
            return;
        }
        self.saved_frames.append(&heading("Custom frames"));
        let sample = ImageSurface::create_from_png(&mut Cursor::new(include_bytes!(
            "../assets/frame-sample.png"
        )));
        for id in ids {
            let selection = DefaultFrame::Imported(id.clone());
            let Ok(frame) = frames::load(&selection) else {
                continue;
            };
            let note = &frame.preset.as_ref().unwrap().1;
            let name = note.split(" · ").next().unwrap_or(note);
            let tile = vbox(6);
            if let Ok(image) = &sample {
                let mut edit = Edit::new(image);
                edit.frame = frame.clone();
                if let Ok(bytes) = render::png(image, &edit) {
                    if let Ok(pixbuf) = gtk::gdk_pixbuf::Pixbuf::from_read(Cursor::new(bytes)) {
                        let texture = gdk::Texture::for_pixbuf(&pixbuf);
                        let picture = gtk::Picture::for_paintable(&texture);
                        picture.set_content_fit(gtk::ContentFit::Contain);
                        picture.set_size_request(100, 64);
                        picture.set_can_shrink(true);
                        tile.append(&picture);
                    }
                }
            }
            let label = gtk::Label::new(Some(name));
            label.set_ellipsize(gtk::pango::EllipsizeMode::End);
            tile.append(&label);
            let select = gtk::Button::builder()
                .child(&tile)
                .hexpand(true)
                .tooltip_text(note)
                .build();
            select.add_css_class("frame-preset");
            select.update_property(&[gtk::accessible::Property::Label(name)]);
            let remove = gtk::Button::builder()
                .icon_name("edit-delete-symbolic")
                .valign(gtk::Align::Center)
                .tooltip_text(format!("Delete {name}"))
                .build();
            remove.update_property(&[gtk::accessible::Property::Label(&format!("Delete {name}"))]);
            let row = hbox(5);
            row.append(&select);
            row.append(&remove);
            self.saved_frames.append(&row);
            self.saved_buttons
                .borrow_mut()
                .push((selection.clone(), select.clone()));
            let apply = weak(self, move |ui, ()| match frames::load(&selection) {
                Ok(frame) => ui.change_frame(|current| *current = frame),
                Err(err) => ui.message(&err.to_string()),
            });
            select.connect_clicked(move |_| apply(()));
            let name = name.to_owned();
            let delete = weak(self, move |ui, ()| ui.confirm_delete(&id, &name));
            remove.connect_clicked(move |_| delete(()));
        }
    }
    fn confirm_delete(self: &Rc<Self>, id: &str, name: &str) {
        let dialog = adw::MessageDialog::new(
            Some(&self.window),
            Some(&format!("Delete {name}?")),
            Some("This removes Still’s saved copy. The file you imported stays where it was."),
        );
        dialog.add_responses(&[("cancel", "Cancel"), ("delete", "Delete")]);
        dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");
        let id = id.to_owned();
        let remove = weak(self, move |ui, ()| match frames::delete(&id) {
            Ok(()) => {
                if let Some(doc) = ui.doc.borrow_mut().as_mut() {
                    let detach = |edit: &mut Edit| {
                        if edit
                            .frame
                            .preset
                            .as_ref()
                            .is_some_and(|(preset, _)| preset == &id)
                        {
                            edit.frame.preset = None;
                        }
                    };
                    detach(&mut doc.edit);
                    for edit in doc.past.iter_mut().chain(doc.future.iter_mut()) {
                        detach(edit);
                    }
                }
                ui.default_frame
                    .replace(frames::default_frame().unwrap_or_default());
                ui.rebuild_saved_frames();
                ui.rebuild_frame();
                ui.refresh();
                ui.message("Custom frame deleted.");
            }
            Err(err) => ui.message(&err.to_string()),
        });
        dialog.connect_response(Some("delete"), move |_, _| remove(()));
        dialog.present();
    }
    fn save_default(&self) {
        let result = (|| -> Result<DefaultFrame> {
            let document = self.doc.borrow();
            let frame = &document.as_ref().ok_or("Open an image first.")?.edit.frame;
            let selection = frames::selection(frame);
            if selection == DefaultFrame::Image {
                frames::save_background(
                    frame
                        .background
                        .as_ref()
                        .ok_or("Choose a background image first.")?,
                )?;
            }
            frames::set_default(&selection)?;
            Ok(selection)
        })();
        match result {
            Ok(selection) => {
                self.default_frame.replace(selection);
                self.refresh();
                self.message("Default frame updated.");
            }
            Err(err) => self.message(&err.to_string()),
        }
    }
    fn apply_saved_import(self: &Rc<Self>, result: Result<Frame>) {
        match result {
            Ok(frame) => {
                let note = frame.preset.as_ref().unwrap().1.clone();
                self.rebuild_saved_frames();
                self.change_frame(|current| *current = frame);
                self.message(&note);
            }
            Err(err) => self.message(&err.to_string()),
        }
    }
    fn change_frame(self: &Rc<Self>, f: impl FnOnce(&mut Frame)) {
        self.frame_editing.set(false);
        if let Some(d) = self.doc.borrow_mut().as_mut() {
            d.checkpoint();
            f(&mut d.edit.frame);
        }
        self.rebuild_frame();
        self.refresh();
    }
    fn live_frame(&self, f: impl FnOnce(&mut Frame)) {
        if let Some(d) = self.doc.borrow_mut().as_mut() {
            if !self.frame_editing.replace(true) {
                d.checkpoint();
            }
            f(&mut d.edit.frame);
        }
        self.refresh();
    }
    fn rebuild_frame(self: &Rc<Self>) {
        clear(&self.frame_controls);
        let Some(frame) = self.doc.borrow().as_ref().map(|d| d.edit.frame.clone()) else {
            return;
        };
        if let Some(modes) = self
            .framing
            .first_child()
            .and_then(|w| w.next_sibling())
            .and_downcast::<gtk::FlowBox>()
        {
            let mut current = modes.first_child();
            for mode in [Mode::Original, Mode::Shadow, Mode::Gradient, Mode::Image] {
                if let Some(child) = current {
                    if let Some(b) = child.first_child() {
                        if mode == frame.mode && frame.preset.is_none() {
                            b.add_css_class("selected-frame");
                        } else {
                            b.remove_css_class("selected-frame");
                        }
                    }
                    current = child.next_sibling();
                }
            }
        }
        if frame.mode == Mode::Original {
            return;
        }
        let reset = gtk::Button::with_label("Reset");
        let f = weak(self, |ui, ()| {
            let preset = ui
                .doc
                .borrow()
                .as_ref()
                .and_then(|doc| doc.edit.frame.preset.as_ref().map(|(id, _)| id.clone()));
            if let Some(id) = preset {
                match frames::load(&DefaultFrame::Imported(id)) {
                    Ok(frame) => ui.change_frame(|current| *current = frame),
                    Err(err) => ui.message(&err.to_string()),
                }
                return;
            }
            ui.change_frame(|old| {
                let mode = old.mode;
                *old = Frame {
                    mode,
                    ..Frame::default()
                };
            });
        });
        reset.connect_clicked(move |_| f(()));
        self.frame_controls.append(&row("Adjust frame", &reset));
        if frame.mode == Mode::Gradient {
            self.gradient_controls(&frame);
        }
        if frame.mode == Mode::Image {
            let choose = gtk::Button::with_label("Choose background image…");
            let f = weak(self, |ui, ()| ui.open(true));
            choose.connect_clicked(move |_| f(()));
            self.frame_controls.append(&choose);
        }
        for (label, value, max, update) in [
            (
                "Padding",
                frame.padding.iter().sum::<f64>() / 4.,
                512.,
                (|f: &mut Frame, v| f.padding = [v; 4]) as fn(&mut Frame, f64),
            ),
            ("Image corners", frame.radius, 512., |f, v| f.radius = v),
            ("Outer corners", frame.outer_radius, 512., |f, v| {
                f.outer_radius = v
            }),
            ("Shadow", frame.shadow, 128., |f, v| {
                f.shadow = v;
                f.shadow_offset = (0., v / 2.);
            }),
        ] {
            self.slider(label, value, max, update);
        }
    }
    fn slider(
        self: &Rc<Self>,
        label: &str,
        value: f64,
        max: f64,
        update: impl Fn(&mut Frame, f64) + 'static,
    ) {
        let line = vbox(2);
        let number = gtk::Label::new(Some(&format!("{value:.0}")));
        number.add_css_class("dim-label");
        line.append(&row(label, &number));
        let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0., max, 1.);
        scale.update_property(&[gtk::accessible::Property::Label(label)]);
        scale.set_value(value);
        scale.set_hexpand(true);
        line.append(&scale);
        self.frame_controls.append(&line);
        let f = weak(self, move |ui, value| {
            number.set_label(&format!("{value:.0}"));
            ui.live_frame(|frame| update(frame, value));
        });
        scale.connect_value_changed(move |scale| f(scale.value()));
        let focus = gtk::EventControllerFocus::new();
        let done = weak(self, |ui, ()| ui.frame_editing.set(false));
        focus.connect_leave(move |_| done(()));
        scale.add_controller(focus);
        let clicks = gtk::GestureClick::new();
        clicks.set_propagation_phase(gtk::PropagationPhase::Capture);
        let done = weak(self, |ui, ()| ui.frame_editing.set(false));
        clicks.connect_pressed(move |_, _, _, _| done(()));
        scale.add_controller(clicks);
    }
    fn gradient_controls(self: &Rc<Self>, frame: &Frame) {
        let dropdown = gtk::DropDown::from_strings(&["Linear", "Radial", "Conic"]);
        dropdown.set_selected(match frame.gradient {
            Gradient::Linear => 0,
            Gradient::Radial => 1,
            Gradient::Conic => 2,
        });
        self.frame_controls.append(&row("Type", &dropdown));
        let f = weak(self, |ui, i: usize| {
            ui.change_frame(|f| {
                f.gradient = [Gradient::Linear, Gradient::Radial, Gradient::Conic][i]
            })
        });
        dropdown.connect_selected_notify(move |d| f(d.selected() as usize));
        let dial = gtk::DrawingArea::builder()
            .content_width(48)
            .content_height(48)
            .build();
        dial.set_tooltip_text(Some("Drag to set gradient direction"));
        let exact = gtk::SpinButton::with_range(0., 359., 1.);
        exact.set_value(frame.angle);
        exact.update_property(&[gtk::accessible::Property::Label(
            "Gradient angle in degrees",
        )]);
        let direction = hbox(8);
        direction.append(&dial);
        direction.append(&row("Angle", &exact));
        self.frame_controls.append(&direction);
        let draw = weak(self, |ui, (c, w, h): (Context, i32, i32)| {
            let doc = ui.doc.borrow();
            let Some(d) = doc.as_ref() else {
                return;
            };
            let (x, y) = (w as f64 / 2., h as f64 / 2.);
            let a = d.edit.frame.angle.to_radians();
            let r = x.min(y) - 5.;
            c.set_source_rgba(0.48, 0.55, 0.51, 0.65);
            c.set_line_width(2.);
            c.arc(x, y, r, 0., std::f64::consts::TAU);
            let _ = c.stroke();
            c.set_source_rgb(0.28, 0.68, 0.45);
            c.arc(
                x + r * a.sin(),
                y - r * a.cos(),
                4.5,
                0.,
                std::f64::consts::TAU,
            );
            let _ = c.fill();
        });
        dial.set_draw_func(move |_, c, w, h| draw((c.clone(), w, h)));
        let dial_draw = dial.downgrade();
        let changed = weak(self, move |ui, value| {
            ui.live_frame(|f| f.angle = value);
            if let Some(dial) = dial_draw.upgrade() {
                dial.queue_draw();
            }
        });
        exact.connect_value_changed(move |value| changed(value.value()));
        let drag = gtk::GestureDrag::new();
        let origin = Rc::new(Cell::new((0., 0.)));
        let spin = exact.clone();
        let size = dial.downgrade();
        let set_angle = move |x: f64, y: f64| {
            if let Some(dial) = size.upgrade() {
                let x = x - dial.width() as f64 / 2.;
                let y = dial.height() as f64 / 2. - y;
                spin.set_value(x.atan2(y).to_degrees().round().rem_euclid(360.));
            }
        };
        let set_angle = Rc::new(set_angle);
        let start = origin.clone();
        let angle = set_angle.clone();
        let start_edit = weak(self, |ui, ()| ui.frame_editing.set(false));
        drag.connect_drag_begin(move |_, x, y| {
            start_edit(());
            start.set((x, y));
            angle(x, y);
        });
        drag.connect_drag_update(move |_, x, y| {
            let (a, b) = origin.get();
            set_angle(a + x, b + y);
        });
        let done = weak(self, |ui, ()| ui.frame_editing.set(false));
        drag.connect_drag_end(move |_, _, _| done(()));
        dial.add_controller(drag);
        let palette = gtk::DropDown::from_strings(&[
            "Choose a palette",
            "Lagoon",
            "Sunset",
            "Lavender",
            "Slate",
        ]);
        let f = weak(self, |ui, index: usize| {
            let presets = [
                [0x49df9a, 0x32bbc2, 0x4078db],
                [0xffad72, 0xf36b87, 0x6c50b1],
                [0xffe0ee, 0xc5adff, 0x768ceb],
                [0xb8d6d6, 0x536f85, 0x242b45],
            ];
            if let Some(colors) = index.checked_sub(1).and_then(|i| presets.get(i)) {
                ui.change_frame(|f| {
                    f.stops = colors
                        .iter()
                        .enumerate()
                        .map(|(i, &c)| (i as f64 / 2., hex(c)))
                        .collect()
                });
            }
        });
        palette.connect_selected_notify(move |d| f(d.selected() as usize));
        self.frame_controls.append(&row("Palette", &palette));
        self.frame_controls.append(&heading("Gradient stops"));
        let strip = gtk::DrawingArea::builder()
            .content_height(28)
            .hexpand(true)
            .build();
        strip.update_property(&[gtk::accessible::Property::Label("Gradient stop preview")]);
        let draw = weak(self, |ui, (cr, w, h): (Context, i32, i32)| {
            let doc = ui.doc.borrow();
            let Some(document) = doc.as_ref() else {
                return;
            };
            let stops = &document.edit.frame.stops;
            let x = 9.;
            let span = (f64::from(w) - 2. * x).max(1.);
            let y = f64::from(h) / 2.;
            let gradient = cairo::LinearGradient::new(x, 0., x + span, 0.);
            for &(at, (r, g, b, a)) in stops {
                gradient.add_color_stop_rgba(at, r, g, b, a);
            }
            cr.set_line_width(18.);
            cr.set_line_cap(cairo::LineCap::Round);
            cr.move_to(x, y);
            cr.line_to(x + span, y);
            if cr.set_source(&gradient).is_ok() {
                let _ = cr.stroke();
            }
            for &(at, (r, g, b, a)) in stops {
                cr.arc(x + at * span, y, 6., 0., std::f64::consts::TAU);
                cr.set_source_rgba(r, g, b, a);
                let _ = cr.fill_preserve();
                cr.set_source_rgba(1., 1., 1., 0.9);
                cr.set_line_width(1.5);
                let _ = cr.stroke();
            }
        });
        strip.set_draw_func(move |_, cr, w, h| draw((cr.clone(), w, h)));
        self.frame_controls.append(&strip);
        let mut positions = Vec::new();
        for (index, &(position, color)) in frame.stops.iter().enumerate() {
            let r = hbox(6);
            let picker = gtk::ColorDialogButton::new(Some(
                gtk::ColorDialog::builder().with_alpha(false).build(),
            ));
            picker.set_rgba(&gdk::RGBA::new(
                color.0 as f32,
                color.1 as f32,
                color.2 as f32,
                color.3 as f32,
            ));
            picker.update_property(&[gtk::accessible::Property::Label(&format!(
                "Stop {} color",
                index + 1
            ))]);
            r.append(&picker);
            let at = gtk::SpinButton::with_range(0., 100., 0.5);
            at.set_value(position * 100.);
            at.set_hexpand(true);
            at.update_property(&[gtk::accessible::Property::Label(&format!(
                "Stop {} position in percent",
                index + 1
            ))]);
            r.append(&at);
            let remove = gtk::Button::builder()
                .icon_name("edit-delete-symbolic")
                .tooltip_text("Remove stop")
                .sensitive(frame.stops.len() > 2)
                .build();
            r.append(&remove);
            let f = weak(self, move |ui, c: gdk::RGBA| {
                ui.frame_editing.set(false);
                ui.live_frame(|f| f.stops[index].1 = rgba(c));
            });
            let redraw = strip.downgrade();
            picker.connect_rgba_notify(move |p| {
                f(p.rgba());
                if let Some(strip) = redraw.upgrade() {
                    strip.queue_draw();
                }
            });
            let f = weak(self, move |ui, v| {
                ui.live_frame(|f| f.stops[index].0 = v);
            });
            let redraw = strip.downgrade();
            at.connect_value_changed(move |a| {
                f(a.value() / 100.);
                if let Some(strip) = redraw.upgrade() {
                    strip.queue_draw();
                }
            });
            positions.push(at.downgrade());
            let f = weak(self, move |ui, ()| {
                ui.change_frame(|f| {
                    f.stops.remove(index);
                })
            });
            remove.connect_clicked(move |_| f(()));
            self.frame_controls.append(&r);
        }
        strip.set_tooltip_text(Some("Drag a marker to move a color stop"));
        let drag = gtk::GestureDrag::new();
        drag.set_button(1);
        let selected = Rc::new(Cell::new(None));
        let start = selected.clone();
        let begin = weak(self, move |ui, (width, x): (f64, f64)| {
            let doc = ui.doc.borrow();
            let Some(document) = doc.as_ref() else {
                return;
            };
            let span = (width - 18.).max(1.);
            let index = document
                .edit
                .frame
                .stops
                .iter()
                .enumerate()
                .map(|(i, &(at, _))| (i, (9. + at * span - x).abs()))
                .filter(|&(_, distance)| distance <= 10.)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(i, _)| i);
            start.set(index.map(|i| (i, x, span)));
            ui.frame_editing.set(false);
        });
        drag.connect_drag_begin(move |gesture, x, _| {
            if let Some(widget) = gesture.widget() {
                begin((f64::from(widget.width()), x));
            }
        });
        drag.connect_drag_update(move |_, dx, _| {
            if let Some((i, x, span)) = selected.get() {
                if let Some(spin) = positions[i].upgrade() {
                    spin.set_value(((x + dx - 9.) / span * 100.).clamp(0., 100.));
                }
            }
        });
        let done = weak(self, |ui, ()| ui.frame_editing.set(false));
        drag.connect_drag_end(move |_, _, _| done(()));
        strip.add_controller(drag);
        let controls = hbox(4);
        let add = gtk::Button::with_label("Add stop");
        add.set_sensitive(frame.stops.len() < 5);
        let f = weak(self, |ui, ()| {
            ui.change_frame(|f| {
                let mut stops = f.stops.clone();
                stops.sort_by(|a, b| a.0.total_cmp(&b.0));
                if let Some(pair) = stops
                    .windows(2)
                    .max_by(|a, b| (a[1].0 - a[0].0).total_cmp(&(b[1].0 - b[0].0)))
                {
                    let (p, a) = pair[0];
                    let (q, b) = pair[1];
                    f.stops.push((
                        (p + q) / 2.,
                        ((a.0 + b.0) / 2., (a.1 + b.1) / 2., (a.2 + b.2) / 2., 1.),
                    ));
                }
            })
        });
        add.connect_clicked(move |_| f(()));
        controls.append(&add);
        let reverse = gtk::Button::with_label("Reverse");
        let f = weak(self, |ui, ()| {
            ui.change_frame(|f| {
                for stop in &mut f.stops {
                    stop.0 = 1. - stop.0;
                }
            })
        });
        reverse.connect_clicked(move |_| f(()));
        controls.append(&reverse);
        self.frame_controls.append(&controls);
    }
}
fn rgba(c: gdk::RGBA) -> Color {
    (
        c.red() as f64,
        c.green() as f64,
        c.blue() as f64,
        c.alpha() as f64,
    )
}
fn hex(c: u32) -> Color {
    (
        ((c >> 16) & 255) as f64 / 255.,
        ((c >> 8) & 255) as f64 / 255.,
        (c & 255) as f64 / 255.,
        1.,
    )
}

fn transform(edit: &Edit, w: f64, h: f64, actual: bool) -> (f64, f64, f64) {
    let (_, _, iw, ih) = edit.geometry();
    let scale = if actual {
        1.
    } else {
        ((w - 36.) / iw as f64)
            .min((h - 36.) / ih as f64)
            .clamp(0.001, 1.)
    };
    (
        (w - iw as f64 * scale) / 2.,
        (h - ih as f64 * scale) / 2.,
        scale,
    )
}
fn point(ui: &Ui, edit: &Edit, (x, y): Point) -> Point {
    let (ox, oy, scale) = transform(
        edit,
        ui.canvas.width() as f64,
        ui.canvas.height() as f64,
        ui.actual.get(),
    );
    let (left, top, _, _) = edit.geometry();
    let r = edit.crop;
    (
        r.x + ((x - ox) / scale - left).round().clamp(0., r.w),
        r.y + ((y - oy) / scale - top).round().clamp(0., r.h),
    )
}
fn wire_canvas(ui: &Rc<Ui>) {
    let draw = weak(ui, |ui: &Rc<Ui>, (c, w, h): (Context, i32, i32)| {
        if let Some(d) = ui.doc.borrow().as_ref() {
            let (x, y, scale) = transform(&d.edit, w as f64, h as f64, ui.actual.get());
            let (_, _, iw, ih) = d.edit.geometry();
            let _ = c.save();
            c.translate(x, y);
            c.scale(scale, scale);
            c.rectangle(0., 0., iw as f64, ih as f64);
            c.clip();
            let mut cache = ui.preview_cache.borrow_mut();
            let pixel_scale = ui.canvas.scale_factor() as f64;
            let pw = (iw as f64 * scale * pixel_scale).ceil().max(1.) as i32;
            let ph = (ih as f64 * scale * pixel_scale).ceil().max(1.) as i32;
            let cacheable = !ui.actual.get() && i64::from(pw) * i64::from(ph) <= 16_000_000;
            if cacheable && !cache.as_ref().is_some_and(|(w, h, _)| *w == pw && *h == ph) {
                *cache = (|| -> Result<_> {
                    let surface = ImageSurface::create(Format::ARgb32, pw, ph)?;
                    let render_context = Context::new(&surface)?;
                    render_context.scale(pw as f64 / iw as f64, ph as f64 / ih as f64);
                    render::paint(&render_context, &d.image, &d.edit, None)?;
                    Ok((pw, ph, surface))
                })()
                .ok();
            }
            if let Some((pw, ph, surface)) = cache.as_ref().filter(|_| cacheable) {
                let _ = c.save();
                c.scale(iw as f64 / *pw as f64, ih as f64 / *ph as f64);
                if c.set_source_surface(surface, 0., 0.).is_ok() {
                    let _ = c.paint();
                }
                let _ = c.restore();
                if let Some(mark) = d.draft.as_ref() {
                    let _ = render::paint_draft(&c, &d.image, &d.edit, mark);
                }
            } else {
                let _ = render::paint(&c, &d.image, &d.edit, d.draft.as_ref());
            }
            let _ = c.restore();
        }
    });
    ui.canvas
        .set_draw_func(move |_, c, w, h| draw((c.clone(), w, h)));
    let gesture = gtk::GestureDrag::new();
    gesture.set_button(1);
    let begin = weak(ui, |ui, (x, y): Point| {
        ui.canvas.grab_focus();
        ui.start.set((x, y));
        if ui.tool.get() == Tool::Text {
            return;
        }
        if let Some(doc) = ui.doc.borrow_mut().as_mut() {
            let p = point(ui, &doc.edit, (x, y));
            doc.draft = Some(Mark {
                tool: ui.tool.get(),
                points: vec![p],
                color: rgba(ui.color.rgba()),
                width: ui.width.value(),
                text: ui.text.text().to_string(),
            });
        }
    });
    gesture.connect_drag_begin(move |_, x, y| begin((x, y)));
    let update = weak(ui, |ui, (x, y, shift): (f64, f64, bool)| {
        if let Some(doc) = ui.doc.borrow_mut().as_mut() {
            let start = ui.start.get();
            let mut p = point(ui, &doc.edit, (start.0 + x, start.1 + y));
            if let Some(mark) = doc.draft.as_mut() {
                let a = mark.points[0];
                if shift {
                    let (dx, dy) = (p.0 - a.0, p.1 - a.1);
                    if matches!(mark.tool, Tool::Rectangle | Tool::Ellipse) {
                        let side = dx.abs().min(dy.abs());
                        p = (a.0 + dx.signum() * side, a.1 + dy.signum() * side);
                    }
                    if matches!(mark.tool, Tool::Line | Tool::Arrow) {
                        let step = std::f64::consts::FRAC_PI_4;
                        let angle = (dy.atan2(dx) / step).round() * step;
                        let length = dx.hypot(dy);
                        p = (a.0 + angle.cos() * length, a.1 + angle.sin() * length);
                    }
                }
                if mark.tool == Tool::Pen || mark.points.len() == 1 {
                    mark.points.push(p);
                } else {
                    mark.points[1] = p;
                }
            }
        }
        ui.canvas.queue_draw();
    });
    let update = Rc::new(update);
    let move_update = update.clone();
    gesture.connect_drag_update(move |g, x, y| {
        move_update((
            x,
            y,
            g.current_event_state()
                .contains(gdk::ModifierType::SHIFT_MASK),
        ))
    });
    let finish = weak(ui, |ui, ()| {
        if let Some(doc) = ui.doc.borrow_mut().as_mut() {
            if let Some(mark) = doc.draft.take() {
                let bounds = Rect::between(mark.points[0], *mark.points.last().unwrap());
                if mark.tool == Tool::Crop {
                    if bounds.w >= 2. && bounds.h >= 2. {
                        doc.checkpoint();
                        doc.edit.crop = bounds;
                    }
                } else if mark.tool != Tool::Text
                    && (bounds.w > 1.
                        || bounds.h > 1.
                        || (mark.tool == Tool::Pen && mark.points.len() > 3))
                {
                    doc.checkpoint();
                    doc.edit.marks.push(mark);
                }
            }
        }
        ui.refresh();
    });
    gesture.connect_drag_end(move |g, x, y| {
        update((
            x,
            y,
            g.current_event_state()
                .contains(gdk::ModifierType::SHIFT_MASK),
        ));
        finish(());
    });
    let cancel = weak(ui, |ui, ()| {
        if let Some(d) = ui.doc.borrow_mut().as_mut() {
            d.draft = None;
        }
        ui.canvas.queue_draw();
    });
    gesture.connect_cancel(move |_, _| cancel(()));
    ui.canvas.add_controller(gesture);
    let text_click = gtk::GestureClick::new();
    text_click.set_button(1);
    let click = weak(ui, |ui, (x, y): Point| {
        if ui.tool.get() != Tool::Text {
            return;
        }
        let text = ui.text.text().to_string();
        if text.trim().is_empty() {
            ui.text.grab_focus();
            return;
        }
        if let Some(doc) = ui.doc.borrow_mut().as_mut() {
            let p = point(ui, &doc.edit, (x, y));
            doc.checkpoint();
            doc.edit.marks.push(Mark {
                tool: Tool::Text,
                points: vec![p],
                color: rgba(ui.color.rgba()),
                width: ui.width.value(),
                text,
            });
        }
        ui.refresh();
    });
    text_click.connect_released(move |_, _, x, y| click((x, y)));
    ui.canvas.add_controller(text_click);
    let keys = gtk::EventControllerKey::new();
    let w = Rc::downgrade(ui);
    keys.connect_key_pressed(move |_, key, _, mods| {
        let Some(ui) = w.upgrade() else {
            return glib::Propagation::Proceed;
        };
        if mods.intersects(
            gdk::ModifierType::CONTROL_MASK
                | gdk::ModifierType::ALT_MASK
                | gdk::ModifierType::SUPER_MASK,
        ) {
            return glib::Propagation::Proceed;
        }
        if key == gdk::Key::Escape {
            if let Some(d) = ui.doc.borrow_mut().as_mut() {
                d.draft = None;
            }
            ui.canvas.queue_draw();
            return glib::Propagation::Stop;
        }
        if let Some(ch) = key.to_unicode() {
            if let Some((tool, _, _, _)) = TOOLS
                .iter()
                .find(|(_, _, k, _)| k.starts_with(ch.to_ascii_uppercase()))
            {
                ui.select(*tool);
                return glib::Propagation::Stop;
            }
        }
        glib::Propagation::Proceed
    });
    ui.canvas.add_controller(keys);
}

fn settings(parent: &adw::ApplicationWindow) {
    let current = match Settings::load() {
        Ok(s) => s,
        Err(e) => {
            capture::report(&format!("Settings: {e}"));
            Settings::default()
        }
    };
    let win = adw::PreferencesWindow::builder()
        .title("Still Settings")
        .transient_for(parent)
        .modal(true)
        .default_width(545)
        .default_height(620)
        .build();
    let page = adw::PreferencesPage::builder()
        .title("Capture")
        .icon_name("camera-photo-symbolic")
        .build();
    let actions = adw::PreferencesGroup::builder()
        .title("After region capture")
        .description("Choose any combination. Copy, Save, and the preview use your default frame.")
        .build();
    let toggle = |title: &str, sub: &str, active: bool| {
        let r = adw::ActionRow::builder().title(title).subtitle(sub).build();
        let s = gtk::Switch::builder()
            .active(active)
            .valign(gtk::Align::Center)
            .build();
        r.add_suffix(&s);
        r.set_activatable_widget(Some(&s));
        actions.add(&r);
        s
    };
    let copy = toggle(
        "Copy to clipboard",
        "Paste the capture immediately.",
        current.copy,
    );
    let save = toggle("Save to folder", "Write a PNG to disk.", current.save);
    let editor = toggle(
        "Open editor",
        "Open the capture for annotation and framing.",
        current.editor,
    );
    let preview = toggle(
        "Show clickable preview",
        "Open the screenshot by clicking its thumbnail when the editor does not open automatically.",
        current.preview,
    );
    page.add(&actions);
    let files = adw::PreferencesGroup::builder()
        .title("Saved screenshots")
        .description("Used when Save to folder is enabled.")
        .build();
    let folder = adw::EntryRow::builder()
        .title("Folder")
        .text(current.folder.to_string_lossy().as_ref())
        .build();
    let filename = adw::EntryRow::builder()
        .title("Filename pattern")
        .text(&current.filename)
        .build();
    files.add(&folder);
    files.add(&filename);
    page.add(&files);
    let apply_group = adw::PreferencesGroup::new();
    let line = adw::ActionRow::builder()
        .title("Apply capture settings")
        .build();
    let apply = gtk::Button::with_label("Save settings");
    apply.add_css_class("suggested-action");
    apply.set_valign(gtk::Align::Center);
    line.add_suffix(&apply);
    apply_group.add(&line);
    page.add(&apply_group);
    let weak = win.downgrade();
    apply.connect_clicked(move |_| {
        let data = Settings {
            copy: copy.is_active(),
            save: save.is_active(),
            editor: editor.is_active(),
            preview: preview.is_active(),
            folder: PathBuf::from(folder.text().as_str()),
            filename: filename.text().to_string(),
        };
        match data.store() {
            Ok(()) => {
                if let Some(w) = weak.upgrade() {
                    w.close();
                }
            }
            Err(e) => line.set_subtitle(&e.to_string()),
        }
    });
    win.add(&page);
    win.present();
}
