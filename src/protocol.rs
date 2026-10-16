//! Формат настроек мыши Ajazz AJ159 V2 (V2MAX, id 0x9e) в feature-отчётах HID.
//!
//! Восстановлен по официальному веб-драйверу ajazz.szslxd-tech.com и его помощнику
//! WebDriver.exe, проверен чтением и записью на мыши (см. README).
//! Каждый отчёт начинается с `[номер, длина, профиль]`, контрольная сумма — сумма
//! байтов с третьего, big-endian, сразу после данных.

use crate::i18n::tr;

pub const DPI: u8 = 4;
pub const LIGHT: u8 = 5;
pub const RATE: u8 = 6;
pub const BUTTONS: u8 = 8;
pub const MACRO: u8 = 9;

/// Длина отчёта вместе с номером — столько же просит и чтение.
pub fn report_len(rid: u8) -> usize {
    match rid {
        DPI => 0x38,
        LIGHT => 0x0F,
        RATE => 0x09,
        BUTTONS => 0x3B,
        MACRO => 0x83,
        _ => 0,
    }
}

fn put_checksum(buf: &mut [u8], at: usize) {
    let sum: u32 = buf[3..at].iter().map(|&b| b as u32).sum();
    buf[at] = (sum >> 8) as u8;
    buf[at + 1] = sum as u8;
}

pub const DPI_LEVELS: usize = 6;

#[derive(Clone)]
pub struct Dpi(pub Vec<u8>);

impl Dpi {
    pub fn value(&self, level: usize) -> u32 {
        self.0[8 + level] as u32 | (self.0[16 + level] as u32) << 8
    }
    pub fn set_value(&mut self, level: usize, dpi: u32) {
        self.0[8 + level] = dpi as u8;
        self.0[16 + level] = (dpi >> 8) as u8;
    }
    /// Активный уровень, с нуля.
    pub fn active(&self) -> usize {
        (self.0[24] as usize).saturating_sub(1).min(DPI_LEVELS - 1)
    }
    pub fn set_active(&mut self, level: usize) {
        self.0[24] = level as u8 + 1;
    }
    pub fn color(&self, level: usize) -> [u8; 3] {
        let i = 25 + level * 3;
        [self.0[i], self.0[i + 1], self.0[i + 2]]
    }
    pub fn set_color(&mut self, level: usize, rgb: [u8; 3]) {
        let i = 25 + level * 3;
        self.0[i..i + 3].copy_from_slice(&rgb);
    }
    /// Высота отрыва: 0 — 1 мм, 1 — 2 мм, 2 — 0,7 мм (так кодирует драйвер).
    pub fn lod(&self) -> u8 {
        self.0[3] >> 4
    }
    pub fn set_lod(&mut self, lod: u8) {
        self.0[3] = lod << 4 | (self.0[3] & 1);
    }
    pub fn ripple(&self) -> bool {
        self.0[3] & 1 != 0
    }
    pub fn set_ripple(&mut self, on: bool) {
        self.0[3] = (self.0[3] & 0xF0) | on as u8;
    }
    pub fn angle_snap(&self) -> bool {
        self.0[4] >> 4 & 1 != 0
    }
    pub fn set_angle_snap(&mut self, on: bool) {
        self.0[4] = (on as u8) << 4 | (self.0[4] & 1);
    }
    pub fn motion_sync(&self) -> bool {
        self.0[4] & 1 != 0
    }
    pub fn set_motion_sync(&mut self, on: bool) {
        self.0[4] = (self.0[4] & 0xF0) | on as u8;
    }
    pub fn finish(&mut self) {
        put_checksum(&mut self.0, 50);
    }
}

/// Отчёт 5: подсветка DPI, таймеры сна и время отклика кнопок.
#[derive(Clone)]
pub struct Light(pub Vec<u8>);

