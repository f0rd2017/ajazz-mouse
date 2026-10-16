//! Главное окно: всё на одном экране. Слева мышь, справа две колонки компактных строк
//! с выпадающими списками. Любая правка сразу уходит в мышь.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use adw::prelude::*;
use gtk::{gdk, gio, glib};

use crate::device::{self, Cmd, Event};
use crate::i18n::tr;
use crate::keys;
use crate::macro_editor::MacroEditor;
use crate::mouse_view::MouseView;
use crate::protocol::*;

const BUTTONS_LIST: &[(usize, &str, &str)] = &[
    (SLOT_LEFT, "Левая", "Left"),
    (SLOT_RIGHT, "Правая", "Right"),
    (SLOT_MIDDLE, "Колесо", "Wheel click"),
    (SLOT_FORWARD, "Боковая 1", "Side 1"),
    (SLOT_BACK, "Боковая 2", "Side 2"),
    (SLOT_DPI, "DPI снизу", "DPI (bottom)"),
    (SLOT_WHEEL_UP, "Колесо вверх", "Wheel up"),
    (SLOT_WHEEL_DOWN, "Колесо вниз", "Wheel down"),
];

/// Макрос хранится по номеру кнопки 1…5 — у остальных его нет.
fn has_macro(slot: usize) -> bool {
    slot <= SLOT_FORWARD
}

fn button_title(slot: usize) -> &'static str {
    BUTTONS_LIST.iter().find(|b| b.0 == slot).map_or(tr("Кнопка", "Button"), |b| tr(b.1, b.2))
}

/// Пункт выпадающего списка кнопки.
#[derive(Clone, Copy, PartialEq)]
enum Choice {
    Value([u8; 3]),
    Shortcut,
    Macro,
}

#[derive(Default)]
struct Model {
    dpi: Option<Dpi>,
    light: Option<Light>,
    rate: Option<Rate>,
    buttons: Option<Buttons>,
}

/// Выпадающий список чисел; значение мыши, которого нет в списке, добавляется на лету.
struct ValueCombo {
    row: adw::ComboRow,
    values: RefCell<Vec<u32>>,
    label: fn(u32) -> String,
}

impl ValueCombo {
    fn new(title: &str, tooltip: &str, values: &[u32], label: fn(u32) -> String) -> Rc<Self> {
        let row = adw::ComboRow::builder().title(title).tooltip_text(tooltip).build();
        let this = Rc::new(Self { row, values: RefCell::new(values.to_vec()), label });
        this.rebuild();
        this
    }

    fn rebuild(&self) {
        let labels: Vec<String> = self.values.borrow().iter().map(|&v| (self.label)(v)).collect();
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        self.row.set_model(Some(&gtk::StringList::new(&refs)));
    }

    fn set(&self, v: u32) {
        let found = self.values.borrow().iter().position(|&x| x == v);
        let pos = match found {
            Some(p) => p,
            None => {
                self.values.borrow_mut().push(v);
                self.rebuild();
                self.values.borrow().len() - 1
            }
        };
        self.row.set_selected(pos as u32);
    }

    fn get(&self) -> Option<u32> {
        self.values.borrow().get(self.row.selected() as usize).copied()
    }
}

struct ButtonRow {
    slot: usize,
    row: adw::ComboRow,
    choices: RefCell<Vec<Choice>>,
}

struct Ui {
    me: RefCell<Weak<Ui>>,
    cmds: mpsc::Sender<Cmd>,
    model: RefCell<Model>,
    /// Окно само выставляет значения — обработчики изменений молчат.
    updating: Cell<bool>,
    last_toast: Cell<Option<Instant>>,

    window: adw::ApplicationWindow,
    toasts: adw::ToastOverlay,
    banner: adw::Banner,
    body: gtk::Stack,
    waiting: adw::StatusPage,
    title: adw::WindowTitle,
    stack: adw::ViewStack,
    battery: gtk::Label,
    battery_icon: gtk::Image,
    battery_box: gtk::Box,
    mouse: MouseView,
    dpi_chip: gtk::Label,
    dpi_dot: gtk::DrawingArea,
    dpi_color: Rc<Cell<[u8; 3]>>,

    button_rows: Vec<ButtonRow>,
    dpi_rows: Vec<(gtk::CheckButton, adw::SpinRow, gtk::ColorDialogButton)>,
    rate: Rc<ValueCombo>,
    lod: Rc<ValueCombo>,
    debounce: Rc<ValueCombo>,
    light_sleep: Rc<ValueCombo>,
    deep_sleep: Rc<ValueCombo>,
    led: Rc<ValueCombo>,
    motion_sync: adw::SwitchRow,
    ripple: adw::SwitchRow,
    angle: adw::SwitchRow,
    macro_editor: RefCell<Option<(usize, Rc<MacroEditor>)>>,
}

