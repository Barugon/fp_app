use eframe::{egui, emath};

#[derive(Default)]
pub struct SelectMenu {
  org: emath::Pos2,
  pos: Option<emath::Pos2>,
}

impl SelectMenu {
  pub fn set_pos(&mut self, pos: emath::Pos2) {
    self.org = pos;
    self.pos = Some(pos);
  }

  pub fn show(&mut self, ctx: &egui::Context, choices: &[String]) -> Option<Response> {
    let mut selection = None;
    if let Some(pos) = &mut self.pos {
      let response = egui::Area::new("choices")
        .order(egui::Order::Foreground)
        .fixed_pos(*pos)
        .show(ctx, |ui| {
          egui::Frame::popup(ui.style()).show(ui, |ui| {
            for (index, choice) in choices.iter().enumerate() {
              ui.horizontal(|ui| {
                if ui.selectable_label(false, choice).clicked() {
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

        // Make sure it's not too far left.
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
