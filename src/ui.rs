use crate::frames::{self, DefaultFrame};
use crate::{
    capture::{self, Result, Settings},
    picker::{self, Actions, FramePicker},
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

const FRAME_PREVIEW_SIDE: i32 = 336;

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
    frame_picker: FramePicker,
    default_frame: RefCell<DefaultFrame>,
    default_style: RefCell<Option<frames::FrameStyle>>,
    default_button: gtk::Button,
    tool: Cell<Tool>,
    buttons: Vec<(Tool, gtk::Button)>,
    status: gtk::Label,
    color: ColorPick,
    stash_color: Cell<[f32; 4]>,
    width: gtk::SpinButton,
    text: gtk::Entry,
    doc: RefCell<Option<Document>>,
    zoom: Cell<f64>,
    pan: Cell<Point>,
    pan_base: Cell<Option<Point>>,
    pointer: Cell<Point>,
    space: Cell<bool>,
    zoom_label: gtk::Label,
    gradient_open: Cell<bool>,
    palettes: RefCell<Vec<frames::Palette>>,
    palette_row: RefCell<Option<(gtk::DropDown, gtk::MenuButton, gtk::Button)>>,
    palette_sync: Cell<bool>,
    export_scale: gtk::DropDown,
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
fn pass_scroll(w: &impl IsA<gtk::Widget>) {
    let wheel = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
    wheel.set_propagation_phase(gtk::PropagationPhase::Capture);
    wheel.connect_scroll(|wheel, _, dy| {
        let Some(view) = wheel
            .widget()
            .and_then(|w| w.ancestor(gtk::ScrolledWindow::static_type()))
            .and_downcast::<gtk::ScrolledWindow>()
        else {
            return glib::Propagation::Proceed;
        };
        let adjustment = view.vadjustment();
        let step = if wheel.unit() == gdk::ScrollUnit::Surface {
            dy
        } else {
            dy * adjustment.page_size().powf(2. / 3.)
        };
        adjustment.set_value(adjustment.value() + step);
        glib::Propagation::Stop
    });
    w.add_controller(wheel);
}
fn clear(b: &gtk::Box) {
    while let Some(child) = b.first_child() {
        b.remove(&child);
    }
}
type SliderSpec = (&'static str, f64, f64, fn(&mut Frame, f64));

fn slider_specs(frame: &Frame) -> Vec<SliderSpec> {
    if frame.preset.is_some() {
        return vec![
            ("Outer space", frame.extra_space, 256., |f, v| {
                f.extra_space = v
            }),
            ("Outer corners", frame.outer_radius, 512., |f, v| {
                f.outer_radius = v
            }),
        ];
    }
    let padding_label = if frame.adaptive_padding {
        "Max padding"
    } else {
        "Padding"
    };
    vec![
        (
            padding_label,
            frame.padding.iter().sum::<f64>() / 4.,
            512.,
            |f, v| f.padding = [v; 4],
        ),
        ("Image corners", frame.radius, 512., |f, v| f.radius = v),
        ("Outer corners", frame.outer_radius, 512., |f, v| {
            f.outer_radius = v
        }),
        ("Shadow", frame.shadow, 128., |f, v| {
            f.shadow = v;
            f.shadow_offset = (0., v / 2.);
        }),
    ]
}

fn connect_frame_picker(ui: &Rc<Ui>) {
    let choose = weak(ui, |ui, id: String| ui.choose_frame(&id));
    let delete = weak(ui, |ui, id: String| {
        let name = frames::entries()
            .ok()
            .and_then(|entries| entries.into_iter().find(|(entry, _)| entry == &id))
            .map_or_else(|| id.clone(), |(_, name)| name);
        ui.confirm_delete(&id, &name);
    });
    let owner = Rc::downgrade(ui);
    ui.frame_picker.set_actions(Actions {
        choose: Box::new(move |id| choose(id.to_owned())),
        delete: Box::new(move |id| delete(id.to_owned())),
        preview: Box::new(move |id| owner.upgrade()?.preview_texture(id)),
    });
}
fn weak<T>(ui: &Rc<Ui>, f: impl Fn(&Rc<Ui>, T) + 'static) -> impl Fn(T) + 'static {
    let weak = Rc::downgrade(ui);
    move |value| {
        if let Some(ui) = weak.upgrade() {
            f(&ui, value);
        }
    }
}
thread_local! {
    static SCALE: Cell<f64> = const { Cell::new(1.) };
}

fn scale() -> f64 {
    SCALE.with(Cell::get)
}

fn scaled(v: i32) -> i32 {
    (f64::from(v) * scale()).round() as i32
}

fn scaled_css(css: &str, factor: f64) -> String {
    let mut out = String::with_capacity(css.len());
    let bytes = css.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit()
            && (i == 0 || !bytes[i - 1].is_ascii_alphanumeric() && bytes[i - 1] != b'#')
        {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                i += 1;
            }
            if css[i..].starts_with("px") {
                let v: f64 = css[start..i].parse().unwrap_or(0.);
                out.push_str(&format!("{:.2}", v * factor));
            } else {
                out.push_str(&css[start..i]);
            }
        } else {
            let ch = css[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

fn icon(kind: &str) -> gtk::DrawingArea {
    let icon = gtk::DrawingArea::new();
    icon.set_content_width(scaled(20));
    icon.set_content_height(scaled(20));
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
        let factor = Settings::load().map_or(1., |s| s.ui_scale).clamp(0.5, 2.);
        SCALE.with(|c| c.set(factor));
        if (factor - 1.).abs() > 1e-3 {
            if let Some(settings) = gtk::Settings::default() {
                let dpi = settings.gtk_xft_dpi();
                let base = if dpi > 0 { dpi } else { 96 * 1024 };
                settings.set_gtk_xft_dpi((f64::from(base) * factor).round() as i32);
            }
        }
        let provider = gtk::CssProvider::new();
        provider.load_from_string(&scaled_css(include_str!("style.css"), factor));
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
        .default_width(scaled(1110))
        .default_height(scaled(730))
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
    let rail = vbox(2);
    rail.add_css_class("tool-rail");
    margin(&rail, 6);
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
            Tool::Redact => {
                "Drag to cover an area with an opaque block. Pick its color on the left."
            }
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
        .min_content_width(scaled(61))
        .child(&rail)
        .build();
    middle.append(&rail_scroll);
    let center = vbox(0);
    center.set_hexpand(true);
    let options = hbox(8);
    options.add_css_class("tool-options");
    margin(&options, 8);
    let color = ColorPick::new(
        gdk::RGBA::new(1., 0.36, 0.19, 1.),
        "Annotation color",
        |_| {},
    );
    options.append(&color.button);
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
    center.set_width_request(scaled(260));
    let framing = vbox(12);
    margin(&framing, 13);
    framing.add_css_class("framing");
    let default_button = button("Make default", "win.frame-default");
    default_button.set_hexpand(true);
    let sharex_menu = gio::Menu::new();
    sharex_menu.append(Some("Import ShareX preset…"), Some("win.sharex"));
    sharex_menu.append(Some("Try ShareX sample"), Some("win.sample"));
    let more = gtk::MenuButton::builder()
        .icon_name("view-more-symbolic")
        .tooltip_text("ShareX presets")
        .menu_model(&sharex_menu)
        .build();
    more.add_css_class("flat");
    more.update_property(&[gtk::accessible::Property::Label("ShareX presets")]);
    let title_row = hbox(6);
    let title = heading("Framing");
    title.set_hexpand(true);
    title_row.append(&title);
    title_row.append(&more);
    framing.append(&title_row);
    let modes = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .min_children_per_line(2)
        .max_children_per_line(2)
        .column_spacing(5)
        .row_spacing(5)
        .homogeneous(true)
        .build();
    let mut frame_buttons = vec![];
    for (title, name, mode) in [
        ("Original", "Original", Mode::Original),
        ("Shadow", "Window shadow", Mode::Shadow),
        ("Gradient", "Gradient", Mode::Gradient),
        ("Image", "Image", Mode::Image),
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
        let label = gtk::Label::new(Some(title));
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
    let frame_picker = FramePicker::new();
    framing.append(frame_picker.widget());
    let frame_controls = vbox(12);
    framing.append(&frame_controls);
    let inspector = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .min_content_width(scaled(218))
        .propagate_natural_width(false)
        .child(&framing)
        .build();
    inspector.set_vexpand(true);
    let default_bar = hbox(0);
    default_bar.add_css_class("default-bar");
    default_bar.append(&default_button);
    let side = vbox(0);
    side.add_css_class("inspector");
    side.append(&inspector);
    side.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    side.append(&default_bar);
    let split = gtk::Paned::new(gtk::Orientation::Horizontal);
    split.add_css_class("editor-split");
    split.set_hexpand(true);
    split.set_vexpand(true);
    split.set_wide_handle(true);
    split.set_start_child(Some(&center));
    split.set_end_child(Some(&side));
    split.set_resize_start_child(true);
    split.set_resize_end_child(false);
    split.set_shrink_start_child(false);
    split.set_shrink_end_child(false);
    split.add_tick_callback(|pane, _| {
        if pane.width() <= 1 {
            return glib::ControlFlow::Continue;
        }
        pane.set_position((pane.width() - 292).max(260));
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
    let export_scale = gtk::DropDown::from_strings(&["1×", "2×", "4×"]);
    export_scale.update_property(&[gtk::accessible::Property::Label("PNG export scale")]);
    export_scale.set_tooltip_text(Some(
        "Scale for Copy and Save PNG. Annotations are drawn at export resolution.",
    ));
    footer.append(&row("Export", &export_scale));
    let zoom_label = gtk::Label::new(Some("Fit"));
    zoom_label.add_css_class("dim-label");
    zoom_label.set_width_chars(5);
    zoom_label.set_tooltip_text(Some(
        "Ctrl+scroll or pinch to zoom, scroll or middle-drag to pan",
    ));
    footer.append(&zoom_label);
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
        frame_picker,
        default_frame: RefCell::new(frames::default_frame().unwrap_or_default()),
        default_style: RefCell::new(None),
        default_button,
        tool: Cell::new(Tool::Crop),
        buttons,
        status,
        color,
        stash_color: Cell::new([0.05, 0.05, 0.05, 1.]),
        width,
        text,
        doc: RefCell::new(None),
        zoom: Cell::new(1.),
        pan: Cell::new((0., 0.)),
        pan_base: Cell::new(None),
        pointer: Cell::new((0., 0.)),
        space: Cell::new(false),
        zoom_label,
        gradient_open: Cell::new(true),
        palettes: RefCell::new(frames::palettes()),
        palette_row: RefCell::new(None),
        palette_sync: Cell::new(false),
        export_scale,
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
                    if frame.pipeline.is_some() {
                        *frame = Frame::default();
                    }
                    frame.mode = mode;
                });
            }
        });
        button.connect_clicked(move |_| f(()));
    }
    connect_frame_picker(&ui);
    ui.reload_frames();
    action(&ui, app, "frame-default", &[], |ui| ui.save_default());
    let refresh_export = weak(&ui, |ui, ()| ui.refresh());
    ui.export_scale
        .connect_selected_notify(move |_| refresh_export(()));
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
    action(&ui, app, "fit", &["0"], |ui| ui.reset_view());
    action(&ui, app, "actual", &["1"], |ui| ui.actual_size());
    action(&ui, app, "zoom-in", &["plus", "equal", "KP_Add"], |ui| {
        ui.zoom_by(1.25, ui.canvas_center())
    });
    action(&ui, app, "zoom-out", &["minus", "KP_Subtract"], |ui| {
        ui.zoom_by(0.8, ui.canvas_center())
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
                match frames::load_default(&selection) {
                    Ok(frame) => {
                        self.default_style
                            .replace(Some(frames::FrameStyle::from(&frame)));
                        doc.edit.frame = frame;
                    }
                    Err(err) => self.message(&format!("Default frame unavailable: {err}")),
                }
            }
            Err(err) => self.message(&format!("Could not read default frame: {err}")),
        }
        *self.doc.borrow_mut() = Some(doc);
        self.zoom.set(1.);
        self.pan.set((0., 0.));
        self.rebuild_frame();
        self.refresh();
    }
    fn png_scale(&self) -> i32 {
        match self.export_scale.selected() {
            1 => 2,
            2 => 4,
            _ => 1,
        }
    }
    fn refresh(&self) {
        self.preview_cache.borrow_mut().take();
        let doc = self.doc.borrow();
        self.body
            .set_visible_child_name(if doc.is_some() { "image" } else { "empty" });
        self.options
            .set_visible(doc.is_some() && self.tool.get() != Tool::Crop);
        self.framing.set_sensitive(doc.is_some());
        self.export_scale.set_sensitive(doc.is_some());
        let selected = doc.as_ref().map(|d| frames::selection(&d.edit.frame));
        let same_frame = selected.as_ref() == Some(&*self.default_frame.borrow());
        let is_default = same_frame
            && (selected == Some(DefaultFrame::Original)
                || doc
                    .as_ref()
                    .map(|d| frames::FrameStyle::from(&d.edit.frame))
                    == *self.default_style.borrow());
        self.default_button.set_sensitive(
            selected.is_some() && (!is_default || selected == Some(DefaultFrame::Image)),
        );
        self.default_button.set_label(if is_default {
            "✓ Default for new screenshots"
        } else if same_frame {
            "Update default"
        } else {
            "Make default"
        });
        if is_default {
            self.default_button.add_css_class("default-chip-on");
        } else {
            self.default_button.remove_css_class("default-chip-on");
        }
        self.default_button
            .set_tooltip_text(Some(match (is_default, selected.as_ref()) {
                (true, Some(DefaultFrame::Image)) => "Save this background as the default again",
                (true, _) => "This frame and its settings are used for new screenshots",
                _ => "Use this frame and its settings for new screenshots",
            }));
        self.frame_picker.show_state(
            selected.as_ref().and_then(DefaultFrame::imported_id),
            self.default_frame.borrow().imported_id(),
        );
        if doc.is_none() {
            self.status.set_label("No image");
        }
        if let Some(d) = doc.as_ref() {
            let (_, _, w, h) = d.edit.geometry();
            let note = d
                .edit
                .frame
                .preset
                .as_ref()
                .map_or("", |(_, note)| note.as_str());
            let scale = self.png_scale();
            let w = i64::from(w) * i64::from(scale);
            let h = i64::from(h) * i64::from(scale);
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
        if (self.tool.get() == Tool::Redact) != (tool == Tool::Redact) {
            let [r, g, b, _] = self.stash_color.replace({
                let c = self.color.rgba();
                [c.red(), c.green(), c.blue(), 1.]
            });
            self.color.set_rgba(gdk::RGBA::new(r, g, b, 1.));
        }
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
            .set_visible(!matches!(tool, Tool::Blur | Tool::Crop));
        self.text.set_visible(tool == Tool::Text);
        self.width.set_visible(tool != Tool::Redact);
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
            let bytes = render::png_scaled(&d.image, &d.edit, self.png_scale())?;
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
    fn reload_frames(&self) {
        match frames::entries() {
            Ok(entries) => self.frame_picker.reload(entries),
            Err(error) => self.message(&error.to_string()),
        }
    }
    fn choose_frame(self: &Rc<Self>, id: &str) {
        match frames::load(&DefaultFrame::Imported(id.to_owned())) {
            Ok(frame) => self.change_frame(|current| *current = frame),
            Err(error) => self.message(&error.to_string()),
        }
    }
    fn preview_texture(&self, id: &str) -> Option<gdk::Texture> {
        let frame = frames::load(&DefaultFrame::Imported(id.to_owned())).ok()?;
        let document = self.doc.borrow();
        let document = document.as_ref()?;
        let edit = document.edit.with_frame(frame);
        let surface = render::thumbnail(&document.image, &edit, FRAME_PREVIEW_SIDE).ok()?;
        picker::texture(surface)
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
                ui.reload_frames();
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
        let result = (|| -> Result<(DefaultFrame, frames::FrameStyle)> {
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
            frames::set_default_style(frame)?;
            Ok((selection, frames::FrameStyle::from(frame)))
        })();
        match result {
            Ok((selection, style)) => {
                self.default_frame.replace(selection);
                self.default_style.replace(Some(style));
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
                self.reload_frames();
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
        self.sync_palette();
        self.refresh();
    }
    fn all_palettes(&self) -> Vec<frames::Palette> {
        let mut all: Vec<_> = [
            ("Lagoon", [0x49df9a, 0x32bbc2, 0x4078db]),
            ("Sunset", [0xffad72, 0xf36b87, 0x6c50b1]),
            ("Lavender", [0xffe0ee, 0xc5adff, 0x768ceb]),
            ("Slate", [0xb8d6d6, 0x536f85, 0x242b45]),
        ]
        .into_iter()
        .map(|(name, colors)| frames::Palette {
            name: name.to_owned(),
            stops: colors
                .iter()
                .enumerate()
                .map(|(i, &c)| (i as f64 / 2., hex(c)))
                .collect(),
        })
        .collect();
        all.extend(self.palettes.borrow().iter().cloned());
        all
    }
    fn stops(&self) -> Option<Vec<(f64, Color)>> {
        self.doc
            .borrow()
            .as_ref()
            .map(|d| d.edit.frame.stops.clone())
    }
    fn sync_palette(&self) {
        let Some((pick, save, remove)) = self.palette_row.borrow().clone() else {
            return;
        };
        let Some(stops) = self.stops() else {
            return;
        };
        let all = self.all_palettes();
        let builtin = all.len() - self.palettes.borrow().len();
        let current = all.iter().position(|p| same_stops(&p.stops, &stops));
        self.palette_sync.set(true);
        pick.set_selected(current.map_or(0, |i| i as u32 + 1));
        self.palette_sync.set(false);
        save.set_sensitive(current.is_none());
        remove.set_sensitive(current.is_some_and(|i| i >= builtin));
    }
    fn store_palettes(self: &Rc<Self>, all: Vec<frames::Palette>) -> bool {
        match frames::set_palettes(&all) {
            Ok(()) => {
                self.palettes.replace(all);
                self.rebuild_frame();
                true
            }
            Err(err) => {
                self.message(&err.to_string());
                false
            }
        }
    }
    fn save_palette(self: &Rc<Self>, name: &str) {
        let Some(stops) = self.stops() else {
            return;
        };
        let mut all = self.palettes.borrow().clone();
        let name = if name.is_empty() {
            format!("Palette {}", all.len() + 1)
        } else {
            name.to_owned()
        };
        let text = format!("Saved “{name}”.");
        all.push(frames::Palette { name, stops });
        if self.store_palettes(all) {
            self.message(&text);
        }
    }
    fn delete_palette(self: &Rc<Self>) {
        let Some(stops) = self.stops() else {
            return;
        };
        let mut all = self.palettes.borrow().clone();
        let Some(index) = all.iter().position(|p| same_stops(&p.stops, &stops)) else {
            return;
        };
        let removed = all.remove(index);
        if !self.store_palettes(all) {
            return;
        }
        let toast = adw::Toast::builder()
            .title(format!("Deleted “{}”.", removed.name))
            .button_label("Undo")
            .build();
        let undo = weak(self, move |ui, ()| {
            let mut all = ui.palettes.borrow().clone();
            all.insert(index.min(all.len()), removed.clone());
            ui.store_palettes(all);
        });
        toast.connect_button_clicked(move |_| undo(()));
        self.toast.add_toast(toast);
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
        reset.add_css_class("flat");
        reset.set_tooltip_text(Some("Reset this frame's settings"));
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
        let layout = hbox(6);
        let layout_title = heading("Layout");
        layout_title.set_hexpand(true);
        layout.append(&layout_title);
        layout.append(&reset);
        self.frame_controls.append(&layout);
        if frame.preset.is_none() {
            let adaptive = gtk::Switch::builder()
                .active(frame.adaptive_padding)
                .valign(gtk::Align::Center)
                .build();
            adaptive.update_property(&[gtk::accessible::Property::Label("Adaptive padding")]);
            let change = weak(self, |ui, active| {
                ui.change_frame(|frame| frame.adaptive_padding = active)
            });
            adaptive.connect_active_notify(move |toggle| change(toggle.is_active()));
            self.frame_controls
                .append(&row("Adaptive padding", &adaptive));
        }
        let sliders = gtk::Grid::builder()
            .column_spacing(8)
            .row_spacing(6)
            .build();
        for (index, (label, value, max, update)) in slider_specs(&frame).into_iter().enumerate() {
            self.slider(&sliders, index as i32, label, value, max, update);
        }
        self.frame_controls.append(&sliders);
        if frame.mode == Mode::Gradient {
            let body = vbox(9);
            body.set_margin_top(8);
            self.gradient_controls(&frame, &body);
            let expander = gtk::Expander::builder()
                .label_widget(&heading("Gradient"))
                .expanded(self.gradient_open.get())
                .child(&body)
                .build();
            let remember = weak(self, |ui, open: bool| ui.gradient_open.set(open));
            expander.connect_expanded_notify(move |e| remember(e.is_expanded()));
            self.frame_controls.append(&expander);
        }
        if frame.mode == Mode::Image {
            let choose = gtk::Button::with_label("Choose background image…");
            let f = weak(self, |ui, ()| ui.open(true));
            choose.connect_clicked(move |_| f(()));
            self.frame_controls.append(&choose);
        }
    }
    fn slider(
        self: &Rc<Self>,
        grid: &gtk::Grid,
        row: i32,
        label: &str,
        value: f64,
        max: f64,
        update: impl Fn(&mut Frame, f64) + 'static,
    ) {
        let name = gtk::Label::new(Some(label));
        name.set_xalign(0.);
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        grid.attach(&name, 0, row, 1, 1);
        let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0., max, 1.);
        scale.set_draw_value(false);
        scale.update_property(&[gtk::accessible::Property::Label(label)]);
        scale.set_value(value);
        scale.set_hexpand(true);
        pass_scroll(&scale);
        grid.attach(&scale, 1, row, 1, 1);
        let number = gtk::SpinButton::new(Some(&scale.adjustment()), 1., 0);
        number.add_css_class("slider-number");
        number.set_width_chars(3);
        number.set_numeric(true);
        number.update_property(&[gtk::accessible::Property::Label(label)]);
        pass_scroll(&number);
        grid.attach(&number, 2, row, 1, 1);
        let f = weak(self, move |ui, value| {
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
        let typing_done = gtk::EventControllerFocus::new();
        let done = weak(self, |ui, ()| ui.frame_editing.set(false));
        typing_done.connect_leave(move |_| done(()));
        number.add_controller(typing_done);
    }
    fn gradient_controls(self: &Rc<Self>, frame: &Frame, target: &gtk::Box) {
        let dropdown = gtk::DropDown::from_strings(&["Linear", "Radial", "Conic"]);
        dropdown.set_selected(match frame.gradient {
            Gradient::Linear => 0,
            Gradient::Radial => 1,
            Gradient::Conic => 2,
        });
        let kind = row("Type", &dropdown);
        let f = weak(self, |ui, i: usize| {
            ui.change_frame(|f| {
                f.gradient = [Gradient::Linear, Gradient::Radial, Gradient::Conic][i]
            })
        });
        dropdown.connect_selected_notify(move |d| f(d.selected() as usize));
        let dial = gtk::DrawingArea::builder()
            .content_width(76)
            .content_height(76)
            .build();
        dial.set_tooltip_text(Some("Drag to set gradient direction. Snaps to 45° steps."));
        let exact = gtk::SpinButton::with_range(0., 359., 1.);
        exact.set_value(frame.angle);
        pass_scroll(&exact);
        exact.update_property(&[gtk::accessible::Property::Label(
            "Gradient angle in degrees",
        )]);
        let direction = hbox(14);
        direction.append(&dial);
        let angle_box = vbox(6);
        angle_box.set_valign(gtk::Align::Center);
        angle_box.set_hexpand(true);
        angle_box.append(&heading("Angle"));
        exact.set_width_chars(5);
        angle_box.append(&exact);
        direction.append(&angle_box);
        let draw = weak(self, |ui, (c, w, h): (Context, i32, i32)| {
            let doc = ui.doc.borrow();
            let Some(d) = doc.as_ref() else {
                return;
            };
            let (x, y) = (w as f64 / 2., h as f64 / 2.);
            let a = d.edit.frame.angle.to_radians();
            let r = x.min(y) - 10.;
            c.set_source_rgba(0.48, 0.55, 0.51, 0.18);
            c.arc(x, y, r, 0., std::f64::consts::TAU);
            let _ = c.fill();
            c.set_source_rgba(0.48, 0.55, 0.51, 0.75);
            c.set_line_width(2.);
            c.arc(x, y, r, 0., std::f64::consts::TAU);
            let _ = c.stroke();
            for i in 0..8 {
                let t = f64::from(i) * std::f64::consts::FRAC_PI_4;
                let len = if i % 2 == 0 { 6. } else { 3.5 };
                c.move_to(x + (r - len) * t.sin(), y - (r - len) * t.cos());
                c.line_to(x + (r - 1.) * t.sin(), y - (r - 1.) * t.cos());
                let _ = c.stroke();
            }
            c.set_source_rgba(0.28, 0.68, 0.45, 0.9);
            c.set_line_width(3.);
            c.set_line_cap(cairo::LineCap::Round);
            c.move_to(x, y);
            c.line_to(x + (r - 8.) * a.sin(), y - (r - 8.) * a.cos());
            let _ = c.stroke();
            c.arc(x, y, 3., 0., std::f64::consts::TAU);
            let _ = c.fill();
            c.arc(
                x + r * a.sin(),
                y - r * a.cos(),
                9.,
                0.,
                std::f64::consts::TAU,
            );
            let _ = c.fill_preserve();
            c.set_source_rgba(1., 1., 1., 0.95);
            c.set_line_width(2.);
            let _ = c.stroke();
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
                if x.hypot(y) < 10. {
                    return;
                }
                let degrees = x.atan2(y).to_degrees().rem_euclid(360.);
                let nearest = (degrees / 45.).round() * 45.;
                let degrees = if (degrees - nearest).abs() <= 4. {
                    nearest
                } else {
                    degrees.round()
                };
                spin.set_value(degrees.rem_euclid(360.));
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
        let all = Rc::new(self.all_palettes());
        let names: Vec<&str> = std::iter::once("Custom")
            .chain(all.iter().map(|p| p.name.as_str()))
            .collect();
        let palette = gtk::DropDown::from_strings(&names);
        palette.set_hexpand(true);
        palette.set_tooltip_text(Some("Color palette"));
        palette.update_property(&[gtk::accessible::Property::Label("Color palette")]);
        let factory = gtk::SignalListItemFactory::new();
        factory.connect_setup(|_, item| {
            let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                return;
            };
            let chip = gtk::DrawingArea::builder()
                .content_width(30)
                .content_height(14)
                .valign(gtk::Align::Center)
                .build();
            let label = gtk::Label::new(None);
            label.set_xalign(0.);
            label.set_ellipsize(gtk::pango::EllipsizeMode::End);
            let entry = hbox(8);
            entry.append(&chip);
            entry.append(&label);
            item.set_child(Some(&entry));
        });
        let shown = all.clone();
        factory.connect_bind(move |_, item| {
            let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                return;
            };
            let Some(entry) = item.child() else {
                return;
            };
            if let Some(label) = entry.last_child().and_downcast::<gtk::Label>() {
                let name = item.item().and_downcast::<gtk::StringObject>();
                label.set_label(&name.map(|n| n.string()).unwrap_or_default());
            }
            if let Some(chip) = entry.first_child().and_downcast::<gtk::DrawingArea>() {
                let stops = (item.position() as usize)
                    .checked_sub(1)
                    .and_then(|i| shown.get(i))
                    .map(|p| p.stops.clone());
                chip.set_visible(stops.is_some());
                chip.set_draw_func(move |_, c, w, h| {
                    let gradient = cairo::LinearGradient::new(0., 0., f64::from(w), 0.);
                    for &(at, (r, g, b, a)) in stops.iter().flatten() {
                        gradient.add_color_stop_rgba(at, r, g, b, a);
                    }
                    round(c, 0., 0., f64::from(w), f64::from(h), 4.);
                    if c.set_source(&gradient).is_ok() {
                        let _ = c.fill();
                    }
                });
            }
        });
        palette.set_factory(Some(&factory));
        let f = weak(self, move |ui, index: usize| {
            if ui.palette_sync.get() {
                return;
            }
            match index.checked_sub(1).and_then(|i| all.get(i)) {
                Some(p) => ui.change_frame(|f| f.stops = p.stops.clone()),
                None => ui.sync_palette(),
            }
        });
        palette.connect_selected_notify(move |d| f(d.selected() as usize));
        let name = gtk::Entry::builder()
            .placeholder_text("Palette name")
            .max_length(40)
            .build();
        name.update_property(&[gtk::accessible::Property::Label("Palette name")]);
        let confirm = gtk::Button::with_label("Save");
        confirm.add_css_class("suggested-action");
        let form = hbox(6);
        margin(&form, 6);
        form.append(&name);
        form.append(&confirm);
        let popover = gtk::Popover::builder().child(&form).build();
        let save = gtk::MenuButton::builder()
            .icon_name("list-add-symbolic")
            .tooltip_text("Save these colors as a palette")
            .popover(&popover)
            .build();
        save.update_property(&[gtk::accessible::Property::Label("Save palette")]);
        let f = weak(self, |ui, text: glib::GString| ui.save_palette(text.trim()));
        let closing = popover.downgrade();
        let typed = name.downgrade();
        let submit = Rc::new(move || {
            let text = typed.upgrade().map(|e| e.text()).unwrap_or_default();
            if let Some(popover) = closing.upgrade() {
                popover.popdown();
            }
            f(text);
        });
        let go = submit.clone();
        name.connect_activate(move |_| go());
        confirm.connect_clicked(move |_| submit());
        let remove = gtk::Button::builder()
            .icon_name("edit-delete-symbolic")
            .tooltip_text("Delete this saved palette")
            .build();
        remove.update_property(&[gtk::accessible::Property::Label("Delete palette")]);
        let f = weak(self, |ui, ()| ui.delete_palette());
        remove.connect_clicked(move |_| f(()));
        let palette_line = hbox(6);
        palette_line.append(&palette);
        palette_line.append(&save);
        palette_line.append(&remove);
        target.append(&palette_line);
        self.palette_row.replace(Some((palette, save, remove)));
        self.sync_palette();
        let strip = gtk::DrawingArea::builder()
            .content_height(52)
            .hexpand(true)
            .build();
        strip.update_property(&[gtk::accessible::Property::Label("Gradient stop preview")]);
        let draw = weak(self, |ui, (cr, w, h): (Context, i32, i32)| {
            let doc = ui.doc.borrow();
            let Some(document) = doc.as_ref() else {
                return;
            };
            let stops = &document.edit.frame.stops;
            let x = STOP_MARGIN;
            let span = (f64::from(w) - 2. * x).max(1.);
            let gradient = cairo::LinearGradient::new(x, 0., x + span, 0.);
            for &(at, (r, g, b, a)) in stops {
                gradient.add_color_stop_rgba(at, r, g, b, a);
            }
            let _ = h;
            round(&cr, x - 6., 4., span + 12., 22., 11.);
            if cr.set_source(&gradient).is_ok() {
                let _ = cr.fill();
            }
            for &(at, (r, g, b, a)) in stops {
                let hx = x + at * span;
                cr.set_source_rgba(0.5, 0.5, 0.5, 0.8);
                cr.set_line_width(2.);
                cr.move_to(hx, 24.);
                cr.line_to(hx, STOP_HANDLE_Y - 8.);
                let _ = cr.stroke();
                cr.arc(hx, STOP_HANDLE_Y, 9., 0., std::f64::consts::TAU);
                cr.set_source_rgba(r, g, b, a);
                let _ = cr.fill_preserve();
                cr.set_source_rgba(1., 1., 1., 0.95);
                cr.set_line_width(2.);
                let _ = cr.stroke_preserve();
                cr.set_source_rgba(0., 0., 0., 0.35);
                cr.set_line_width(0.75);
                let _ = cr.stroke();
            }
        });
        strip.set_draw_func(move |_, cr, w, h| draw((cr.clone(), w, h)));
        target.append(&strip);
        let mut positions = Vec::new();
        for (index, &(position, color)) in frame.stops.iter().enumerate() {
            let r = hbox(6);
            let f = weak(self, move |ui, c: gdk::RGBA| {
                ui.frame_editing.set(false);
                ui.live_frame(|f| f.stops[index].1 = rgba(c));
            });
            let redraw = strip.downgrade();
            let picker = ColorPick::new(
                rgba_of(color),
                &format!("Stop {} color", index + 1),
                move |c| {
                    f(c);
                    if let Some(strip) = redraw.upgrade() {
                        strip.queue_draw();
                    }
                },
            );
            r.append(&picker.button);
            let at = gtk::SpinButton::with_range(0., 100., 0.5);
            at.set_value(position * 100.);
            at.set_hexpand(true);
            pass_scroll(&at);
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
            target.append(&r);
        }
        strip.set_tooltip_text(Some("Drag a marker to move a color stop"));
        let drag = gtk::GestureDrag::new();
        drag.set_button(1);
        let selected = Rc::new(Cell::new(None));
        let start = selected.clone();
        let begin = weak(self, move |ui, (width, x, y): (f64, f64, f64)| {
            if y < 20. {
                start.set(None);
                return;
            }
            let doc = ui.doc.borrow();
            let Some(document) = doc.as_ref() else {
                return;
            };
            let span = (width - 2. * STOP_MARGIN).max(1.);
            let index = document
                .edit
                .frame
                .stops
                .iter()
                .enumerate()
                .map(|(i, &(at, _))| (i, (STOP_MARGIN + at * span - x).abs()))
                .filter(|&(_, distance)| distance <= 14.)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(i, _)| i);
            start.set(index.map(|i| (i, document.edit.frame.stops[i].0, span)));
            ui.frame_editing.set(false);
        });
        drag.connect_drag_begin(move |gesture, x, y| {
            if let Some(widget) = gesture.widget() {
                begin((f64::from(widget.width()), x, y));
            }
        });
        drag.connect_drag_update(move |_, dx, _| {
            if let Some((i, at, span)) = selected.get() {
                if let Some(spin) = positions[i].upgrade() {
                    spin.set_value(((at + dx / span) * 100.).clamp(0., 100.).round());
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
        target.append(&controls);
        target.append(&kind);
        target.append(&direction);
    }
}
fn same_stops(a: &[(f64, Color)], b: &[(f64, Color)]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(&(p, c), &(q, d))| {
            [p - q, c.0 - d.0, c.1 - d.1, c.2 - d.2, c.3 - d.3]
                .iter()
                .all(|v| v.abs() < 1e-4)
        })
}
const STOP_MARGIN: f64 = 16.;
const STOP_HANDLE_Y: f64 = 38.;
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

const SWATCHES: [u32; 12] = [
    0xff5b30, 0xe5484d, 0xf5a524, 0xf7d046, 0x30a46c, 0x12a594, 0x3e63dd, 0x8e4ec6, 0xe93d82,
    0xffffff, 0x8b8d98, 0x0d0d0d,
];
fn rgba_of(c: Color) -> gdk::RGBA {
    gdk::RGBA::new(c.0 as f32, c.1 as f32, c.2 as f32, 1.)
}
fn color_text(c: gdk::RGBA) -> String {
    let byte = |v: f32| (v * 255.).round().clamp(0., 255.) as u8;
    format!(
        "#{:02X}{:02X}{:02X}",
        byte(c.red()),
        byte(c.green()),
        byte(c.blue())
    )
}
fn swatch(color: Rc<Cell<[f32; 4]>>, width: i32, height: i32) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::builder()
        .content_width(width)
        .content_height(height)
        .build();
    area.set_draw_func(move |_, c, w, h| {
        let [r, g, b, _] = color.get();
        let (w, h) = (f64::from(w), f64::from(h));
        round(c, 0., 0., w, h, 5.);
        c.set_source_rgb(f64::from(r), f64::from(g), f64::from(b));
        let _ = c.fill_preserve();
        c.set_source_rgba(0.5, 0.5, 0.5, 0.55);
        c.set_line_width(1.);
        let _ = c.stroke();
    });
    area
}
fn round(c: &Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let r = r.min(w / 2.).min(h / 2.);
    c.new_path();
    c.arc(x + w - r, y + r, r, -std::f64::consts::FRAC_PI_2, 0.);
    c.arc(x + w - r, y + h - r, r, 0., std::f64::consts::FRAC_PI_2);
    c.arc(
        x + r,
        y + h - r,
        r,
        std::f64::consts::FRAC_PI_2,
        std::f64::consts::PI,
    );
    c.arc(
        x + r,
        y + r,
        r,
        std::f64::consts::PI,
        std::f64::consts::PI * 1.5,
    );
    c.close_path();
}

#[derive(Clone)]
struct ColorPick {
    button: gtk::MenuButton,
    value: Rc<Cell<[f32; 4]>>,
    apply: Rc<dyn Fn(gdk::RGBA, bool)>,
}
impl ColorPick {
    fn new(initial: gdk::RGBA, label: &str, changed: impl Fn(gdk::RGBA) + 'static) -> Self {
        let value = Rc::new(Cell::new([
            initial.red(),
            initial.green(),
            initial.blue(),
            1.,
        ]));
        let face = swatch(value.clone(), 26, 16);
        let button = gtk::MenuButton::builder()
            .child(&face)
            .tooltip_text(label)
            .valign(gtk::Align::Center)
            .build();
        button.update_property(&[gtk::accessible::Property::Label(label)]);
        let popover = gtk::Popover::new();
        let content = vbox(10);
        margin(&content, 12);
        let grid = gtk::Grid::builder()
            .column_spacing(6)
            .row_spacing(6)
            .build();
        let entry = gtk::Entry::builder()
            .max_width_chars(9)
            .width_chars(9)
            .text(color_text(initial))
            .build();
        entry.update_property(&[gtk::accessible::Property::Label("Hex color")]);
        let apply: Rc<dyn Fn(gdk::RGBA, bool)> = {
            let value = value.clone();
            let face = face.downgrade();
            let entry = entry.downgrade();
            Rc::new(move |color: gdk::RGBA, notify: bool| {
                value.set([color.red(), color.green(), color.blue(), 1.]);
                if let Some(face) = face.upgrade() {
                    face.queue_draw();
                }
                if let Some(entry) = entry.upgrade() {
                    entry.set_text(&color_text(color));
                }
                if notify {
                    changed(color);
                }
            })
        };
        for (i, &c) in SWATCHES.iter().enumerate() {
            let chip = swatch(
                Rc::new(Cell::new({
                    let (r, g, b, _) = hex(c);
                    [r as f32, g as f32, b as f32, 1.]
                })),
                28,
                28,
            );
            let b = gtk::Button::builder().child(&chip).build();
            b.add_css_class("flat");
            b.update_property(&[gtk::accessible::Property::Label(&color_text(rgba_of(hex(
                c,
            ))))]);
            let apply = apply.clone();
            b.connect_clicked(move |_| apply(rgba_of(hex(c)), true));
            grid.attach(&b, (i % 6) as i32, (i / 6) as i32, 1, 1);
        }
        content.append(&grid);
        let apply_entry = apply.clone();
        entry.connect_activate(move |e| {
            if let Ok(color) = gdk::RGBA::parse(e.text().trim()) {
                apply_entry(color, true);
            }
        });
        content.append(&entry);
        let more = gtk::Button::with_label("More colors…");
        let current = value.clone();
        let apply_more = apply.clone();
        let anchor = button.downgrade();
        let popup = popover.downgrade();
        more.connect_clicked(move |_| {
            let (Some(anchor), Some(popup)) = (anchor.upgrade(), popup.upgrade()) else {
                return;
            };
            popup.popdown();
            let window = anchor.root().and_downcast::<gtk::Window>();
            let [r, g, b, _] = current.get();
            let apply = apply_more.clone();
            gtk::ColorDialog::builder()
                .with_alpha(false)
                .build()
                .choose_rgba(
                    window.as_ref(),
                    Some(&gdk::RGBA::new(r, g, b, 1.)),
                    gtk::gio::Cancellable::NONE,
                    move |result| {
                        if let Ok(color) = result {
                            apply(color, true);
                        }
                    },
                );
        });
        content.append(&more);
        popover.set_child(Some(&content));
        button.set_popover(Some(&popover));
        Self {
            button,
            value,
            apply,
        }
    }
    fn rgba(&self) -> gdk::RGBA {
        let [r, g, b, a] = self.value.get();
        gdk::RGBA::new(r, g, b, a)
    }
    fn set_rgba(&self, color: gdk::RGBA) {
        (self.apply)(color, false);
    }
    fn set_visible(&self, visible: bool) {
        self.button.set_visible(visible);
    }
}

const MAX_SCALE: f64 = 32.;
const KEEP_VISIBLE: f64 = 80.;

fn fit_scale(edit: &Edit, w: f64, h: f64) -> f64 {
    let (_, _, iw, ih) = edit.geometry();
    ((w - 36.) / iw as f64)
        .min((h - 36.) / ih as f64)
        .clamp(0.001, 1.)
}
fn transform(edit: &Edit, w: f64, h: f64, zoom: f64, pan: Point) -> (f64, f64, f64) {
    let (_, _, iw, ih) = edit.geometry();
    let scale = (fit_scale(edit, w, h) * zoom).clamp(0.001, MAX_SCALE);
    let pan = clamp_pan(pan, (w, h), (iw as f64 * scale, ih as f64 * scale));
    (
        (w - iw as f64 * scale) / 2. + pan.0,
        (h - ih as f64 * scale) / 2. + pan.1,
        scale,
    )
}
fn clamp_pan(pan: Point, view: Point, image: Point) -> Point {
    let limit = |pan: f64, view: f64, image: f64| {
        let reach = (image / 2. + view / 2. - KEEP_VISIBLE.min(image / 2.)).max(0.);
        pan.clamp(-reach, reach)
    };
    (limit(pan.0, view.0, image.0), limit(pan.1, view.1, image.1))
}
fn point(ui: &Ui, edit: &Edit, tool: Tool, (x, y): Point) -> Point {
    let (ox, oy, scale) = ui.view(edit);
    let (left, top, width, height) = edit.geometry();
    let r = edit.crop;
    let (before, after) = if render::is_free(tool) {
        (
            (left, top),
            (f64::from(width) - left - r.w, f64::from(height) - top - r.h),
        )
    } else {
        ((0., 0.), (0., 0.))
    };
    (
        r.x + ((x - ox) / scale - left)
            .round()
            .clamp(-before.0, r.w + after.0),
        r.y + ((y - oy) / scale - top)
            .round()
            .clamp(-before.1, r.h + after.1),
    )
}
impl Ui {
    fn view(&self, edit: &Edit) -> (f64, f64, f64) {
        transform(
            edit,
            self.canvas.width() as f64,
            self.canvas.height() as f64,
            self.zoom.get(),
            self.pan.get(),
        )
    }
    fn canvas_center(&self) -> Point {
        (
            self.canvas.width() as f64 / 2.,
            self.canvas.height() as f64 / 2.,
        )
    }
    fn view_changed(&self) {
        let label = match self.doc.borrow().as_ref() {
            Some(d) if self.zoom.get() > 1.0001 || self.zoom.get() < 0.9999 => {
                let (_, _, scale) = self.view(&d.edit);
                format!("{:.0}%", scale * 100.)
            }
            _ => "Fit".to_owned(),
        };
        self.zoom_label.set_label(&label);
        self.canvas.queue_draw();
    }
    fn reset_view(&self) {
        self.zoom.set(1.);
        self.pan.set((0., 0.));
        self.view_changed();
    }
    fn actual_size(&self) {
        let fit = match self.doc.borrow().as_ref() {
            Some(d) => fit_scale(
                &d.edit,
                self.canvas.width() as f64,
                self.canvas.height() as f64,
            ),
            None => return,
        };
        self.zoom.set(1. / fit);
        self.pan.set((0., 0.));
        self.view_changed();
    }
    fn zoom_by(&self, factor: f64, anchor: Point) {
        let (w, h) = (self.canvas.width() as f64, self.canvas.height() as f64);
        let result = {
            let doc = self.doc.borrow();
            let Some(d) = doc.as_ref() else {
                return;
            };
            let (ox, oy, scale) = self.view(&d.edit);
            let fit = fit_scale(&d.edit, w, h);
            let (_, _, iw, ih) = d.edit.geometry();
            let (iw, ih) = (iw as f64, ih as f64);
            let target = (scale * factor).clamp(fit, MAX_SCALE);
            let at = ((anchor.0 - ox) / scale, (anchor.1 - oy) / scale);
            let pan = (
                anchor.0 - at.0 * target - (w - iw * target) / 2.,
                anchor.1 - at.1 * target - (h - ih * target) / 2.,
            );
            (target / fit, if target <= fit { (0., 0.) } else { pan })
        };
        self.zoom.set(result.0);
        self.pan.set(result.1);
        self.view_changed();
    }
    fn pan_by(&self, dx: f64, dy: f64) {
        let pan = self.pan.get();
        self.pan.set((pan.0 + dx, pan.1 + dy));
        self.view_changed();
    }
}
fn wire_canvas(ui: &Rc<Ui>) {
    let draw = weak(ui, |ui: &Rc<Ui>, (c, _, _): (Context, i32, i32)| {
        if let Some(d) = ui.doc.borrow().as_ref() {
            let (x, y, scale) = ui.view(&d.edit);
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
            let cacheable = i64::from(pw) * i64::from(ph) <= 16_000_000;
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
        if ui.space.get() {
            ui.pan_base.set(Some(ui.pan.get()));
            return;
        }
        if ui.tool.get() == Tool::Text {
            return;
        }
        if let Some(doc) = ui.doc.borrow_mut().as_mut() {
            let p = point(ui, &doc.edit, ui.tool.get(), (x, y));
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
        if let Some(base) = ui.pan_base.get() {
            ui.pan.set((base.0 + x, base.1 + y));
            ui.view_changed();
            return;
        }
        if let Some(doc) = ui.doc.borrow_mut().as_mut() {
            let start = ui.start.get();
            let mut p = point(ui, &doc.edit, ui.tool.get(), (start.0 + x, start.1 + y));
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
        if ui.pan_base.take().is_some() {
            return;
        }
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
    let middle = gtk::GestureDrag::new();
    middle.set_button(2);
    let begin = weak(ui, |ui, ()| ui.pan_base.set(Some(ui.pan.get())));
    middle.connect_drag_begin(move |_, _, _| begin(()));
    let update = weak(ui, |ui, (x, y): Point| {
        if let Some(base) = ui.pan_base.get() {
            ui.pan.set((base.0 + x, base.1 + y));
            ui.view_changed();
        }
    });
    middle.connect_drag_update(move |_, x, y| update((x, y)));
    let end = weak(ui, |ui, ()| ui.pan_base.set(None));
    middle.connect_drag_end(move |_, _, _| end(()));
    ui.canvas.add_controller(middle);
    let motion = gtk::EventControllerMotion::new();
    let track = weak(ui, |ui, p: Point| ui.pointer.set(p));
    motion.connect_motion(move |_, x, y| track((x, y)));
    ui.canvas.add_controller(motion);
    let wheel = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::BOTH_AXES);
    let on_wheel = weak(ui, |ui, (dx, dy, mods): (f64, f64, gdk::ModifierType)| {
        if mods.contains(gdk::ModifierType::CONTROL_MASK) {
            ui.zoom_by((-dy * 0.15).exp(), ui.pointer.get());
        } else if ui.zoom.get() > 1.0001 {
            let (dx, dy) = if mods.contains(gdk::ModifierType::SHIFT_MASK) && dx == 0. {
                (dy, 0.)
            } else {
                (dx, dy)
            };
            ui.pan_by(-dx * 40., -dy * 40.);
        }
    });
    wheel.connect_scroll(move |c, dx, dy| {
        on_wheel((dx, dy, c.current_event_state()));
        glib::Propagation::Stop
    });
    ui.canvas.add_controller(wheel);
    let pinch = gtk::GestureZoom::new();
    let base = Rc::new(Cell::new(1.));
    let start = base.clone();
    let begin = weak(ui, move |ui, ()| start.set(ui.zoom.get()));
    pinch.connect_begin(move |_, _| begin(()));
    let pinch_scale = weak(ui, move |ui, (scale, center): (f64, Point)| {
        let current = ui.zoom.get();
        let wanted = base.get() * scale;
        if current > 0. {
            ui.zoom_by(wanted / current, center);
        }
    });
    pinch.connect_scale_changed(move |g, scale| {
        let center = g.bounding_box_center().unwrap_or((0., 0.));
        pinch_scale((scale, center));
    });
    ui.canvas.add_controller(pinch);
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
            let p = point(ui, &doc.edit, Tool::Text, (x, y));
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
    let released = w.clone();
    keys.connect_key_released(move |_, key, _, _| {
        if key == gdk::Key::space {
            if let Some(ui) = released.upgrade() {
                ui.space.set(false);
            }
        }
    });
    keys.connect_key_pressed(move |_, key, _, mods| {
        let Some(ui) = w.upgrade() else {
            return glib::Propagation::Proceed;
        };
        if key == gdk::Key::space {
            ui.space.set(true);
            return glib::Propagation::Stop;
        }
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
        .default_width(scaled(545))
        .default_height(scaled(660))
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
    let look = adw::PreferencesGroup::builder().title("Appearance").build();
    let zoom = adw::SpinRow::builder()
        .title("Interface scale")
        .subtitle("Percent. Takes effect the next time Still opens.")
        .adjustment(&gtk::Adjustment::new(
            (current.ui_scale * 100.).round(),
            50.,
            200.,
            5.,
            25.,
            0.,
        ))
        .build();
    look.add(&zoom);
    page.add(&look);
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
            ui_scale: zoom.value() / 100.,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(w: i32, h: i32) -> Edit {
        Edit::new(&ImageSurface::create(Format::ARgb32, w, h).unwrap())
    }

    #[test]
    fn css_pixel_sizes_follow_the_interface_scale() {
        let css = scaled_css(
            ".a { min-width: 34px; color: #4bd996; margin: 0 .5em; opacity: .15; }",
            1.5,
        );
        assert!(css.contains("min-width: 51.00px"));
        assert!(css.contains("#4bd996") && css.contains(".5em") && css.contains(".15"));
    }

    #[test]
    fn fit_view_is_centered_and_never_enlarges() {
        let (x, y, scale) = transform(&edit(100, 50), 400., 300., 1., (0., 0.));
        assert_eq!((x, y, scale), (150., 125., 1.));
        let (x, _, scale) = transform(&edit(1000, 500), 536., 400., 1., (0., 0.));
        assert!((scale - 0.5).abs() < 1e-9 && (x - 18.).abs() < 1e-9);
    }

    #[test]
    fn zoomed_view_scales_from_the_fit_size_and_pans() {
        let image = edit(1000, 500);
        let (_, _, fit) = transform(&image, 536., 400., 1., (0., 0.));
        let (x, _, scale) = transform(&image, 536., 400., 2., (10., 0.));
        assert!((scale - fit * 2.).abs() < 1e-9);
        assert!((x - ((536. - 1000.) / 2. + 10.)).abs() < 1e-9);
    }

    #[test]
    fn panning_cannot_lose_the_image() {
        let (px, py) = clamp_pan((9999., -9999.), (500., 400.), (1000., 800.));
        assert!(px < 1000. && py > -1000.);
        let origin = (500. - 1000.) / 2. + px;
        assert!(origin <= 500. - KEEP_VISIBLE + 1e-9);
        let (nx, _) = clamp_pan((-9999., 0.), (500., 400.), (1000., 800.));
        assert!((500. - 1000.) / 2. + nx + 1000. >= KEEP_VISIBLE - 1e-9);
    }
}