impl Light {
    /// Подсветка уровня DPI: 0 — выключена, 5 — постоянно, 6 — дыхание.
    pub fn mode(&self) -> u8 {
        self.0[3] >> 4
    }
    pub fn set_mode(&mut self, mode: u8) {
        self.0[3] = mode << 4;
    }
    /// Глубокий сон, минуты: старшая тетрада лежит в байте 4, младшая — в байте 5.
    pub fn deep_sleep_min(&self) -> u32 {
        (self.0[4] & 0xF0 | self.0[5] >> 4) as u32
    }
    pub fn set_deep_sleep_min(&mut self, min: u32) {
        let m = min as u8;
        self.0[4] = (m & 0xF0) | (self.0[4] & 0x0F);
        self.0[5] = (m & 0x0F) << 4 | (self.0[5] & 0x0F);
    }
    pub fn light_sleep_s(&self) -> u32 {
        self.0[9] as u32 * 10
    }
    pub fn set_light_sleep_s(&mut self, s: u32) {
        self.0[9] = (s / 10) as u8;
    }
    pub fn debounce_ms(&self) -> u32 {
        self.0[10] as u32 * 2
    }
    pub fn set_debounce_ms(&mut self, ms: u32) {
        self.0[10] = (ms / 2) as u8;
    }
    pub fn finish(&mut self) {
        put_checksum(&mut self.0, 11);
    }
}

/// Отчёт 6: частота опроса. Код: 1 — 1000 Гц, 2 — 500, 4 — 250, 8 — 125.
#[derive(Clone)]
pub struct Rate(pub Vec<u8>);

impl Rate {
    pub fn hz(&self) -> u32 {
        match self.0[3] {
            8 => 125,
            4 => 250,
            2 => 500,
            _ => 1000,
        }
    }
    pub fn set_hz(&mut self, hz: u32) {
        let code = (1000 / hz) as u8;
        self.0[3] = code;
        self.0[4] = !code;
    }
}

/// Слоты кнопок в отчёте 8: 18 штук по 3 байта `[функция, параметр, параметр]`.
pub const SLOT_LEFT: usize = 0;
pub const SLOT_RIGHT: usize = 1;
pub const SLOT_MIDDLE: usize = 2;
pub const SLOT_BACK: usize = 3;
pub const SLOT_FORWARD: usize = 4;
pub const SLOT_DPI: usize = 9;
pub const SLOT_WHEEL_DOWN: usize = 16;
pub const SLOT_WHEEL_UP: usize = 17;

/// Заводские функции кнопок (как в мыши из коробки).
pub const DEFAULTS: &[(usize, [u8; 3])] = &[
    (SLOT_LEFT, [2, 0, 0]),
    (SLOT_RIGHT, [3, 0, 0]),
    (SLOT_MIDDLE, [4, 0, 0]),
    (SLOT_BACK, [5, 0, 0]),
    (SLOT_FORWARD, [6, 0, 0]),
    (SLOT_DPI, [13, 0, 0]),
    (SLOT_WHEEL_DOWN, [9, 0, 0]),
    (SLOT_WHEEL_UP, [10, 0, 0]),
];

pub fn default_function(slot: usize) -> Option<[u8; 3]> {
    DEFAULTS.iter().find(|d| d.0 == slot).map(|d| d.1)
}

#[derive(Clone)]
pub struct Buttons(pub Vec<u8>);

impl Buttons {
    pub fn slot(&self, slot: usize) -> [u8; 3] {
        let i = 3 + slot * 3;
        [self.0[i], self.0[i + 1], self.0[i + 2]]
    }
    pub fn set_slot(&mut self, slot: usize, value: [u8; 3]) {
        let i = 3 + slot * 3;
        self.0[i..i + 3].copy_from_slice(&value);
    }
    pub fn finish(&mut self) {
        put_checksum(&mut self.0, 57);
    }
}

pub const FN_SHORTCUT: u8 = 17;
pub const FN_MACRO: u8 = 18;

