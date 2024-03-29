use eframe::{egui, emath};
use std::mem;

#[derive(Default)]
pub struct FindDlg {
  text: String,
  focus: bool,
}

#[derive(Eq, PartialEq)]
pub enum Response {
  None,
  Cancel,
  Term(String),
}

impl FindDlg {
  pub fn open() -> Self {
    Self {
      text: String::new(),
      focus: true,
    }
  }

  pub fn show(&mut self, ctx: &egui::Context) -> Response {
    let mut response = Response::None;
    let mut open = !ctx.input(|state| state.key_pressed(egui::Key::Escape));

    egui::Window::new(egui::RichText::from("🔎  Find").strong())
      .open(&mut open)
      .collapsible(false)
      .resizable(false)
      .anchor(emath::Align2::CENTER_CENTER, [0.0, 0.0])
      .default_width(150.0)
      .show(ctx, |ui| {
        ui.add_space(8.0);
        ui.horizontal(|ui| {
          let widget = egui::TextEdit::singleline(&mut self.text).hint_text("Airport ID or name");
          let edit_response = ui.add_sized(ui.available_size(), widget);
          if mem::take(&mut self.focus) {
            self.focus = false;
            edit_response.request_focus();
          }

          if edit_response.lost_focus() && ui.input(|state| state.key_pressed(egui::Key::Enter)) {
            response = Response::Term(mem::take(&mut self.text));
          }
        });
        ui.add_space(8.0);
        ui.separator();
        ui.horizontal(|ui| {
          ui.add_enabled_ui(!self.text.is_empty(), |ui| {
            if ui.button("Ok").clicked() {
              response = Response::Term(mem::take(&mut self.text));
            }
          });

          if ui.button("Cancel").clicked() {
            response = Response::Cancel;
          }
        });
      });

    if !open {
      response = Response::Cancel;
    }

    response
  }
}
