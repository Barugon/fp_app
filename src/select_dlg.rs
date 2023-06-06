use eframe::{egui, emath};

#[derive(Default)]
pub struct SelectDlg;

impl SelectDlg {
  pub fn show<'a, I: Iterator<Item = &'a str>>(
    &mut self,
    ctx: &egui::Context,
    choices: I,
  ) -> Option<Response> {
    let mut selection = None;
    let mut open = true;
    egui::Window::new(egui::RichText::from("👉  Select").strong())
      .open(&mut open)
      .collapsible(false)
      .resizable(false)
      .anchor(emath::Align2::CENTER_CENTER, [0.0, 0.0])
      .default_width(200.0)
      .show(ctx, |ui| {
        ui.add_space(8.0);
        ui.vertical_centered(|ui| {
          for (index, text) in choices.enumerate() {
            ui.horizontal(|ui| {
              let button = egui::Button::new(text);
              if ui.add_sized(ui.available_size(), button).clicked() {
                selection = Some(Response::Index(index));
              }
            });
          }
        });
      });

    if !open || ctx.input(|state| state.key_pressed(egui::Key::Escape)) {
      selection = Some(Response::Close);
    }

    selection
  }
}

pub enum Response {
  Close,
  Index(usize),
}
