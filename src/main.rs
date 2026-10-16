mod device;
mod i18n;
mod keys;
mod macro_editor;
mod mouse_view;
mod protocol;
mod window;

use adw::prelude::*;
use gtk::{gdk, glib};

pub const APP_ID: &str = "io.github.f0rd2017.AjazzMouse";

const CSS: &str = "
:root {
  --window-bg-color: #111216;
  --view-bg-color: #16171c;
  --headerbar-bg-color: #111216;
  --headerbar-shade-color: transparent;
  --card-bg-color: #1b1d23;
  --popover-bg-color: #1d1f25;
  --dialog-bg-color: #16171c;
  --accent-bg-color: #5b7dfa;
  --accent-color: #93a8ff;
  --accent-fg-color: #ffffff;
}
.mouse-stage {
  background: radial-gradient(circle at 50% 38%, #262932 0%, #181a20 55%, #121318 100%);
  border-radius: 28px;
  border: 1px solid alpha(white, 0.05);
  padding: 18px 12px 18px 12px;
}
.dpi-chip {
  background: alpha(white, 0.06);
  border-radius: 999px;
  padding: 6px 14px;
}
.dpi-chip-label { font-weight: 600; }
.battery-pill {
  background: alpha(white, 0.06);
  border-radius: 999px;
  padding: 4px 12px;
}
.battery { font-weight: 600; }
.compact row > box.header { min-height: 36px; padding-top: 2px; padding-bottom: 2px; }
.compact row { min-height: 0; }
.compact spinbutton > button { min-width: 26px; min-height: 26px; padding: 0; }
.dpi-color > button { padding: 3px; min-width: 0; min-height: 0; border-radius: 999px; }
.dpi-color swatch { min-width: 18px; min-height: 18px; border-radius: 999px; }
";

fn main() -> glib::ExitCode {
    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_startup(|_| {
        adw::StyleManager::default().set_color_scheme(adw::ColorScheme::ForceDark);
        gtk::Window::set_default_icon_name(APP_ID);
        let provider = gtk::CssProvider::new();
        provider.load_from_string(CSS);
        gtk::style_context_add_provider_for_display(
            &gdk::Display::default().expect("дисплей"),
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    });
    app.connect_activate(|app| match app.active_window() {
        Some(w) => w.present(),
        None => window::build(app),
    });
    app.run()
}
