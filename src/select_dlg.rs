use eframe::{egui, emath};

#[derive(Default)]
pub struct SelectDlg;

impl SelectDlg {
  pub fn show(&mut self, ctx: &egui::Context, choices: Vec<String>) -> Option<Response> {
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
        ui.vertical_centered(|ui| {
          for (index, text) in choices.into_iter().enumerate() {
            ui.horizontal(|ui| {
              let button = egui::Button::new(text);
              if ui.add_sized(ui.available_size(), button).clicked() {
                selection = Some(Response::Index(index));
              }
            });
          }
        });
      });

    if !open || ctx.input().key_pressed(egui::Key::Escape) {
      selection = Some(Response::Cancel);
    }

    selection
  }
}

pub enum Response {
  Cancel,
  Index(usize),
}
