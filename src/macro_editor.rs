//! Редактор макроса: запись нажатий клавиш с паузами между ними.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;

use crate::i18n::tr;
use crate::keys;
use crate::protocol::{MACRO_MAX_DELAY_MS, MACRO_MAX_EVENTS, MacroEvent};

pub struct MacroEditor {
    pub dialog: adw::Dialog,
    events: Rc<RefCell<Vec<MacroEvent>>>,
    list: gtk::ListBox,
    empty: gtk::Label,
    mode: adw::ToggleGroup,
    count: adw::SpinRow,
}

impl MacroEditor {
    pub fn new(button: &str, on_save: impl Fn(u8, u8, Vec<MacroEvent>) + 'static) -> Rc<Self> {
        let record = gtk::ToggleButton::builder().label(tr("Записать", "Record")).css_classes(["destructive-action"]).build();
        let clear = gtk::Button::builder().icon_name("edit-clear-all-symbolic").tooltip_text(tr("Очистить", "Clear")).build();
        let save = gtk::Button::builder().label(tr("Сохранить в мышь", "Save to mouse")).css_classes(["suggested-action"]).build();
        let header = adw::HeaderBar::builder().title_widget(&adw::WindowTitle::new(tr("Макрос", "Macro"), button)).build();
        header.pack_start(&record);
        header.pack_start(&clear);
        header.pack_end(&save);

        let mode = adw::ToggleGroup::new();
        for (name, ru, en) in [("0", "N раз", "N times"), ("1", "Автоповтор", "Loop"), ("2", "Пока зажата", "While held")] {
            mode.add(adw::Toggle::builder().name(name).label(tr(ru, en)).build());
        }
        mode.set_active_name(Some("0"));
        mode.set_valign(gtk::Align::Center);
        let mode_row = adw::ActionRow::builder().title(tr("Повтор", "Repeat")).build();
        mode_row.add_suffix(&mode);
        let count = adw::SpinRow::with_range(1.0, 255.0, 1.0);
        count.set_title(tr("Сколько раз", "Times"));
        count.set_value(1.0);
        let repeat = adw::PreferencesGroup::new();
        repeat.add(&mode_row);
        repeat.add(&count);

        let list = gtk::ListBox::builder().selection_mode(gtk::SelectionMode::None).css_classes(["boxed-list"]).build();
        let empty = gtk::Label::builder()
            .label(tr(
                "Нажми «Записать» и введи последовательность клавиш.\nПаузы между нажатиями сохранятся (до 1,27 с).",
                "Press “Record” and type a key sequence.\nPauses between keys are kept (up to 1.27 s).",
            ))
            .justify(gtk::Justification::Center)
            .css_classes(["dim-label"])
            .margin_top(24)
            .margin_bottom(24)
            .build();
        let events_group = adw::PreferencesGroup::builder()
            .title(tr("События", "Events"))
            .description(if crate::i18n::russian() {
                format!("Не больше {MACRO_MAX_EVENTS} нажатий и отпусканий")
            } else {
                format!("Up to {MACRO_MAX_EVENTS} key presses and releases")
            })
            .build();
        events_group.add(&empty);
        events_group.add(&list);

        let page = adw::PreferencesPage::new();
        page.add(&repeat);
        page.add(&events_group);
        let view = adw::ToolbarView::new();
        view.add_top_bar(&header);
        view.set_content(Some(&page));
        let dialog = adw::Dialog::builder()
            .title(format!("{}: {button}", tr("Макрос", "Macro")))
            .content_width(520)
            .content_height(620)
            .child(&view)
            .build();

        let this = Rc::new(Self { dialog, events: Rc::default(), list, empty, mode, count });
        let t = this.clone();
        this.mode.connect_notify_local(Some("active-name"), move |m, _| {
            t.count.set_sensitive(m.active_name().as_deref() == Some("0"));
        });

        // Запись: ловим клавиши раньше всех виджетов диалога.
        let keys_ctl = gtk::EventControllerKey::new();
        keys_ctl.set_propagation_phase(gtk::PropagationPhase::Capture);
        let held: Rc<RefCell<HashSet<u8>>> = Rc::default();
        let last = Rc::new(Cell::new(0u32));
        let (t, r, h, l) = (this.clone(), record.clone(), held.clone(), last.clone());
        keys_ctl.connect_key_pressed(move |ctl, _, keycode, _| {
            if !r.is_active() {
                return glib::Propagation::Proceed;
            }
            if let Some(usage) = keys::usage_from_keycode(keycode)
                && h.borrow_mut().insert(usage)
            {
                t.push(usage, false, ctl.current_event_time(), &l);
            }
            glib::Propagation::Stop
        });
        let (t, r, h, l) = (this.clone(), record.clone(), held.clone(), last.clone());
        keys_ctl.connect_key_released(move |ctl, _, keycode, _| {
            if !r.is_active() {
                return;
            }
            if let Some(usage) = keys::usage_from_keycode(keycode)
                && h.borrow_mut().remove(&usage)
            {
                t.push(usage, true, ctl.current_event_time(), &l);
            }
        });
        this.dialog.add_controller(keys_ctl);

        let l = last.clone();
        record.connect_toggled(move |b| {
            b.set_label(if b.is_active() { tr("Стоп", "Stop") } else { tr("Записать", "Record") });
            l.set(0);
        });
        let t = this.clone();
        clear.connect_clicked(move |_| {
            t.events.borrow_mut().clear();
            t.refresh();
        });
        let t = this.clone();
        save.connect_clicked(move |_| {
            record.set_active(false);
            let mode: u8 = t.mode.active_name().and_then(|n| n.parse().ok()).unwrap_or(0);
            on_save(mode, t.count.value() as u8, t.events.borrow().clone());
            t.dialog.close();
        });
        this.refresh();
        this
    }

    /// Показать макрос, прочитанный из мыши.
    pub fn load(&self, mode: u8, count: u8, events: Vec<MacroEvent>) {
        self.mode.set_active_name(Some(&mode.min(2).to_string()));
        self.count.set_value(count.max(1) as f64);
        *self.events.borrow_mut() = events;
        self.refresh();
    }

    fn push(&self, usage: u8, release: bool, time: u32, last: &Cell<u32>) {
        let mut events = self.events.borrow_mut();
        if events.len() >= MACRO_MAX_EVENTS {
            return;
        }
        let delay = if last.get() == 0 { 10 } else { time.saturating_sub(last.get()).clamp(10, MACRO_MAX_DELAY_MS) };
        last.set(time);
        events.push(MacroEvent { usage, release, delay_ms: delay });
        drop(events);
        self.refresh();
    }

    fn refresh(&self) {
        while let Some(row) = self.list.first_child() {
            self.list.remove(&row);
        }
        let events = self.events.borrow();
        self.empty.set_visible(events.is_empty());
        self.list.set_visible(!events.is_empty());
        for e in events.iter() {
            let row = adw::ActionRow::builder()
                .title(keys::usage_name(e.usage))
                .subtitle(if e.release { tr("отпустить", "release") } else { tr("нажать", "press") })
                .build();
            row.add_prefix(&gtk::Image::from_icon_name(if e.release { "go-up-symbolic" } else { "go-down-symbolic" }));
            row.add_suffix(&gtk::Label::builder().label(format!("+{} {}", e.delay_ms, tr("мс", "ms"))).css_classes(["dim-label", "numeric"]).build());
            self.list.append(&row);
        }
    }
}
