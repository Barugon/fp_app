// Don't show the console on Windows.
#![windows_subsystem = "windows"]

#[macro_use]
mod util;

mod app;
mod chart;
mod nasr;

use eframe::egui;
use std::env;

fn main() {
  let mut theme = None;
  let mut decorated = true;
  for arg in env::args() {
    match arg.as_str() {
      "--dark" => theme = Some(egui::Visuals::dark()),
      "--light" => theme = Some(egui::Visuals::light()),
      "--no-deco" => decorated = false,
      _ => (),
    }
  }

  eframe::run_native(
    env!("CARGO_PKG_NAME"),
    eframe::NativeOptions {
      decorated,
      ..Default::default()
    },
    Box::new(|cc| Box::new(app::App::new(cc, theme))),
  );
}
