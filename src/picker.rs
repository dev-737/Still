use gtk::{gdk, glib, prelude::*};
use std::{cell::RefCell, collections::HashMap, rc::Rc};

const TILE_WIDTH: i32 = 168;
const TILE_HEIGHT: i32 = 104;
const POPOVER_WIDTH: i32 = 392;
const POPOVER_HEIGHT: i32 = 420;
const POPOVER_MIN_HEIGHT: i32 = 160;
const POPOVER_CHROME: f32 = 150.;
const PREVIEW_POLL: std::time::Duration = std::time::Duration::from_millis(120);
const TITLE: &str = "Custom frames";
const BUTTON_THUMB_WIDTH: i32 = 56;
const BUTTON_THUMB_HEIGHT: i32 = 36;

type PreviewRenderer = Box<dyn Fn(&str) -> Option<gdk::Texture>>;

pub struct Actions {
    pub choose: Box<dyn Fn(&str)>,
    pub delete: Box<dyn Fn(&str)>,
    pub preview: PreviewRenderer,
}

#[derive(Default)]
struct Shared {
    names: RefCell<HashMap<String, String>>,
    previews: RefCell<HashMap<String, gdk::Texture>>,
    selected: RefCell<Option<String>>,
    default: RefCell<Option<String>>,
    actions: RefCell<Option<Rc<Actions>>>,
}

impl Shared {
    fn name_of(&self, id: &str) -> String {
        self.names
            .borrow()
            .get(id)
            .cloned()
            .unwrap_or_else(|| id.to_owned())
    }

    fn actions(&self) -> Option<Rc<Actions>> {
        self.actions.borrow().clone()
    }

    fn render_preview(&self, id: &str) -> Option<gdk::Texture> {
        let actions = self.actions()?;
        (actions.preview)(id)
    }
}

struct Tile {
    root: gtk::Box,
    picture: gtk::Picture,
    label: gtk::Label,
    delete: gtk::Button,
    default_badge: gtk::Image,
}

impl Tile {
    fn build() -> Self {
        let picture = gtk::Picture::builder()
            .content_fit(gtk::ContentFit::Contain)
            .can_shrink(true)
            .width_request(TILE_WIDTH)
            .height_request(TILE_HEIGHT)
            .build();
        picture.add_css_class("tile-preview");
        let delete = gtk::Button::builder()
            .icon_name("edit-delete-symbolic")
            .halign(gtk::Align::End)
            .valign(gtk::Align::Start)
            .build();
        delete.add_css_class("tile-delete");
        delete.add_css_class("circular");
        delete.add_css_class("osd");
        let default_badge = gtk::Image::from_icon_name("starred-symbolic");
        default_badge.set_halign(gtk::Align::Start);
        default_badge.set_valign(gtk::Align::Start);
        default_badge.add_css_class("tile-default-badge");
        let overlay = gtk::Overlay::new();
        overlay.set_child(Some(&picture));
        overlay.add_overlay(&delete);
        overlay.add_overlay(&default_badge);
        let label = gtk::Label::new(None);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.add_css_class("tile-name");
        let root = gtk::Box::new(gtk::Orientation::Vertical, 4);
        root.add_css_class("frame-tile");
        root.append(&overlay);
        root.append(&label);
        Self {
            root,
            picture,
            label,
            delete,
            default_badge,
        }
    }

    fn of(item: &gtk::ListItem) -> Option<Self> {
        let root = item.child()?.downcast::<gtk::Box>().ok()?;
        let overlay = root.first_child()?.downcast::<gtk::Overlay>().ok()?;
        let label = overlay.next_sibling()?.downcast::<gtk::Label>().ok()?;
        let mut picture = None;
        let mut delete = None;
        let mut default_badge = None;
        let mut child = overlay.first_child();
        while let Some(widget) = child {
            picture = picture.or_else(|| widget.clone().downcast::<gtk::Picture>().ok());
            delete = delete.or_else(|| widget.clone().downcast::<gtk::Button>().ok());
            default_badge = default_badge.or_else(|| widget.clone().downcast::<gtk::Image>().ok());
            child = widget.next_sibling();
        }
        Some(Self {
            root,
            picture: picture?,
            label,
            delete: delete?,
            default_badge: default_badge?,
        })
    }
}

