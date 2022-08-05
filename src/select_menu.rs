use eframe::{egui, emath};

#[derive(Default)]
pub struct SelectMenu {
  metrics: Option<(emath::Pos2, f32)>,
}

impl SelectMenu {
  pub fn set_pos(&mut self, pos: emath::Pos2) {
    self.metrics = Some((pos, 0.0));
  }

  pub fn show(&mut self, ctx: &egui::Context, choices: &[String]) -> Option<Response> {
    let mut selection = None;
    if let Some((pos, width)) = &mut self.metrics {
      let response = egui::Area::new("choices")
        .order(egui::Order::Foreground)
        .fixed_pos(*pos)
        .show(ctx, |ui| {
          egui::Frame::popup(ui.style()).show(ui, |ui| {
            for (index, choice) in choices.iter().enumerate() {
              ui.horizontal(|ui| {
                let size = emath::Vec2::new(*width, ui.available_height());
                let layout = egui::Layout::centered_and_justified(egui::Direction::LeftToRight)
                  .with_main_align(emath::Align::Min);
                ui.allocate_ui_with_layout(size, layout, |ui| {
                  if ui.selectable_label(false, choice).clicked() {
                    selection = Some(Response::Index(index));
                  }
                });
              });
            }
          });
        })
        .response;

      // If the user clicked off then return Response::Close.
      if response.clicked_elsewhere() {
        selection = Some(Response::Close);
      } else {
        let style = ctx.style();
        let margin = style.spacing.window_margin.left + style.spacing.window_margin.right;

        // Grab the interior width.
        *width = (response.rect.width() - margin).max(0.0);

        // Make sure that the popup doesn't go past the window's edges.
        let available = ctx.available_rect();

        if response.rect.max.x > available.max.x {
          pos.x -= response.rect.max.x - available.max.x;
        }

        if response.rect.max.y > available.max.y {
          pos.y -= response.rect.max.y - available.max.y;
        }
      }
    }

    if selection.is_some() {
      self.metrics = None;
    }

    selection
  }
}

pub enum Response {
  Close,
  Index(usize),
}