pub fn build(app: &adw::Application) {
    let (tx, rx) = async_channel::unbounded();
    let cmds = device::spawn(tx);
    let ui = Ui::new(app, cmds);
    ui.window.present();
    // Цикл событий держит окно живым: поток устройства шлёт их, пока жив канал команд в Ui.
    glib::spawn_future_local(async move {
        while let Ok(event) = rx.recv().await {
            ui.on_event(event);
        }
    });
}

/// Невидимые метки заводского и текущего пункта: по ним раскрытый список ставит пометку и галочку.
const DEFAULT_MARK: &str = "\u{2062}";
const CURRENT_MARK: &str = "\u{2063}";

/// В строке — только название функции; в раскрытом списке у заводской ещё серая пометка.
fn mark_defaults(row: &adw::ComboRow) {
    fn text(item: &glib::Object) -> (gtk::ListItem, String) {
        let item = item.downcast_ref::<gtk::ListItem>().expect("ListItem").clone();
        let s = item.item().and_downcast::<gtk::StringObject>().map(|o| o.string().to_string()).unwrap_or_default();
        (item, s)
    }
    let main = gtk::SignalListItemFactory::new();
    main.connect_setup(|_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().expect("ListItem");
        item.set_child(Some(&gtk::Label::builder().xalign(1.0).ellipsize(gtk::pango::EllipsizeMode::End).build()));
    });
    main.connect_bind(|_, item| {
        let (item, s) = text(item);
        if let Some(l) = item.child().and_downcast::<gtk::Label>() {
            l.set_label(s.trim_end_matches(CURRENT_MARK).trim_end_matches(DEFAULT_MARK));
        }
    });
    let list = gtk::SignalListItemFactory::new();
    list.connect_setup(|_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().expect("ListItem");
        let b = gtk::Box::builder().spacing(8).build();
        b.append(&gtk::Label::builder().xalign(0.0).build());
        b.append(&gtk::Label::builder().label(tr("стандарт", "default")).css_classes(["dim-label", "caption"]).build());
        b.append(&gtk::Image::from_icon_name("object-select-symbolic"));
        item.set_child(Some(&b));
    });
    list.connect_bind(move |_, item| {
        let (item, s) = text(item);
        let Some(b) = item.child().and_downcast::<gtk::Box>() else { return };
        let name = b.first_child().and_downcast::<gtk::Label>();
        let mark = name.as_ref().and_then(|n| n.next_sibling());
        let check = b.last_child();
        if let (Some(name), Some(mark), Some(check)) = (name, mark, check) {
            let current = s.ends_with(CURRENT_MARK);
            let s = s.trim_end_matches(CURRENT_MARK);
            name.set_label(s.trim_end_matches(DEFAULT_MARK));
            mark.set_visible(s.ends_with(DEFAULT_MARK));
            check.set_visible(current);
        }
    });
    row.set_factory(Some(&main));
    row.set_list_factory(Some(&list));
}

fn group(title: &str) -> adw::PreferencesGroup {
    adw::PreferencesGroup::builder().title(title).css_classes(["compact"]).build()
}

fn rgba(c: [u8; 3]) -> gdk::RGBA {
    gdk::RGBA::new(c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0, 1.0)
}

fn show_about(window: &adw::ApplicationWindow) {
    let repo = env!("CARGO_PKG_REPOSITORY");
    adw::AboutDialog::builder()
        .application_name("Ajazz Mouse")
        .application_icon(crate::APP_ID)
        .version(env!("CARGO_PKG_VERSION"))
        .comments(tr("Настройка мыши Ajazz AJ159 V2 без официального драйвера", "Configure the Ajazz AJ159 V2 mouse without the official driver"))
        .developer_name("yuto")
        .license_type(gtk::License::MitX11)
        .website(repo)
        .issue_url(format!("{repo}/issues"))
        .build()
        .present(Some(window));
}

fn seconds(s: u32) -> String {
    match s {
        s if s < 60 => format!("{s} {}", tr("с", "s")),
        s if s % 60 == 0 => format!("{} {}", s / 60, tr("мин", "min")),
        s => format!("{} {} {} {}", s / 60, tr("мин", "min"), s % 60, tr("с", "s")),
    }
}

