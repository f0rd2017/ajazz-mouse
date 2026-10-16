//! Клавиши: код evdev (то, что GTK отдаёт как hardware keycode − 8) ↔ HID usage и подпись.

/// (evdev, HID usage, подпись)
const KEYS: &[(u32, u8, &str)] = &[
    (1, 0x29, "Esc"),
    (2, 0x1E, "1"),
    (3, 0x1F, "2"),
    (4, 0x20, "3"),
    (5, 0x21, "4"),
    (6, 0x22, "5"),
    (7, 0x23, "6"),
    (8, 0x24, "7"),
    (9, 0x25, "8"),
    (10, 0x26, "9"),
    (11, 0x27, "0"),
    (12, 0x2D, "-"),
    (13, 0x2E, "="),
    (14, 0x2A, "Backspace"),
    (15, 0x2B, "Tab"),
    (16, 0x14, "Q"),
    (17, 0x1A, "W"),
    (18, 0x08, "E"),
    (19, 0x15, "R"),
    (20, 0x17, "T"),
    (21, 0x1C, "Y"),
    (22, 0x18, "U"),
    (23, 0x0C, "I"),
    (24, 0x12, "O"),
    (25, 0x13, "P"),
    (26, 0x2F, "["),
    (27, 0x30, "]"),
    (28, 0x28, "Enter"),
    (29, 0xE0, "Ctrl"),
    (30, 0x04, "A"),
    (31, 0x16, "S"),
    (32, 0x07, "D"),
    (33, 0x09, "F"),
    (34, 0x0A, "G"),
    (35, 0x0B, "H"),
    (36, 0x0D, "J"),
    (37, 0x0E, "K"),
    (38, 0x0F, "L"),
    (39, 0x33, ";"),
    (40, 0x34, "'"),
    (41, 0x35, "`"),
    (42, 0xE1, "Shift"),
    (43, 0x31, "\\"),
    (44, 0x1D, "Z"),
    (45, 0x1B, "X"),
    (46, 0x06, "C"),
    (47, 0x19, "V"),
    (48, 0x05, "B"),
    (49, 0x11, "N"),
    (50, 0x10, "M"),
    (51, 0x36, ","),
    (52, 0x37, "."),
    (53, 0x38, "/"),
    (54, 0xE5, "Right Shift"),
    (55, 0x55, "Num *"),
    (56, 0xE2, "Alt"),
    (57, 0x2C, "Space"),
    (58, 0x39, "Caps Lock"),
    (59, 0x3A, "F1"),
    (60, 0x3B, "F2"),
    (61, 0x3C, "F3"),
    (62, 0x3D, "F4"),
    (63, 0x3E, "F5"),
    (64, 0x3F, "F6"),
    (65, 0x40, "F7"),
    (66, 0x41, "F8"),
    (67, 0x42, "F9"),
    (68, 0x43, "F10"),
    (69, 0x53, "Num Lock"),
    (70, 0x47, "Scroll Lock"),
    (71, 0x5F, "Num 7"),
    (72, 0x60, "Num 8"),
    (73, 0x61, "Num 9"),
    (74, 0x56, "Num -"),
    (75, 0x5C, "Num 4"),
    (76, 0x5D, "Num 5"),
    (77, 0x5E, "Num 6"),
    (78, 0x57, "Num +"),
    (79, 0x59, "Num 1"),
    (80, 0x5A, "Num 2"),
    (81, 0x5B, "Num 3"),
    (82, 0x62, "Num 0"),
    (83, 0x63, "Num ."),
    (86, 0x64, "<>"),
    (87, 0x44, "F11"),
    (88, 0x45, "F12"),
    (96, 0x58, "Num Enter"),
    (97, 0xE4, "Right Ctrl"),
    (98, 0x54, "Num /"),
    (99, 0x46, "Print Screen"),
    (100, 0xE6, "Right Alt"),
    (102, 0x4A, "Home"),
    (103, 0x52, "↑"),
    (104, 0x4B, "Page Up"),
    (105, 0x50, "←"),
    (106, 0x4F, "→"),
    (107, 0x4D, "End"),
    (108, 0x51, "↓"),
    (109, 0x4E, "Page Down"),
    (110, 0x49, "Insert"),
    (111, 0x4C, "Delete"),
    (119, 0x48, "Pause"),
    (125, 0xE3, "Super"),
    (126, 0xE7, "Right Super"),
    (127, 0x65, "Menu"),
    (183, 0x68, "F13"),
    (184, 0x69, "F14"),
    (185, 0x6A, "F15"),
    (186, 0x6B, "F16"),
    (187, 0x6C, "F17"),
    (188, 0x6D, "F18"),
    (189, 0x6E, "F19"),
    (190, 0x6F, "F20"),
    (191, 0x70, "F21"),
    (192, 0x71, "F22"),
    (193, 0x72, "F23"),
    (194, 0x73, "F24"),
];

pub fn usage_from_keycode(hardware_keycode: u32) -> Option<u8> {
    let evdev = hardware_keycode.checked_sub(8)?;
    KEYS.iter().find(|k| k.0 == evdev).map(|k| k.1)
}

/// Клавиши, которые по-русски подписаны иначе.
fn russian_name(usage: u8) -> Option<&'static str> {
    Some(match usage {
        0x2C => "Пробел",
        0xE4 => "Ctrl справа",
        0xE5 => "Shift справа",
        0xE6 => "Alt справа",
        0xE7 => "Super справа",
        _ => return None,
    })
}

pub fn usage_name(usage: u8) -> String {
    if crate::i18n::russian()
        && let Some(name) = russian_name(usage)
    {
        return name.into();
    }
    KEYS.iter()
        .find(|k| k.1 == usage)
        .map(|k| k.2.to_string())
        .unwrap_or_else(|| format!("0x{usage:02x}"))
}

/// Модификаторы HID (0xE0…0xE7) в сочетании задаются битами, а не клавишей.
pub fn is_modifier(usage: u8) -> bool {
    (0xE0..=0xE7).contains(&usage)
}
