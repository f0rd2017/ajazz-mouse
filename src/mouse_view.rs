//! Белая AJ159 V2 сверху — картинка, сгенерированная по снимку с сайта Ajazz (кадр «Product Dimensions»).
//! Кнопки на ней подсвечиваются наведением и выбираются кликом; зоны размечены по самой картинке.

use std::cell::RefCell;
use std::f64::consts::PI;
use std::rc::Rc;

use gtk::cairo::{self, Context, RadialGradient};
use gtk::prelude::*;
use gtk::{gdk, glib};

use crate::protocol::{SLOT_BACK, SLOT_DPI, SLOT_FORWARD, SLOT_LEFT, SLOT_MIDDLE, SLOT_RIGHT};

const PNG: &[u8] = include_bytes!("../assets/mouse_top.png");
pub const ACCENT: (f64, f64, f64) = (0.36, 0.49, 0.98);

struct Photo {
    surface: cairo::ImageSurface,
    alpha: Vec<u8>,
    w: f64,
    h: f64,
}

impl Photo {
    fn load() -> Self {
        let texture = gdk::Texture::from_bytes(&glib::Bytes::from_static(PNG)).expect("картинка мыши");
        let (w, h) = (texture.width(), texture.height());
        let stride = w as usize * 4;
        // Формат выгрузки GDK по умолчанию — BGRA с premultiplied alpha, то же, что cairo ARGB32.
        let mut data = vec![0u8; stride * h as usize];
        texture.download(&mut data, stride);
        let alpha = data.as_chunks::<4>().0.iter().map(|p| p[3]).collect();
        let surface = cairo::ImageSurface::create_for_data(data, cairo::Format::ARgb32, w, h, stride as i32).expect("поверхность cairo");
        Self { surface, alpha, w: w as f64, h: h as f64 }
    }

    fn opaque(&self, x: f64, y: f64) -> bool {
        if x < 0.0 || y < 0.0 || x >= self.w || y >= self.h {
            return false;
        }
        self.alpha[y as usize * self.w as usize + x as usize] > 128
    }
}

#[derive(Default)]
struct State {
    hover: Option<usize>,
    selected: Option<usize>,
}

#[derive(Clone)]
pub struct MouseView {
    pub area: gtk::DrawingArea,
    state: Rc<RefCell<State>>,
}

impl MouseView {
    pub fn new(on_select: impl Fn(usize) + 'static) -> Self {
        let photo = Rc::new(Photo::load());
        let area = gtk::DrawingArea::builder().content_width(230).content_height(400).hexpand(true).vexpand(true).build();
        let state = Rc::new(RefCell::new(State::default()));

        let (s, p) = (state.clone(), photo.clone());
        area.set_draw_func(move |_, cr, w, h| draw(cr, w as f64, h as f64, &p, &s.borrow()));

        let motion = gtk::EventControllerMotion::new();
        let (s, a, p) = (state.clone(), area.clone(), photo.clone());
        motion.connect_motion(move |_, x, y| {
            let hit = hit_test(&p, a.width() as f64, a.height() as f64, x, y);
            if s.borrow().hover != hit {
                s.borrow_mut().hover = hit;
                a.set_cursor_from_name(Some(if hit.is_some() { "pointer" } else { "default" }));
                a.queue_draw();
            }
        });
        let (s, a) = (state.clone(), area.clone());
        motion.connect_leave(move |_| {
            s.borrow_mut().hover = None;
            a.queue_draw();
        });
        area.add_controller(motion);

        let click = gtk::GestureClick::new();
        let (a, p) = (area.clone(), photo);
        click.connect_released(move |_, _, x, y| {
            if let Some(slot) = hit_test(&p, a.width() as f64, a.height() as f64, x, y) {
                on_select(slot);
            }
        });
        area.add_controller(click);
        Self { area, state }
    }

    /// Подсветка кнопки снаружи — когда мышь над её строкой в списке.
    pub fn set_selected(&self, slot: Option<usize>) {
        self.state.borrow_mut().selected = slot;
        self.area.queue_draw();
    }
}

fn transform(p: &Photo, w: f64, h: f64) -> (f64, f64, f64) {
    let s = (w / (p.w + 30.0)).min(h / (p.h + 30.0));
    (s, (w - p.w * s) / 2.0, (h - p.h * s) / 2.0)
}