impl Ui {
    fn new(app: &adw::Application, cmds: mpsc::Sender<Cmd>) -> Rc<Self> {
        let title = adw::WindowTitle::new("Ajazz AJ159 V2", tr("ищу донгл…", "looking for the dongle…"));
        let battery = gtk::Label::builder().css_classes(["numeric", "battery"]).build();
        let battery_icon = gtk::Image::from_icon_name("battery-missing-symbolic");
        let battery_box = gtk::Box::builder().spacing(6).css_classes(["battery-pill"]).visible(false).build();
        battery_box.append(&battery_icon);
        battery_box.append(&battery);
        let stack = adw::ViewStack::new();
        let switcher = adw::ViewSwitcher::builder().stack(&stack).policy(adw::ViewSwitcherPolicy::Wide).build();
        let header = adw::HeaderBar::builder().title_widget(&switcher).build();
        header.pack_start(&title);
        let menu = gio::Menu::new();
        menu.append(Some(tr("О программе", "About Ajazz Mouse")), Some("win.about"));
        header.pack_end(&gtk::MenuButton::builder().icon_name("open-menu-symbolic").menu_model(&menu).primary(true).build());
        header.pack_end(&battery_box);

        // --- мышь слева ---
        let dpi_chip = gtk::Label::builder().css_classes(["numeric", "dpi-chip-label"]).build();
        let dpi_color = Rc::new(Cell::new([255u8, 255, 255]));
        let dpi_dot = gtk::DrawingArea::builder().content_width(9).content_height(9).valign(gtk::Align::Center).build();
        let c = dpi_color.clone();
        dpi_dot.set_draw_func(move |_, cr, w, h| {
            let [r, g, b] = c.get();
            cr.set_source_rgb(r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0);
            cr.arc(w as f64 / 2.0, h as f64 / 2.0, w.min(h) as f64 / 2.0, 0.0, std::f64::consts::TAU);
            let _ = cr.fill();
        });
        let chip = gtk::Box::builder().spacing(8).halign(gtk::Align::Center).css_classes(["dpi-chip"]).build();
        chip.append(&dpi_dot);
        chip.append(&dpi_chip);
        let me: Rc<RefCell<Weak<Ui>>> = Rc::default();
        let m = me.clone();
        let mouse = MouseView::new(move |slot| {
            if let Some(ui) = m.borrow().upgrade()
                && let Some(b) = ui.button_rows.iter().find(|b| b.slot == slot)
            {
                // Клик по кнопке на рисунке открывает раздел «Кнопки» и её список.
                ui.stack.set_visible_child_name("buttons");
                b.row.grab_focus();
                adw::prelude::ActionRowExt::activate(&b.row);
            }
        });
        let stage = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(10)
            .width_request(270)
            .margin_start(14)
            .margin_top(14)
            .margin_bottom(14)
            .css_classes(["mouse-stage"])
            .build();
        stage.append(&mouse.area);
        stage.append(&chip);

        // --- кнопки ---
        let buttons = group(tr("Кнопки", "Buttons"));
        let reset = gtk::Button::builder().label(tr("Сбросить", "Reset"))
            .css_classes(["flat"])
            .tooltip_text(tr("Вернуть всем кнопкам заводские функции", "Give all buttons their factory functions back"))
            .build();
        buttons.set_header_suffix(Some(&reset));
        let m = me.clone();
        reset.connect_clicked(move |_| {
            if let Some(ui) = m.borrow().upgrade() {
                ui.confirm_reset();
            }
        });
        let mut button_rows = Vec::new();
        for &(slot, ru, en) in BUTTONS_LIST {
            let hint = match slot {
                SLOT_FORWARD => tr("Передняя боковая, ближе к колесу", "Front side button, closer to the wheel"),
                SLOT_BACK => tr("Задняя боковая", "Rear side button"),
                SLOT_DPI => tr("Кнопка на дне мыши", "Button on the bottom of the mouse"),
                _ => "",
            };
            let row = adw::ComboRow::builder().title(tr(ru, en)).enable_search(true).tooltip_text(hint).build();
            row.set_expression(Some(&gtk::PropertyExpression::new(
                gtk::StringObject::static_type(),
                None::<gtk::Expression>,
                "string",
            )));
            mark_defaults(&row);
            buttons.add(&row);
            button_rows.push(ButtonRow { slot, row, choices: RefCell::default() });
        }

        // --- DPI ---
        let dpi = group(tr("Уровни DPI", "DPI levels"));
        dpi.set_header_suffix(Some(&gtk::Label::builder().label(tr("точка — активный", "dot marks the active one")).css_classes(["dim-label", "caption"]).build()));
        let mut dpi_rows = Vec::new();
        let mut first: Option<gtk::CheckButton> = None;
        for i in 0..DPI_LEVELS {
            let row = adw::SpinRow::with_range(50.0, 32000.0, 50.0);
            row.set_title(&format!("{}", i + 1));
            row.set_update_policy(gtk::SpinButtonUpdatePolicy::IfValid);
            let radio = gtk::CheckButton::builder().valign(gtk::Align::Center).tooltip_text(tr("Сделать активным", "Make active")).build();
            match &first {
                Some(f) => radio.set_group(Some(f)),
                None => first = Some(radio.clone()),
            }
            let color = gtk::ColorDialogButton::builder()
                .dialog(&gtk::ColorDialog::builder().with_alpha(false).title(tr("Цвет уровня", "Level color")).build())
                .valign(gtk::Align::Center)
                .tooltip_text(tr("Цвет подсветки уровня", "Indicator color for this level"))
                .css_classes(["dpi-color"])
                .build();
            row.add_prefix(&radio);
            row.add_suffix(&color);
            dpi.add(&row);
            dpi_rows.push((radio, row, color));
        }

        // --- сенсор и питание ---
        let rate = ValueCombo::new(
            tr("Частота опроса", "Polling rate"),
            tr("Сколько раз в секунду мышь отчитывается", "How many times per second the mouse reports"),
            &[125, 250, 500, 1000],
            |v| format!("{v} {}", tr("Гц", "Hz")),
        );
        let lod = ValueCombo::new(
            tr("Высота отрыва", "Lift-off distance"),
            tr("На какой высоте сенсор перестаёт видеть коврик", "Height at which the sensor stops tracking the pad"),
            &[2, 0, 1],
            |v| [tr("1 мм", "1 mm"), tr("2 мм", "2 mm"), tr("0,7 мм", "0.7 mm")][v as usize % 3].into(),
        );
        let motion_sync = adw::SwitchRow::builder().title("Motion Sync").tooltip_text(tr("Ровнее движение, задержка чуть выше", "Smoother motion, slightly higher latency")).build();
        let ripple = adw::SwitchRow::builder().title(tr("Подавление дрожания", "Ripple control"))
            .tooltip_text(tr("Ripple Control: сглаживает сенсор на высоких DPI", "Smooths sensor jitter at high DPI"))
            .build();
        let angle = adw::SwitchRow::builder()
            .title(tr("Выпрямление линий", "Angle snapping"))
            .tooltip_text(tr(
                "Angle Snapping: тянет движение к прямым — для рисования, не для игр",
                "Pulls movement toward straight lines — for drawing, not for games",
            ))
            .build();
        let sensor = group(tr("Сенсор", "Sensor"));
        sensor.add(&rate.row);
        sensor.add(&lod.row);
        sensor.add(&motion_sync);
        sensor.add(&ripple);
        sensor.add(&angle);

        let debounce_values: Vec<u32> = (2..=25).map(|x| x * 2).collect();
        let debounce = ValueCombo::new(
            tr("Отклик кнопок", "Button debounce"),
            tr("Защита от дребезга: меньше — быстрее, но возможны двойные клики", "Lower is faster but may cause double clicks"),
            &debounce_values,
            |v| format!("{v} {}", tr("мс", "ms")),
        );
        let light_sleep = ValueCombo::new(
            tr("Лёгкий сон", "Light sleep"),
            tr("Через сколько простоя; просыпается от движения", "Idle time before it; wakes up on movement"),
            &[10, 20, 30, 60, 120, 300, 600],
            seconds,
        );
        let deep_sleep = ValueCombo::new(
            tr("Глубокий сон", "Deep sleep"),
            tr("Через сколько простоя; будится нажатием кнопки", "Idle time before it; wakes up on a button press"),
            &[1, 2, 3, 5, 10, 15, 20, 30, 45, 60],
            |v| format!("{v} {}", tr("мин", "min")),
        );
        let led = ValueCombo::new(
            tr("Подсветка DPI", "DPI indicator"),
            tr("Как светится индикатор уровня DPI", "How the DPI level indicator lights up"),
            &[0, 5, 6],
            |v| {
                match v {
                    5 => tr("постоянно", "steady"),
                    6 => tr("дыхание", "breathing"),
                    _ => tr("выключена", "off"),
                }
                .into()
            },
        );
        let power = group(tr("Питание", "Power"));
        power.add(&debounce.row);
        power.add(&light_sleep.row);
        power.add(&deep_sleep.row);
        power.add(&led.row);

        let page = |group: &adw::PreferencesGroup| {
            let clamp = adw::Clamp::builder().maximum_size(560).margin_top(14).margin_bottom(18).margin_end(18).child(group).build();
            gtk::ScrolledWindow::builder().hscrollbar_policy(gtk::PolicyType::Never).hexpand(true).child(&clamp).build()
        };
        stack.add_titled_with_icon(&page(&buttons), Some("buttons"), tr("Кнопки", "Buttons"), "input-mouse-symbolic");
        stack.add_titled_with_icon(&page(&dpi), Some("dpi"), "DPI", "find-location-symbolic");
        stack.add_titled_with_icon(&page(&sensor), Some("sensor"), tr("Сенсор", "Sensor"), "speedometer-symbolic");
        stack.add_titled_with_icon(&page(&power), Some("power"), tr("Питание", "Power"), "battery-good-symbolic");

        let content = gtk::Box::builder().spacing(18).build();
        content.append(&stage);
        content.append(&stack);

        let waiting = adw::StatusPage::builder()
            .icon_name("input-mouse-symbolic")
            .title(tr("Ищу донгл Ajazz", "Looking for the Ajazz dongle"))
            .description(tr("Вставь донгл 2.4 ГГц в USB", "Plug the 2.4 GHz dongle into a USB port"))
            .build();
        waiting.set_child(Some(&adw::Spinner::builder().width_request(32).height_request(32).halign(gtk::Align::Center).build()));
        let body = gtk::Stack::builder().transition_type(gtk::StackTransitionType::Crossfade).vexpand(true).build();
        body.add_named(&waiting, Some("waiting"));
        body.add_named(&content, Some("content"));

        let banner = adw::Banner::new(tr("Мышь спит — пошевели её, чтобы применить настройки", "The mouse is asleep — move it to apply the settings"));
        let main = gtk::Box::builder().orientation(gtk::Orientation::Vertical).build();
        main.append(&banner);
        main.append(&body);
        let toasts = adw::ToastOverlay::new();
        toasts.set_child(Some(&main));
        let view = adw::ToolbarView::new();
        view.add_top_bar(&header);
        view.set_content(Some(&toasts));
        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title("Ajazz Mouse")
            .default_width(960)
            .default_height(620)
            .content(&view)
            .build();
        let about = gio::SimpleAction::new("about", None);
        let w = window.clone();
        about.connect_activate(move |_, _| show_about(&w));
        window.add_action(&about);

        let ui = Rc::new(Self {
            me: RefCell::default(),
            cmds,
            model: RefCell::default(),
            updating: Cell::new(false),
            last_toast: Cell::new(None),
            window,
            toasts,
            banner,
            body,
            waiting,
            title,
            stack,
            battery,
            battery_icon,
            battery_box,
            mouse,
            dpi_chip,
            dpi_dot,
            dpi_color,
            button_rows,
            dpi_rows,
            rate,
            lod,
            debounce,
            light_sleep,
            deep_sleep,
            led,
            motion_sync,
            ripple,
            angle,
            macro_editor: RefCell::default(),
        });
        *me.borrow_mut() = Rc::downgrade(&ui);
        *ui.me.borrow_mut() = Rc::downgrade(&ui);
        ui.connect();
        ui
    }

