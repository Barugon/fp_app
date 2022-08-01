use eframe::{egui, emath};
use std::path::PathBuf;

#[derive(Default)]
pub struct SelectDlg;

impl SelectDlg {
  pub fn show(&mut self, ctx: &egui::Context, choices: Choices) -> Option<Response> {
    let mut selection = None;
    let mut open = true;
    egui::Window::new(egui::RichText::from("🌐  Select").strong())
      .open(&mut open)
      .collapsible(false)
      .resizable(false)
      .anchor(emath::Align2::CENTER_CENTER, [0.0, 0.0])
      .default_width(200.0)
      .show(ctx, |ui| {
        ui.add_space(8.0);
        ui.vertical_centered(|ui| match &choices {
          Choices::Paths(paths) => {
            for (index, path) in paths.iter().enumerate() {
              ui.horizontal(|ui| {
                let text = path.file_stem().unwrap().to_str().unwrap();
                let button = egui::Button::new(text);
                if ui.add_sized(ui.available_size(), button).clicked() {
                  selection = Some(Response::Index(index));
                }
              });
            }
          }
          Choices::Strings(strings) => {
            for (index, string) in strings.iter().enumerate() {
              ui.horizontal(|ui| {
                let button = egui::Button::new(string);
                if ui.add_sized(ui.available_size(), button).clicked() {
                  selection = Some(Response::Index(index));
                }
              });
            }
          }
        });

        ui.separator();

        if ui.button("Cancel").clicked() {
          selection = Some(Response::Cancel);
        }
      });

    if !open || ctx.input().key_pressed(egui::Key::Escape) {
      selection = Some(Response::Cancel);
    }

    selection
  }
}

pub enum Choices<'a> {
  Paths(&'a Vec<PathBuf>),
  Strings(&'a Vec<String>),
}

pub enum Response {
  Cancel,
  Index(usize),
}
