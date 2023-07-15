// Don't show the console on Windows.
#![windows_subsystem = "windows"]

#[macro_use]
mod util;

mod app;
mod chart;
mod error_dlg;
mod find_dlg;
mod nasr;
mod select_dlg;
mod select_menu;
mod touch;

use eframe::{egui, emath};
use std::env;
use util::Wrest;

struct Opts {
  native: eframe::NativeOptions,
  theme: Option<egui::Visuals>,
  scale: Option<f32>,
}

fn parse_args() -> Opts {
  let mut theme = None;
  let mut deco = true;
  let mut sim = false;
  let icon = image::load_from_memory(util::APP_ICON).wrest();
  let icon_data = Some(eframe::IconData {
    width: icon.width(),
    height: icon.height(),
    rgba: icon.into_rgba8().into_raw(),
  });

  for arg in env::args() {
    match arg.as_str() {
      // Force dark theme as default.
      "--dark" => theme = Some(egui::Visuals::dark()),

      // Force light theme as default.
      "--light" => theme = Some(egui::Visuals::light()),

      // Create the window with no decorations (useful for small devices like phones).
      "--no-deco" => deco = false,

      // Simulate what it would look like on a device like PinePhone or Librem 5.
      "--sim" => sim = true,
      _ => (),
    }
  }

  let (native, scale) = if sim {
    const INNER_SIZE: emath::Vec2 = emath::Vec2::new(540.0, 972.0);
    (
      eframe::NativeOptions {
        decorated: deco,
        icon_data,
        initial_window_size: Some(INNER_SIZE),
        max_window_size: Some(INNER_SIZE),
        min_window_size: Some(INNER_SIZE),
        resizable: false,
        ..Default::default()
      },
      Some(2.0 * 540.0 / 720.0),
    )
  } else if deco {
    const INNER_SIZE: emath::Vec2 = emath::Vec2::new(540.0, 394.0);
    (
      eframe::NativeOptions {
        icon_data,
        min_window_size: Some(INNER_SIZE),
        ..Default::default()
      },
      None,
    )
  } else {
    (
      eframe::NativeOptions {
        decorated: false,
        icon_data,
        ..Default::default()
      },
      None,
    )
  };

  Opts {
    native,
    theme,
    scale,
  }
}

fn main() {
  let opts = parse_args();
  eframe::run_native(
    env!("CARGO_PKG_NAME"),
    opts.native,
    Box::new(move |cc| Box::new(app::App::new(cc, opts.theme, opts.scale))),
  )
  .wrest();
}