    fn weak(&self) -> Weak<Ui> {
        self.me.borrow().clone()
    }

    fn connect(&self) {
        let w = self.weak();
        let edit = Rc::new(move |f: &dyn Fn(&Ui)| {
            if let Some(ui) = w.upgrade()
                && !ui.updating.get()
            {
                f(&ui);
            }
        });

        for b in &self.button_rows {
            let (e, slot) = (edit.clone(), b.slot);
            b.row.connect_selected_notify(move |row| {
                let i = row.selected() as usize;
                e(&|ui| ui.choose(slot, i));
            });
            // Наведение на строку подсвечивает кнопку на рисунке.
            let hover = gtk::EventControllerMotion::new();
            let m = self.mouse.clone();
            let shown = if slot == SLOT_WHEEL_UP || slot == SLOT_WHEEL_DOWN { SLOT_MIDDLE } else { slot };
            hover.connect_enter(move |_, _, _| m.set_selected(Some(shown)));
            let m = self.mouse.clone();
            hover.connect_leave(move |_| m.set_selected(None));
            b.row.add_controller(hover);
        }
        for (i, (radio, row, color)) in self.dpi_rows.iter().enumerate() {
            let e = edit.clone();
            radio.connect_toggled(move |r| {
                if r.is_active() {
                    e(&|ui| ui.change_dpi(|d| d.set_active(i)));
                }
            });
            let e = edit.clone();
            row.adjustment().connect_value_changed(move |a| {
                let v = (a.value() / 50.0).round() as u32 * 50;
                e(&|ui| ui.change_dpi(|d| d.set_value(i, v)));
            });
            let e = edit.clone();
            color.connect_rgba_notify(move |c| {
                let rgba = c.rgba();
                let rgb = [rgba.red(), rgba.green(), rgba.blue()].map(|x| (x * 255.0).round() as u8);
                e(&|ui| ui.change_dpi(|d| d.set_color(i, rgb)));
            });
        }
        let combo = |c: &Rc<ValueCombo>, f: fn(&Ui, u32)| {
            let (e, c2) = (edit.clone(), c.clone());
            c.row.connect_selected_notify(move |_| {
                if let Some(v) = c2.get() {
                    e(&|ui| f(ui, v));
                }
            });
        };
        combo(&self.rate, |ui, hz| {
            ui.change(|m| {
                let r = m.rate.as_mut()?;
                r.set_hz(hz);
                Some(r.0.clone())
            })
        });
        combo(&self.lod, |ui, v| ui.change_dpi(|d| d.set_lod(v as u8)));
        combo(&self.debounce, |ui, v| ui.change_light(|l| l.set_debounce_ms(v)));
        combo(&self.light_sleep, |ui, v| ui.change_light(|l| l.set_light_sleep_s(v)));
        combo(&self.deep_sleep, |ui, v| ui.change_light(|l| l.set_deep_sleep_min(v)));
        combo(&self.led, |ui, v| ui.change_light(|l| l.set_mode(v as u8)));
        let switch = |s: &adw::SwitchRow, f: fn(&mut Dpi, bool)| {
            let e = edit.clone();
            s.connect_active_notify(move |s| {
                let on = s.is_active();
                e(&|ui| ui.change_dpi(|d| f(d, on)));
            });
        };
        switch(&self.motion_sync, Dpi::set_motion_sync);
        switch(&self.ripple, Dpi::set_ripple);
        switch(&self.angle, Dpi::set_angle_snap);
    }