fn id_of_object(object: &glib::Object) -> Option<String> {
    object
        .downcast_ref::<gtk::StringObject>()
        .map(|id| id.string().to_string())
}

fn id_of_item(item: &gtk::ListItem) -> Option<String> {
    id_of_object(&item.item()?)
}

fn list_item(object: &glib::Object) -> &gtk::ListItem {
    object
        .downcast_ref::<gtk::ListItem>()
        .expect("grid view rows are list items")
}

pub fn texture(mut surface: cairo::ImageSurface) -> Option<gdk::Texture> {
    surface.flush();
    let (width, height, stride) = (surface.width(), surface.height(), surface.stride());
    let bytes = glib::Bytes::from(&surface.data().ok()?[..]);
    let format = if cfg!(target_endian = "little") {
        gdk::MemoryFormat::B8g8r8a8Premultiplied
    } else {
        gdk::MemoryFormat::A8r8g8b8Premultiplied
    };
    let texture = gdk::MemoryTexture::new(width, height, format, &bytes, stride as usize);
    Some(texture.upcast())
}

enum Viewing {
    Closed,
    ScrolledAway,
    Visible,
}

fn viewing(tile: &gtk::Widget) -> Viewing {
    let popover_open = tile
        .ancestor(gtk::Popover::static_type())
        .is_some_and(|popover| popover.is_visible());
    if !popover_open {
        return Viewing::Closed;
    }
    if tile.height() == 0 {
        return Viewing::ScrolledAway;
    }
    let Some(scroller) = tile.ancestor(gtk::ScrolledWindow::static_type()) else {
        return Viewing::Visible;
    };
    match tile.compute_bounds(&scroller) {
        Some(bounds)
            if bounds.y() < scroller.height() as f32 && bounds.y() + bounds.height() > 0. =>
        {
            Viewing::Visible
        }
        _ => Viewing::ScrolledAway,
    }
}

fn show_preview_when_visible(item: &gtk::ListItem, shared: &Rc<Shared>, id: String) {
    let item = item.downgrade();
    let shared = shared.clone();
    glib::timeout_add_local_full(PREVIEW_POLL, glib::Priority::LOW, move || {
        let Some(item) = item.upgrade() else {
            return glib::ControlFlow::Break;
        };
        let Some(tile) = Tile::of(&item) else {
            return glib::ControlFlow::Break;
        };
        if id_of_item(&item).as_deref() != Some(id.as_str()) {
            return glib::ControlFlow::Break;
        }
        match viewing(tile.root.upcast_ref()) {
            Viewing::Closed => return glib::ControlFlow::Break,
            Viewing::ScrolledAway => return glib::ControlFlow::Continue,
            Viewing::Visible => {}
        }
        let cached = shared.previews.borrow().get(&id).cloned();
        let preview = cached.or_else(|| {
            let rendered = shared.render_preview(&id)?;
            shared
                .previews
                .borrow_mut()
                .insert(id.clone(), rendered.clone());
            Some(rendered)
        });
        if let Some(preview) = preview {
            tile.picture.set_paintable(Some(&preview));
        }
        glib::ControlFlow::Break
    });
}

fn bind(item: &gtk::ListItem, shared: &Rc<Shared>) {
    let (Some(id), Some(tile)) = (id_of_item(item), Tile::of(item)) else {
        return;
    };
    let name = shared.name_of(&id);
    tile.label.set_text(&name);
    tile.label.set_tooltip_text(Some(&name));
    let is_selected = shared.selected.borrow().as_deref() == Some(id.as_str());
    if is_selected {
        tile.root.add_css_class("selected-frame");
    } else {
        tile.root.remove_css_class("selected-frame");
    }
    tile.default_badge
        .set_visible(shared.default.borrow().as_deref() == Some(id.as_str()));
    tile.delete
        .update_property(&[gtk::accessible::Property::Label(&format!("Delete {name}"))]);
    let cached = shared.previews.borrow().get(&id).cloned();
    match cached {
        Some(preview) => tile.picture.set_paintable(Some(&preview)),
        None => {
            tile.picture.set_paintable(None::<&gdk::Texture>);
            show_preview_when_visible(item, shared, id);
        }
    }
}

