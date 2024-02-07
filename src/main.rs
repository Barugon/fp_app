// Don't show the console on Windows.
#![windows_subsystem = "windows"]

#[macro_use]
mod util;

mod app;
mod chart;
mod config;
mod error_dlg;
mod find_dlg;
mod nasr;
mod select_dlg;
mod select_menu;
mod touch;

use eframe::egui;
use std::env;

struct Opts {
  native: eframe::NativeOptions,
  theme: Option<egui::Visuals>,
  scale: Option<f32>,
  config: config::Storage,
}

fn parse_args() -> Opts {
  let mut sim = false;
  let mut theme = None;
  let mut deco = cfg!(not(feature = "phosh"));
  let icon = image::load_from_memory(util::APP_ICON).unwrap();
  let icon = egui::IconData {
    width: icon.width(),
    height: icon.height(),
    rgba: icon.into_rgba8().into_raw(),
  };

  for arg in env::args() {
    match arg.as_str() {
      // Force dark theme as default.
      "--dark" => theme = Some(egui::Visuals::dark()),

      // Force light theme as default.
      "--light" => theme = Some(egui::Visuals::light()),

      // Hide window decorations.
      "--no-deco" => deco = false,

      // Simulate what it would look like on a device like PinePhone or Librem 5.
      "--sim" => sim = true,
      _ => (),
    }
  }

  let config = config::Storage::new(deco && !sim).unwrap();
  let (viewport, scale) = {
    use eframe::emath;
    if sim {
      const INNER_SIZE: emath::Vec2 = emath::Vec2::new(540.0, 972.0);
      let viewport = egui::ViewportBuilder::default()
        .with_decorations(deco)
        .with_icon(icon)
        .with_inner_size(INNER_SIZE)
        .with_max_inner_size(INNER_SIZE)
        .with_min_inner_size(INNER_SIZE)
        .with_resizable(false);
      (viewport, Some(2.0 * 540.0 / 720.0))
    } else if deco {
      const MIN_SIZE: emath::Vec2 = emath::Vec2::new(540.0, 394.0);
      let win_info = config.get_win_info();
      let mut viewport = egui::ViewportBuilder::default()
        .with_icon(icon)
        .with_min_inner_size(MIN_SIZE)
        .with_maximized(win_info.maxed);
      if let Some(size) = win_info.size {
        viewport = viewport.with_inner_size(size);
      }
      (viewport, None)
    } else {
      let viewport = egui::ViewportBuilder::default()
        .with_decorations(false)
        .with_icon(icon);
      (viewport, None)
    }
  };

  let native = eframe::NativeOptions {
    viewport,
    ..Default::default()
  };

  Opts {
    native,
    theme,
    scale,
    config,
  }
}

fn main() {
  let opts = parse_args();
  eframe::run_native(
    &util::title_case(env!("CARGO_PKG_NAME")),
    opts.native,
    Box::new(move |cc| Box::new(app::App::new(cc, opts.theme, opts.scale, opts.config))),
  )
  .unwrap();
}