    // --- события от мыши ---

    fn on_event(&self, event: Event) {
        match event {
            Event::Connected => {
                self.title.set_subtitle(tr("донгл 2.4 ГГц · читаю настройки…", "2.4 GHz dongle · reading settings…"));
                self.waiting.set_title(tr("Читаю настройки мыши", "Reading mouse settings"));
                self.waiting.set_description(Some(tr("Если мышь спит — пошевели её", "If the mouse is asleep, move it")));
            }
            Event::Disconnected => {
                *self.model.borrow_mut() = Model::default();
                self.title.set_subtitle(tr("донгл не найден", "dongle not found"));
                self.battery_box.set_visible(false);
                self.banner.set_revealed(false);
                self.waiting.set_title(tr("Ищу донгл Ajazz", "Looking for the Ajazz dongle"));
                self.waiting.set_description(Some(tr("Вставь донгл 2.4 ГГц в USB", "Plug the 2.4 GHz dongle into a USB port")));
                self.body.set_visible_child_name("waiting");
            }
            Event::Asleep(asleep) => {
                let loaded = self.model.borrow().dpi.is_some();
                self.banner.set_revealed(asleep && loaded);
                if asleep && !loaded {
                    self.waiting.set_title(tr("Мышь спит", "The mouse is asleep"));
                    self.waiting.set_description(Some(tr("Пошевели мышь Ajazz — и настройки прочитаются", "Move the Ajazz mouse to read its settings")));
                }
            }
            Event::Battery { percent, charging } => {
                self.battery_box.set_visible(true);
                self.battery.set_label(&format!("{percent}%"));
                let level = (percent as u32 / 10 * 10).min(100);
                let charge = if charging { "-charging" } else { "" };
                self.battery_icon.set_icon_name(Some(&format!("battery-level-{level}{charge}-symbolic")));
            }
            Event::DpiLevel(level) => {
                if let Some(d) = self.model.borrow_mut().dpi.as_mut() {
                    d.set_active(level as usize);
                }
                self.show_settings();
            }
            Event::Settings { dpi, light, rate, buttons } => {
                *self.model.borrow_mut() = Model {
                    dpi: Some(Dpi(dpi)),
                    light: Some(Light(light)),
                    rate: Some(Rate(rate)),
                    buttons: Some(Buttons(buttons)),
                };
                self.show_settings();
                self.banner.set_revealed(false);
                self.body.set_visible_child_name("content");
                // Иначе фокус встаёт в первое поле и выделяет его значение.
                GtkWindowExt::set_focus(&self.window, None::<&gtk::Widget>);
            }
            Event::Macro(index, report) => {
                if let Some((slot, editor)) = self.macro_editor.borrow().as_ref()
                    && *slot as u8 + 1 == index
                    && let Some((mode, count, events)) = parse_macro(&report)
                {
                    editor.load(mode, count, events);
                }
            }
            Event::Saved => {
                self.banner.set_revealed(false);
                // Ползунок может сохраняться много раз подряд — одного тоста хватит.
                if self.last_toast.get().is_none_or(|t| t.elapsed() > Duration::from_secs(3)) {
                    self.last_toast.set(Some(Instant::now()));
                    self.toasts.add_toast(adw::Toast::builder().title(tr("Сохранено в мышь", "Saved to the mouse")).timeout(1).build());
                }
            }
            Event::SaveFailed => self.toasts.add_toast(adw::Toast::new(tr("Мышь не приняла настройку — попробуй ещё раз", "The mouse did not accept the setting — try again"))),
        }
    }

