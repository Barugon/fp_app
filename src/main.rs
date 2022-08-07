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
      "--dark" => theme = Some(egui::Visuals::dark()),
      "--light" => theme = Some(egui::Visuals::light()),
      "--no-deco" => decorated = false,
      "--sim" => sim = true,
      _ => (),
    }
  }

  let (options, ppp) = if sim {
    // Simulate what it would look like on a device like PinePhone or Librem 5.
    const INNER_SIZE: emath::Vec2 = emath::Vec2::new(480.0, 900.0);
    (
      eframe::NativeOptions {
        resizable: false,
        initial_window_size: Some(INNER_SIZE),
        max_window_size: Some(INNER_SIZE),
        min_window_size: Some(INNER_SIZE),
        decorated,
        ..Default::default()
      },
      Some(2.0 * 480.0 / 720.0),
    )
  } else {
    (
      eframe::NativeOptions {
        decorated,
        ..Default::default()
      },
      None,
    )
  };

  eframe::run_native(
    env!("CARGO_PKG_NAME"),
    options,
    Box::new(move |cc| Box::new(app::App::new(cc, theme, ppp))),
  );
}