/// Готовые функции кнопок: (код, название по-русски, по-английски).
pub const FUNCTIONS: &[([u8; 3], &str, &str)] = &[
    ([2, 0, 0], "Левая кнопка", "Left click"),
    ([3, 0, 0], "Правая кнопка", "Right click"),
    ([4, 0, 0], "Средняя кнопка", "Middle click"),
    ([5, 0, 0], "Назад", "Back"),
    ([6, 0, 0], "Вперёд", "Forward"),
    ([7, 0, 0], "Двойной клик", "Double click"),
    ([8, 0, 0], "Автоклик", "Auto click"),
    ([10, 0, 0], "Прокрутка вверх", "Scroll up"),
    ([9, 0, 0], "Прокрутка вниз", "Scroll down"),
    ([13, 0, 0], "Цикл DPI", "DPI cycle"),
    ([14, 0, 0], "DPI +", "DPI +"),
    ([15, 0, 0], "DPI −", "DPI −"),
    ([24, 0, 0], "Пауза / play", "Play / pause"),
    ([23, 0, 0], "Следующий трек", "Next track"),
    ([22, 0, 0], "Предыдущий трек", "Previous track"),
    ([25, 0, 0], "Стоп", "Stop"),
    ([27, 0, 0], "Громче", "Volume up"),
    ([28, 0, 0], "Тише", "Volume down"),
    ([26, 0, 0], "Без звука", "Mute"),
    ([17, 1, 6], "Копировать", "Copy"),
    ([17, 1, 25], "Вставить", "Paste"),
    ([17, 1, 27], "Вырезать", "Cut"),
    ([17, 1, 29], "Отменить", "Undo"),
    ([17, 1, 28], "Повторить", "Redo"),
    ([33, 0, 0], "Браузер: назад", "Browser: back"),
    ([32, 0, 0], "Браузер: вперёд", "Browser: forward"),
    ([36, 0, 0], "Браузер: обновить", "Browser: refresh"),
    ([37, 0, 0], "Браузер: домой", "Browser: home"),
    ([38, 0, 0], "Браузер: поиск", "Browser: search"),
    ([29, 0, 0], "Калькулятор", "Calculator"),
    ([30, 0, 0], "Почта", "Mail"),
    ([1, 0, 0], "Отключить", "Disabled"),
];

pub const MOD_CTRL: u8 = 1;
pub const MOD_SHIFT: u8 = 2;
pub const MOD_ALT: u8 = 4;
pub const MOD_SUPER: u8 = 8;

/// Название функции кнопки для подписи.
pub fn function_name(value: [u8; 3]) -> String {
    if let Some(&(_, ru, en)) = FUNCTIONS.iter().find(|(v, _, _)| *v == value) {
        return tr(ru, en).to_string();
    }
    match value[0] {
        0 => tr("Не назначено", "Not assigned").into(),
        FN_SHORTCUT => shortcut_name(value[1], value[2]),
        FN_MACRO => tr("Макрос", "Macro").into(),
        _ => format!("{} {:02x} {:02x} {:02x}", tr("Код", "Code"), value[0], value[1], value[2]),
    }
}

pub fn shortcut_name(mods: u8, usage: u8) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for (bit, name) in [(MOD_CTRL, "Ctrl"), (MOD_SHIFT, "Shift"), (MOD_ALT, "Alt"), (MOD_SUPER, "Super")] {
        if mods & bit != 0 {
            parts.push(name);
        }
    }
    let key = crate::keys::usage_name(usage);
    parts.push(&key);
    parts.join(" + ")
}

/// Событие макроса: нажатие или отпускание клавиши и пауза перед ним.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct MacroEvent {
    pub usage: u8,
    pub release: bool,
    pub delay_ms: u32,
}

pub const MACRO_MAX_EVENTS: usize = 50;
pub const MACRO_MAX_DELAY_MS: u32 = 1270;

