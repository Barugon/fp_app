use eframe::{egui, emath, epaint};

#[derive(Default)]
pub struct ErrorDlg {
  text: Option<String>,
  reset: bool,
}

impl ErrorDlg {
  pub fn open(text: String) -> Self {
    Self {
      text: Some(text),
      reset: true,
    }
  }

  pub fn show(&mut self, ctx: &egui::Context) -> bool {
    if ctx
      .input(|state| state.key_pressed(egui::Key::Enter) || state.key_pressed(egui::Key::Escape))
    {
      self.text = None;
    }

    let mut open = self.text.is_some();
    let win = egui::Window::new(egui::RichText::from("⚠  Error").strong())
      .open(&mut open)
      .collapsible(false)
      .resizable(false)
      .anchor(emath::Align2::CENTER_CENTER, [0.0, 0.0]);

    // Hack to reset the window size.
    let win = if self.reset {
      self.reset = false;
      win.fixed_size([200.0, 20.0])
    } else {
      win
    };

    win.show(ctx, |ui| {
      ui.add_space(8.0);
      ui.vertical_centered(|ui| {
        let text = egui::RichText::from(self.text.as_ref().unwrap());
        let widget = egui::Label::new(text.color(epaint::Color32::LIGHT_RED)).wrap(false);
        ui.add(widget);
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
