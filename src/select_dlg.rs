use eframe::{egui, emath};
use std::path;

#[derive(Default)]
pub struct SelectDlg {
  info: Option<(path::PathBuf, Vec<path::PathBuf>)>,
}

impl SelectDlg {
  pub fn open(path: path::PathBuf, files: Vec<path::PathBuf>) -> Self {
    Self {
      info: Some((path, files)),
    }
  }

  pub fn show(&mut self, ctx: &egui::Context) -> Option<(path::PathBuf, path::PathBuf)> {
    let mut selection = None;
    if let Some((path, files)) = &self.info {
      egui::Window::new(egui::RichText::from("🌐  Select").strong())
        .collapsible(false)
        .resizable(false)
        .anchor(emath::Align2::CENTER_CENTER, [0.0, 0.0])
        .default_width(200.0)
        .show(ctx, |ui| {
          ui.add_space(8.0);
          ui.vertical_centered(|ui| {
            for file in files {
              ui.horizontal(|ui| {
                let text = file.file_stem().unwrap().to_str().unwrap();
                let button = egui::Button::new(text);
                if ui.add_sized(ui.available_size(), button).clicked() {
                  selection = Some((path.clone(), file.clone()));
                }
              });
            }
          });
        });
    }

    if selection.is_some() {
      self.info = None;
    }

    selection
  }
}
