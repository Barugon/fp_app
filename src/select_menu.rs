use eframe::{egui, emath, epaint};

#[derive(Default)]
pub struct SelectMenu {
  org: emath::Pos2,
  pos: Option<emath::Pos2>,
  width: f32,
}

impl SelectMenu {
  pub fn set_pos(&mut self, pos: emath::Pos2) {
    self.org = pos;
    self.pos = Some(pos);
    self.width = 0.0;
  }

  pub fn show(&mut self, ctx: &egui::Context, choices: &[String]) -> Option<Response> {
    let mut selection = None;
    if let Some(pos) = &mut self.pos {
      let response = egui::Area::new("select_menu")
        .order(egui::Order::Foreground)
        .fixed_pos([pos.x - self.width * 0.5, pos.y])
        .show(ctx, |ui| {
          egui::Frame::popup(ui.style()).show(ui, |ui| {
            for (index, choice) in choices.iter().enumerate() {
              if index == 1 {
                // ui.add_space(1.0);
                ui.add_sized([self.width, 1.0], egui::Separator::default().spacing(2.0));
              }

              let layout = egui::Layout::left_to_right(emath::Align::Center);
              ui.allocate_ui_with_layout(emath::vec2(0.0, 0.0), layout, |ui| {
                let style = ui.style_mut();
                style.spacing.button_padding = epaint::vec2(2.0, 0.0);
                style.visuals.widgets.active.bg_stroke = epaint::Stroke::NONE;
                style.visuals.widgets.hovered.bg_stroke = epaint::Stroke::NONE;
                style.visuals.widgets.inactive.weak_bg_fill = epaint::Color32::TRANSPARENT;
                style.visuals.widgets.inactive.bg_stroke = epaint::Stroke::NONE;

                // Make all the buttons the same width.
                let widget = egui::Button::new(choice);
                let size = emath::vec2(self.width, style.spacing.interact_size.y);
                let response = ui.add_sized(size, widget);
                self.width = response.rect.width();

                if response.clicked() {
                  selection = Some(Response::Index(index));
                }
              });
            }
          });
        })
        .response;

      // If the user clicked off then return Response::Close.
      if response.clicked_elsewhere() {
        selection = Some(Response::Close);
      } else {
        // Make sure that the popup doesn't go past the window's edges.
        let available = ctx.available_rect();
        let mut changed = false;

        if response.rect.max.x > available.max.x {
          pos.x -= response.rect.max.x - available.max.x;
          if pos.x < 0.0 {
            pos.x = 0.0;
          }
          changed = true;
        }

        // Make sure it's not too far left (this can happen if a previous menu was wider than this one).
        if pos.x < self.org.x && response.rect.max.x < available.max.x {
          pos.x += (self.org.x - pos.x).min(available.max.x - response.rect.max.x);
          changed = true;
        }

        if response.rect.max.y > available.max.y {
          pos.y -= response.rect.max.y - available.max.y;
          if pos.y < 0.0 {
            pos.y = 0.0;
          }
          changed = true;
        }

        if changed {
          ctx.request_repaint();
        }
      }
    }

    if selection.is_some() {
      self.pos = None;
    }

    selection
  }
}

pub enum Response {
  Close,
  Index(usize),
}