/// Режим повтора макроса: 0 — N раз, 1 — автоповтор, 2 — пока зажата кнопка.
pub fn build_macro(index: u8, mode: u8, count: u8, events: &[MacroEvent]) -> Vec<u8> {
    let mut buf = vec![0u8; 131];
    buf[0] = MACRO;
    buf[1] = 131;
    buf[2] = index;
    buf[3] = mode;
    buf[7] = if mode == 0 { count.max(1) } else { 1 };
    let events = &events[..events.len().min(MACRO_MAX_EVENTS)];
    buf[28] = events.len() as u8;
    let mut i = 29;
    for e in events {
        let d = (e.delay_ms.min(MACRO_MAX_DELAY_MS) / 10) as u8;
        // Так кодирует драйвер: отпускание — старший бит, у нажатия выставлен младший.
        buf[i] = if e.release { d | 0x80 } else { d | 1 };
        buf[i + 1] = e.usage;
        i += 2;
    }
    put_checksum(&mut buf, 129);
    buf
}

/// Обратная сборка макроса из прочитанного отчёта; пустой слот мыши заполнен 0xff.
pub fn parse_macro(buf: &[u8]) -> Option<(u8, u8, Vec<MacroEvent>)> {
    if buf.len() < 131 || buf[0] != MACRO || buf[3] == 0xFF {
        return None;
    }
    let n = buf[28] as usize;
    let mut events = Vec::new();
    let mut i = 29;
    while events.len() < n && i + 1 < 129 {
        let (b, usage) = (buf[i], buf[i + 1]);
        i += 2;
        // У нажатия младший бит паузы всегда 1, так что пауза точна до 10 мс.
        events.push(MacroEvent { usage, release: b & 0x80 != 0, delay_ms: (b & 0x7F) as u32 * 10 });
    }
    Some((buf[3], buf[7], events))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macro_matches_driver_encoding() {
        let events = [
            MacroEvent { usage: 0xE1, release: false, delay_ms: 10 },
            MacroEvent { usage: 0xE1, release: true, delay_ms: 50 },
        ];
        let m = build_macro(4, 0, 1, &events);
        // Байты сняты с мыши после записи тем же кодированием (16.10.2026).
        assert_eq!(&m[..33], &[9, 0x83, 4, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 1, 0xE1, 0x85, 0xE1]);
        assert_eq!(&m[129..], &[0x02, 0x4B]);
        let (mode, count, back) = parse_macro(&m).unwrap();
        assert_eq!((mode, count), (0, 1));
        assert_eq!(back, events);
    }

    #[test]
    fn settings_from_mouse() {
        // Отчёты, прочитанные с мыши Юто 16.10.2026.
        let dpi = Dpi(hex("0438010101 3f0000 90 20 b0 40 60 80 00 00 01 03 04 06 09 0c 00 00 02 ff0000 00ff00 0000ff 00ffff ffff00 ff00ff ff4000 ffffff 02 101b 00000000"));
        assert_eq!((0..6).map(|i| dpi.value(i)).collect::<Vec<_>>(), [400, 800, 1200, 1600, 2400, 3200]);
        assert_eq!(dpi.active(), 1);
        assert!(dpi.ripple() && dpi.motion_sync() && !dpi.angle_snap());
        let mut d = dpi.clone();
        d.finish();
        assert_eq!(d.0, dpi.0, "контрольная сумма DPI");
        let light = Light(hex("050f010003580 0ff000104015f0000"));
        assert_eq!((light.mode(), light.deep_sleep_min(), light.light_sleep_s(), light.debounce_ms()), (0, 5, 10, 8));
        let mut l = light.clone();
        l.set_deep_sleep_min(5);
        l.finish();
        assert_eq!(l.0, light.0, "контрольная сумма подсветки");
        let buttons = Buttons(hex("083b01 020000 030000 040000 050000 060000 000000 000000 000000 000000 0d0000 000000 000000 000000 000000 000000 000000 090000 0a0000 0034"));
        let mut b = buttons.clone();
        b.finish();
        assert_eq!(b.0, buttons.0, "контрольная сумма кнопок");
        assert_eq!(function_name(buttons.slot(SLOT_DPI)), tr("Цикл DPI", "DPI cycle"));
        for &(slot, value) in DEFAULTS {
            assert_eq!(buttons.slot(slot), value, "заводская функция слота {slot}");
        }
        assert_eq!(Rate(hex("06090101fe00000000")).hz(), 1000);
    }

    fn hex(s: &str) -> Vec<u8> {
        let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }
}