    /// Разложить модель по виджетам.
    fn show_settings(&self) {
        let m = self.model.borrow();
        let (Some(dpi), Some(light), Some(rate), Some(buttons)) = (&m.dpi, &m.light, &m.rate, &m.buttons) else { return };
        self.updating.set(true);
        for b in &self.button_rows {
            let current = buttons.slot(b.slot);
            // Сначала запись своего, затем текущее (если его нет среди готовых) и готовые функции.
            let mut choices = vec![Choice::Shortcut];
            if has_macro(b.slot) {
                choices.push(Choice::Macro);
            }
            if !FUNCTIONS.iter().any(|f| f.0 == current) {
                choices.push(Choice::Value(current));
            }
            choices.extend(FUNCTIONS.iter().map(|f| Choice::Value(f.0)));
            let labels: Vec<String> = choices
                .iter()
                .map(|c| {
                    let label = match c {
                        Choice::Value(v) if default_function(b.slot) == Some(*v) => format!("{}{DEFAULT_MARK}", function_name(*v)),
                        Choice::Value(v) => function_name(*v),
                        Choice::Shortcut => tr("Записать клавишу или сочетание…", "Record a key or shortcut…").into(),
                        Choice::Macro => tr("Записать макрос…", "Record a macro…").into(),
                    };
                    if *c == Choice::Value(current) { label + CURRENT_MARK } else { label }
                })
                .collect();
            let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
            b.row.set_model(Some(&gtk::StringList::new(&refs)));
            let pos = choices.iter().position(|c| *c == Choice::Value(current)).unwrap_or(0);
            b.row.set_selected(pos as u32);
            *b.choices.borrow_mut() = choices;
        }
        for (i, (radio, row, color)) in self.dpi_rows.iter().enumerate() {
            row.set_value(dpi.value(i) as f64);
            color.set_rgba(&rgba(dpi.color(i)));
            radio.set_active(dpi.active() == i);
        }
        self.rate.set(rate.hz());
        self.lod.set(dpi.lod() as u32);
        self.motion_sync.set_active(dpi.motion_sync());
        self.ripple.set_active(dpi.ripple());
        self.angle.set_active(dpi.angle_snap());
        self.debounce.set(light.debounce_ms());
        self.light_sleep.set(light.light_sleep_s());
        self.deep_sleep.set(light.deep_sleep_min());
        self.led.set(light.mode() as u32);
        let active = dpi.active();
        self.dpi_chip.set_label(&format!("{} DPI · {} {}", dpi.value(active), rate.hz(), tr("Гц", "Hz")));
        self.dpi_color.set(dpi.color(active));
        self.dpi_dot.queue_draw();
        self.title.set_subtitle(tr("донгл 2.4 ГГц", "2.4 GHz dongle"));
        self.updating.set(false);
    }