fn setup(item: &gtk::ListItem, shared: &Rc<Shared>) {
    let tile = Tile::build();
    let weak_item = item.downgrade();
    let shared = shared.clone();
    tile.delete.connect_clicked(move |_| {
        let id = weak_item.upgrade().and_then(|item| id_of_item(&item));
        if let (Some(id), Some(actions)) = (id, shared.actions()) {
            (actions.delete)(&id);
        }
    });
    item.set_child(Some(&tile.root));
}

struct Summary {
    root: gtk::Box,
    thumbnail: gtk::Picture,
    thumbnail_slot: gtk::ScrolledWindow,
    name: gtk::Label,
}

impl Summary {
    fn build() -> Self {
        let thumbnail = gtk::Picture::builder()
            .content_fit(gtk::ContentFit::Contain)
            .can_shrink(true)
            .build();
        thumbnail.add_css_class("tile-preview");
        let thumbnail_slot = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Never)
            .width_request(BUTTON_THUMB_WIDTH)
            .height_request(BUTTON_THUMB_HEIGHT)
            .valign(gtk::Align::Center)
            .child(&thumbnail)
            .visible(false)
            .build();
        let name = gtk::Label::builder()
            .label(TITLE)
            .hexpand(true)
            .xalign(0.)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        let arrow = gtk::Image::from_icon_name("pan-down-symbolic");
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        root.append(&thumbnail_slot);
        root.append(&name);
        root.append(&arrow);
        Self {
            root,
            thumbnail,
            thumbnail_slot,
            name,
        }
    }
}

pub struct FramePicker {
    button: gtk::MenuButton,
    summary: Summary,
    store: gtk::StringList,
    shared: Rc<Shared>,
}

struct Content {
    search: gtk::SearchEntry,
    scroller: gtk::ScrolledWindow,
}

fn available_height(anchor: &gtk::Widget) -> i32 {
    let room = anchor
        .root()
        .and_then(|root| {
            let bounds = anchor.compute_bounds(&root)?;
            let below = root.height() as f32 - (bounds.y() + bounds.height());
            Some(below.max(bounds.y()))
        })
        .unwrap_or(POPOVER_HEIGHT as f32 + POPOVER_CHROME);
    ((room - POPOVER_CHROME) as i32).clamp(POPOVER_MIN_HEIGHT, POPOVER_HEIGHT)
}

fn build_content(shared: &Rc<Shared>, store: &gtk::StringList, popover: &gtk::Popover) -> Content {
    let search = gtk::SearchEntry::builder()
        .placeholder_text("Search custom frames")
        .build();
    let filter = gtk::CustomFilter::new({
        let shared = shared.clone();
        let search = search.clone();
        move |object| {
            let query = search.text().to_lowercase();
            id_of_object(object)
                .is_some_and(|id| shared.name_of(&id).to_lowercase().contains(&query))
        }
    });
    search.connect_search_changed({
        let filter = filter.clone();
        move |_| filter.changed(gtk::FilterChange::Different)
    });
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup({
        let shared = shared.clone();
        move |_, object| setup(list_item(object), &shared)
    });
    factory.connect_bind({
        let shared = shared.clone();
        move |_, object| bind(list_item(object), &shared)
    });
    let visible = gtk::FilterListModel::new(Some(store.clone()), Some(filter));
    let grid = gtk::GridView::new(Some(gtk::NoSelection::new(Some(visible))), Some(factory));
    grid.set_min_columns(2);
    grid.set_max_columns(3);
    grid.set_single_click_activate(true);
    grid.add_css_class("frame-grid");
    grid.connect_activate({
        let shared = shared.clone();
        let popover = popover.clone();
        move |grid, position| {
            let id = grid
                .model()
                .and_then(|model| model.item(position))
                .and_then(|object| id_of_object(&object));
            popover.popdown();
            if let (Some(id), Some(actions)) = (id, shared.actions()) {
                (actions.choose)(&id);
            }
        }
    });
    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .min_content_width(POPOVER_WIDTH)
        .min_content_height(POPOVER_HEIGHT)
        .child(&grid)
        .build();
    let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
    content.append(&search);
    content.append(&scroller);
    popover.set_child(Some(&content));
    Content { search, scroller }
}