fn hit_test(p: &Photo, w: f64, h: f64, x: f64, y: f64) -> Option<usize> {
    let (s, ox, oy) = transform(p, w, h);
    let (x, y) = ((x - ox) / s, (y - oy) / s);
    let surface = cairo::ImageSurface::create(cairo::Format::A8, 1, 1).ok()?;
    let cr = Context::new(&surface).ok()?;
    let inside = |slot| {
        cr.new_path();
        region(&cr, slot);
        cr.in_fill(x, y).unwrap_or(false)
    };
    // Боковые кнопки и колесо — раньше: они лежат поверх левой и правой.
    if let Some(slot) = [SLOT_MIDDLE, SLOT_FORWARD, SLOT_BACK].into_iter().find(|&s| inside(s)) {
        return Some(slot);
    }
    if !p.opaque(x, y) {
        return None;
    }
    [SLOT_LEFT, SLOT_RIGHT].into_iter().find(|&s| inside(s))
}

fn rounded(cr: &Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -PI / 2.0, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, PI / 2.0);
    cr.arc(x + r, y + h - r, r, PI / 2.0, PI);
    cr.arc(x + r, y + r, r, PI, 3.0 * PI / 2.0);
    cr.close_path();
}

fn polygon(cr: &Context, pts: &[(f64, f64)]) {
    cr.move_to(pts[0].0, pts[0].1);
    for &(x, y) in &pts[1..] {
        cr.line_to(x, y);
    }
    cr.close_path();
}

/// Зоны в пикселях картинки (542×1000): по швам, колесу и боковым кнопкам на ней.
fn region(cr: &Context, slot: usize) {
    match slot {
        SLOT_LEFT => polygon(cr, &[(0.0, 0.0), (265.0, 0.0), (265.0, 455.0), (198.0, 455.0), (183.0, 466.0), (62.0, 466.0), (50.0, 452.0), (39.0, 90.0)]),
        SLOT_RIGHT => polygon(cr, &[(265.0, 0.0), (542.0, 0.0), (496.0, 90.0), (494.0, 250.0), (483.0, 440.0), (472.0, 466.0), (352.0, 466.0), (338.0, 455.0), (265.0, 455.0)]),
        SLOT_MIDDLE => rounded(cr, 225.0, 94.0, 83.0, 179.0, 36.0),
        SLOT_FORWARD => rounded(cr, 9.0, 315.0, 25.0, 142.0, 10.0),
        SLOT_BACK => rounded(cr, 12.0, 456.0, 41.0, 167.0, 12.0),
        // Кнопка DPI на дне: показываем её пунктиром только при подсветке.
        SLOT_DPI => cr.arc(271.0, 740.0, 34.0, 0.0, 2.0 * PI),
        _ => {}
    }
}

fn draw(cr: &Context, w: f64, h: f64, p: &Photo, st: &State) {
    let (s, ox, oy) = transform(p, w, h);
    cr.translate(ox, oy);
    cr.scale(s, s);

    // Мягкая тень под мышью.
    cr.save().ok();
    cr.translate(p.w / 2.0, p.h / 2.0 + 10.0);
    cr.scale(1.0, p.h / p.w);
    let r = p.w * 0.62;
    let shadow = RadialGradient::new(0.0, 0.0, r * 0.4, 0.0, 0.0, r);
    shadow.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, 0.55);
    shadow.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.0);
    cr.set_source(&shadow).ok();
    cr.arc(0.0, 0.0, r, 0.0, 2.0 * PI);
    let _ = cr.fill();
    cr.restore().ok();

    cr.set_source_surface(&p.surface, 0.0, 0.0).ok();
    cr.source().set_filter(cairo::Filter::Good);
    cr.paint().ok();

    let (ar, ag, ab) = ACCENT;
    for slot in [SLOT_LEFT, SLOT_RIGHT, SLOT_MIDDLE, SLOT_FORWARD, SLOT_BACK, SLOT_DPI] {
        let strong = st.selected == Some(slot);
        if !strong && st.hover != Some(slot) {
            continue;
        }
        if slot == SLOT_DPI {
            cr.new_path();
            region(cr, slot);
            cr.set_source_rgba(ar, ag, ab, 0.9);
            cr.set_line_width(2.5);
            cr.set_dash(&[5.0, 4.0], 0.0);
            let _ = cr.stroke();
            cr.set_dash(&[], 0.0);
            continue;
        }
        // Тон по зоне, но только там, где есть сама мышь (по альфе снимка).
        cr.save().ok();
        cr.new_path();
        region(cr, slot);
        cr.clip();
        if slot == SLOT_LEFT || slot == SLOT_RIGHT {
            // Колесо — отдельная кнопка, его не красим вместе с левой и правой.
            cr.new_path();
            cr.rectangle(0.0, 0.0, p.w, p.h);
            region(cr, SLOT_MIDDLE);
            cr.set_fill_rule(cairo::FillRule::EvenOdd);
            cr.clip();
            cr.set_fill_rule(cairo::FillRule::Winding);
        }
        cr.set_source_rgba(ar, ag, ab, if strong { 0.38 } else { 0.22 });
        cr.mask_surface(&p.surface, 0.0, 0.0).ok();
        cr.restore().ok();
    }
}
