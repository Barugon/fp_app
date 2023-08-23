use crate::util;
use eframe::{egui, emath, epaint};

#[derive(Default)]
pub struct SelectMenu {
  pos: emath::Pos2,
  org: emath::Pos2,
  width: f32,
}

impl SelectMenu {
  fn add_btn(&mut self, ui: &mut egui::Ui, text: &str) -> egui::Response {
    let layout = egui::Layout::left_to_right(emath::Align::Center);
    ui.allocate_ui_with_layout(emath::vec2(0.0, 0.0), layout, |ui| {
      let style = ui.style_mut();
      style.spacing.button_padding = epaint::vec2(2.0, 0.0);
      style.visuals.widgets.active.bg_stroke = epaint::Stroke::NONE;
      style.visuals.widgets.hovered.bg_stroke = epaint::Stroke::NONE;
      style.visuals.widgets.inactive.weak_bg_fill = epaint::Color32::TRANSPARENT;
      style.visuals.widgets.inactive.bg_stroke = epaint::Stroke::NONE;

      // Make all the buttons the same width.
      let widget = egui::Button::new(text);
      let size = emath::vec2(self.width, style.spacing.interact_size.y);
      let response = ui.add_sized(size, widget);
      self.width = response.rect.width();

      response
    })
    .inner
  }
}

impl SelectMenu {
  pub fn set_pos(&mut self, pos: emath::Pos2) {
    self.width = 218.0;
    self.pos = emath::pos2(pos.x - self.width * 0.5, pos.y);
    self.org = pos;
  }

  pub fn show<'a, I: Iterator<Item = &'a str>>(
    &mut self,
    ctx: &egui::Context,
    lat_lon: &str,
    choices: Option<I>,
  ) -> Option<Response> {
    let mut selection = None;
    let response = egui::Area::new("select_menu")
      .order(egui::Order::Foreground)
      .fixed_pos(self.pos)
      .show(ctx, |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
          if self.add_btn(ui, lat_lon).clicked() {
            selection = Some(Response::LatLon);
          }

          if let Some(choices) = choices {
            ui.add_sized([self.width, 1.0], egui::Separator::default().spacing(2.0));
            for (index, choice) in choices.enumerate() {
              if self.add_btn(ui, choice).clicked() {
                selection = Some(Response::Index(index));
              }
            }
          }
        });
      })
      .response;

    // If the user clicked off then return Response::Close.
    if response.clicked_elsewhere() {
      selection = Some(Response::Close);
    } else {
      // Center the popup and make sure it doesn't go past the window's edges.
      let available = ctx.available_rect();
      let size = response.rect.size();
      let min = emath::pos2(self.org.x - size.x * 0.5, self.org.y);
      let max = emath::pos2(min.x + size.x, min.y + size.y);
      let mut rect = emath::Rect::from_min_max(min, max);

      // Right.
      if rect.max.x > available.max.x {
        rect = rect.translate(emath::vec2(available.max.x - rect.max.x, 0.0));
      }

      // Left.
      if rect.min.x < available.min.x {
        rect = rect.translate(emath::vec2(available.min.x - rect.min.x, 0.0));
      }

      // Bottom.
      if rect.max.y > available.max.y {
        rect = rect.translate(emath::vec2(0.0, available.max.y - rect.max.y));
      }

      // Top.
      if rect.min.y < available.min.y {
        rect = rect.translate(emath::vec2(0.0, available.min.y - rect.min.y));
      }

      if util::Pos::from(rect.min) != util::Pos::from(self.pos) {
        self.pos = rect.min;
        ctx.request_repaint();
      }
    }

    selection
  }
}

pub enum Response {
  Close,
  LatLon,
  Index(usize),
}
