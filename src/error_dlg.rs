use eframe::{egui, emath, epaint};

#[derive(Default)]
pub struct ErrorDlg {
  text: Option<String>,
}

impl ErrorDlg {
  pub fn open(text: String) -> Self {
    Self { text: Some(text) }
  }

  pub fn show(&mut self, ctx: &egui::Context) -> bool {
    if ctx.input().key_pressed(egui::Key::Enter) {
      self.text = None;
    } else if ctx.input().key_pressed(egui::Key::Escape) {
      self.text = None;
    }

    let mut open = self.text.is_some();
    egui::Window::new(egui::RichText::from("⚠  Open Error").strong())
      .open(&mut open)
      .collapsible(false)
      .resizable(false)
      .anchor(emath::Align2::CENTER_CENTER, [0.0, 0.0])
      .default_size([300.0, 0.0])
      .show(ctx, |ui| {
        ui.add_space(8.0);
        ui.vertical_centered(|ui| {
          let text =
            egui::RichText::from(self.text.as_ref().unwrap()).color(epaint::Color32::LIGHT_RED);
          ui.label(text);
        });
        ui.add_space(8.0);
        ui.separator();
        ui.horizontal(|ui| {
          if ui.button("Close").clicked() {
            self.text = None;
          }
        });
      });

    if self.text.is_none() && open {
      open = false;
    }

    open
  }
}