impl FramePicker {
    pub fn new() -> Self {
        let shared = Rc::new(Shared::default());
        let store = gtk::StringList::new(&[]);
        let popover = gtk::Popover::new();
        let summary = Summary::build();
        let button = gtk::MenuButton::builder()
            .child(&summary.root)
            .popover(&popover)
            .visible(false)
            .build();
        button.add_css_class("frame-preset");
        button.add_css_class("custom-frames-button");
        let content = RefCell::new(None::<Content>);
        popover.connect_show({
            let shared = shared.clone();
            let store = store.clone();
            move |popover| {
                let mut built = content.borrow_mut();
                let content = built.get_or_insert_with(|| build_content(&shared, &store, popover));
                if let Some(anchor) = popover.parent() {
                    content
                        .scroller
                        .set_min_content_height(available_height(&anchor));
                }
                shared.previews.borrow_mut().clear();
                content.search.set_text("");
                content.search.grab_focus();
                store.items_changed(0, store.n_items(), store.n_items());
            }
        });
        Self {
            button,
            summary,
            store,
            shared,
        }
    }

    pub fn widget(&self) -> &gtk::MenuButton {
        &self.button
    }

    pub fn set_actions(&self, actions: Actions) {
        self.shared.actions.replace(Some(Rc::new(actions)));
    }

    pub fn reload(&self, entries: Vec<(String, String)>) {
        let ids: Vec<&str> = entries.iter().map(|(id, _)| id.as_str()).collect();
        self.store.splice(0, self.store.n_items(), &ids);
        *self.shared.names.borrow_mut() = entries.into_iter().collect();
        self.button.set_visible(self.store.n_items() > 0);
        self.update_button();
    }

    pub fn show_state(&self, selected: Option<&str>, default: Option<&str>) {
        let unchanged = self.shared.selected.borrow().as_deref() == selected
            && self.shared.default.borrow().as_deref() == default;
        if unchanged {
            return;
        }
        self.shared.selected.replace(selected.map(str::to_owned));
        self.shared.default.replace(default.map(str::to_owned));
        self.update_button();
        self.store
            .items_changed(0, self.store.n_items(), self.store.n_items());
    }

    fn update_button(&self) {
        let selected = self.shared.selected.borrow().clone();
        let known = selected
            .as_deref()
            .filter(|id| self.shared.names.borrow().contains_key(*id));
        match known {
            Some(id) => {
                self.summary.name.set_text(&self.shared.name_of(id));
                self.summary.thumbnail_slot.set_visible(true);
                self.show_thumbnail(id);
            }
            None => {
                self.summary
                    .name
                    .set_text(&format!("{TITLE} · {}", self.store.n_items()));
                self.summary.thumbnail_slot.set_visible(false);
            }
        }
    }

    fn show_thumbnail(&self, id: &str) {
        let cached = self.shared.previews.borrow().get(id).cloned();
        if cached.is_some() {
            self.summary.thumbnail.set_paintable(cached.as_ref());
            return;
        }
        self.summary.thumbnail.set_paintable(None::<&gdk::Texture>);
        let shared = self.shared.clone();
        let thumbnail = self.summary.thumbnail.clone();
        let id = id.to_owned();
        glib::idle_add_local_full(glib::Priority::LOW, move || {
            if shared.selected.borrow().as_deref() != Some(id.as_str()) {
                return glib::ControlFlow::Break;
            }
            if let Some(rendered) = shared.render_preview(&id) {
                shared
                    .previews
                    .borrow_mut()
                    .insert(id.clone(), rendered.clone());
                thumbnail.set_paintable(Some(&rendered));
            }
            glib::ControlFlow::Break
        });
    }
}