    // --- правки ---

    /// Изменить модель и отправить отчёт в мышь.
    fn change(&self, f: impl FnOnce(&mut Model) -> Option<Vec<u8>>) {
        let report = f(&mut self.model.borrow_mut());
        if let Some(report) = report {
            let _ = self.cmds.send(Cmd::Write(report));
            self.show_settings();
        }
    }

    fn change_dpi(&self, f: impl FnOnce(&mut Dpi)) {
        self.change(|m| {
            let d = m.dpi.as_mut()?;
            f(d);
            d.finish();
            Some(d.0.clone())
        });
    }

    fn change_light(&self, f: impl FnOnce(&mut Light)) {
        self.change(|m| {
            let l = m.light.as_mut()?;
            f(l);
            l.finish();
            Some(l.0.clone())
        });
    }

    fn choose(&self, slot: usize, index: usize) {
        let Some(row) = self.button_rows.iter().find(|b| b.slot == slot) else { return };
        let choice = row.choices.borrow().get(index).copied();
        match choice {
            Some(Choice::Value(v)) => self.assign(slot, v),
            Some(Choice::Shortcut) => self.capture_shortcut(slot),
            Some(Choice::Macro) => self.open_macro_editor(slot),
            None => {}
        }
    }

    fn assign(&self, slot: usize, value: [u8; 3]) {
        let ok = {
            let m = self.model.borrow();
            let Some(buttons) = m.buttons.as_ref() else { return };
            // Иначе мышью станет нечем кликнуть.
            value[0] == 2 || (0..18).any(|s| s != slot && buttons.slot(s)[0] == 2)
        };
        if !ok {
            self.toasts.add_toast(adw::Toast::new(tr("Хотя бы одна кнопка должна остаться левым кликом", "At least one button must stay a left click")));
            self.show_settings();
            return;
        }
        self.change(|m| {
            let b = m.buttons.as_mut()?;
            b.set_slot(slot, value);
            b.finish();
            Some(b.0.clone())
        });
    }

