// Don't show the console on Windows.
#![windows_subsystem = "windows"]

#[macro_use]
mod util;

mod app;
mod chart;
mod error_dlg;
mod nasr;
mod select_dlg;
mod select_menu;

use eframe::{egui, emath};
use std::env;

fn main() {
  let mut theme = None;
  let mut decorated = true;
  let mut sim = false;
  for arg in env::args() {
    match arg.as_str() {
      // Force dark them as default.
      "--dark" => {
        assert!(theme.is_none(), "theme specified more than once");
        theme = Some(egui::Visuals::dark());
      }

      // Force light them as default.
      "--light" => {
        assert!(theme.is_none(), "theme specified more than once");
        theme = Some(egui::Visuals::light());
      }

      // Create the window with no decorations (useful for small devices like phones).
      "--no-deco" => decorated = false,

      // Simulate what it would look like on a device like PinePhone or Librem 5.
      "--sim" => sim = true,
      _ => (),
    }
  }

  let (options, scale) = if sim {
    const INNER_SIZE: emath::Vec2 = emath::Vec2::new(540.0, 972.0);
    (
      eframe::NativeOptions {
        resizable: false,
        initial_window_size: Some(INNER_SIZE),
        max_window_size: Some(INNER_SIZE),
        min_window_size: Some(INNER_SIZE),
        decorated,
        ..Default::default()
      },
      Some(2.0 * 540.0 / 720.0),
    )
  } else {
    (
      if decorated {
        eframe::NativeOptions {
          min_window_size: Some(emath::Vec2::splat(480.0)),
          ..Default::default()
        }
      } else {
        eframe::NativeOptions {
          decorated,
          ..Default::default()
        }
      },
      None,
    )
  };

  eframe::run_native(
    env!("CARGO_PKG_NAME"),
    options,
    Box::new(move |cc| Box::new(app::App::new(cc, theme, scale))),
  );
}
