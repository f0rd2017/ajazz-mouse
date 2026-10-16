//! Язык интерфейса: русский при русской локали, иначе английский.

use std::sync::OnceLock;

/// Русская ли локаль сообщений — в порядке, в каком её ищет gettext.
pub fn russian() -> bool {
    static RU: OnceLock<bool> = OnceLock::new();
    *RU.get_or_init(|| {
        ["LANGUAGE", "LC_ALL", "LC_MESSAGES", "LANG"]
            .iter()
            .find_map(|v| std::env::var(v).ok().filter(|s| !s.is_empty()))
            .is_some_and(|s| s.starts_with("ru"))
    })
}

/// Подпись на языке интерфейса.
pub fn tr(ru: &'static str, en: &'static str) -> &'static str {
    if russian() { ru } else { en }
}