    fn confirm_reset(&self) {
        let dialog = adw::AlertDialog::builder()
            .heading(tr("Сбросить кнопки?", "Reset buttons?"))
            .body(tr(
                "Все кнопки и колесо получат заводские функции. Назначенные клавиши и макросы с кнопок пропадут.",
                "All buttons and the wheel get their factory functions back. Assigned keys and macros will be removed from the buttons.",
            ))
            .build();
        dialog.add_responses(&[("cancel", tr("Отмена", "Cancel")), ("reset", tr("Сбросить", "Reset"))]);
        dialog.set_response_appearance("reset", adw::ResponseAppearance::Destructive);
        let w = self.weak();
        dialog.connect_response(Some("reset"), move |_, _| {
            if let Some(ui) = w.upgrade() {
                ui.change(|m| {
                    let b = m.buttons.as_mut()?;
                    for &(slot, value) in DEFAULTS {
                        b.set_slot(slot, value);
                    }
                    b.finish();
                    Some(b.0.clone())
                });
            }
        });
        dialog.present(Some(&self.window));
    }

    /// Окно «нажми сочетание»: первое нажатие с обычной клавишей и есть ответ.
    fn capture_shortcut(&self, slot: usize) {
        let dialog = adw::AlertDialog::builder()
            .heading(if crate::i18n::russian() {
                format!("Клавиша для кнопки «{}»", button_title(slot))
            } else {
                format!("Key for the “{}” button", button_title(slot))
            })
            .body(tr("Нажми на клавиатуре, например F5 или Ctrl + Shift + K", "Press a key or a shortcut, e.g. F5 or Ctrl + Shift + K"))
            .build();
        dialog.add_response("cancel", tr("Отмена", "Cancel"));
        let keys_ctl = gtk::EventControllerKey::new();
        keys_ctl.set_propagation_phase(gtk::PropagationPhase::Capture);
        let picked: Rc<Cell<Option<[u8; 3]>>> = Rc::default();
        let (d, p) = (dialog.clone(), picked.clone());
        keys_ctl.connect_key_pressed(move |_, _, keycode, state| {
            let Some(usage) = keys::usage_from_keycode(keycode) else { return glib::Propagation::Stop };
            if keys::is_modifier(usage) {
                return glib::Propagation::Stop;
            }
            let mut mods = 0;
            for (mask, bit) in [
                (gdk::ModifierType::CONTROL_MASK, MOD_CTRL),
                (gdk::ModifierType::SHIFT_MASK, MOD_SHIFT),
                (gdk::ModifierType::ALT_MASK, MOD_ALT),
                (gdk::ModifierType::SUPER_MASK, MOD_SUPER),
            ] {
                if state.contains(mask) {
                    mods |= bit;
                }
            }
            p.set(Some([FN_SHORTCUT, mods, usage]));
            d.close();
            glib::Propagation::Stop
        });
        dialog.add_controller(keys_ctl);
        let w = self.weak();
        dialog.connect_closed(move |_| {
            if let Some(ui) = w.upgrade() {
                match picked.get() {
                    Some(v) => ui.assign(slot, v),
                    None => ui.show_settings(),
                }
            }
        });
        dialog.present(Some(&self.window));
    }

    fn open_macro_editor(&self, slot: usize) {
        let index = slot as u8 + 1;
        let w = self.weak();
        let editor = MacroEditor::new(button_title(slot), move |mode, count, events| {
            let Some(ui) = w.upgrade() else { return };
            if events.is_empty() {
                ui.toasts.add_toast(adw::Toast::new(tr("Макрос пустой — нечего сохранять", "The macro is empty — nothing to save")));
                return;
            }
            ui.assign(slot, [FN_MACRO, 0, index]);
            let _ = ui.cmds.send(Cmd::Write(build_macro(index, mode, count, &events)));
        });
        // Кнопка уже с макросом — покажем его.
        if self.model.borrow().buttons.as_ref().is_some_and(|b| b.slot(slot) == [FN_MACRO, 0, index]) {
            let _ = self.cmds.send(Cmd::ReadMacro(index));
        }
        let w = self.weak();
        editor.dialog.connect_closed(move |_| {
            if let Some(ui) = w.upgrade() {
                ui.show_settings();
            }
        });
        *self.macro_editor.borrow_mut() = Some((slot, editor.clone()));
        editor.dialog.present(Some(&self.window));
    }
}
