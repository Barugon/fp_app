use std::mem;

use eframe::{egui, emath};

#[derive(Default)]
pub struct SelectDlg {
  reset: bool,
}

impl SelectDlg {
  pub fn show<'a, I: Iterator<Item = &'a str>>(
    &mut self,
    ctx: &egui::Context,
    choices: I,
  ) -> Option<Response> {
    let mut selection = None;
    let mut open = true;
    let win = egui::Window::new(egui::RichText::from("👉  Select").strong())
      .open(&mut open)
      .collapsible(false)
      .resizable(false)
      .anchor(emath::Align2::CENTER_CENTER, [0.0, 0.0]);

    // Hack to reset the window size.
    let win = if mem::take(&mut self.reset) {
      win.fixed_size([200.0, 500.0])
    } else {
      win
    };

    win.show(ctx, |ui| {
      ui.add_space(8.0);
      ui.vertical_centered(|ui| {
        egui::ScrollArea::vertical().show(ui, |ui| {
          for (index, text) in choices.enumerate() {
            ui.horizontal(|ui| {
              let widget = egui::SelectableLabel::new(false, text);
              if ui.add_sized(ui.available_size(), widget).clicked() {
                selection = Some(Response::Index(index));
              }
            });
          }
        });
      });
    });

    if !open || ctx.input(|state| state.key_pressed(egui::Key::Escape)) {
      selection = Some(Response::Close);
    }

    self.reset = selection.is_some();
    selection
  }
}

pub enum Response {
  Close,
  Index(usize),
}
