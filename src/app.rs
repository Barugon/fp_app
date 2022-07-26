use crate::{chart, util};
use eframe::{egui, emath, epaint};
use egui_extras;
use egui_file;
use std::{borrow, collections, path, sync};

pub struct App {
  default_theme: egui::Visuals,
  chart_reader: chart::AsyncReader,
  file_dlg: Option<egui_file::FileDialog>,
  chart: Chart,
  error: Option<borrow::Cow<'static, str>>,
  night_mode: bool,
  side_panel: bool,
  ui_enabled: bool,
}

impl App {
  pub fn new(cc: &eframe::CreationContext, theme: Option<egui::Visuals>) -> Self {
    if let Some(theme) = theme {
      cc.egui_ctx.set_visuals(theme);
    }

    let mut style = (*cc.egui_ctx.style()).clone();
    if style.visuals.dark_mode {
      // Make the "extreme" background color somewhat less extreme.
      style.visuals.extreme_bg_color = epaint::Color32::from_gray(20)
    }

    // Make the fonts a bit bigger.
    for font_id in style.text_styles.values_mut() {
      font_id.size *= 1.1;
    }

    let default_theme = style.visuals.clone();
    cc.egui_ctx.set_style(style);

    // Create the chart reader.
    let ctx = cc.egui_ctx.clone();
    let chart_reader = chart::AsyncReader::new("Chart Reader", move || ctx.request_repaint());

    // If we're starting in night mode then set the dark theme.
    let night_mode = to_bool(cc.storage.unwrap().get_string(NIGHT_MODE_KEY));
    if night_mode {
      cc.egui_ctx.set_visuals(dark_theme());
    }

    Self {
      default_theme,
      chart_reader,
      file_dlg: None,
      chart: Chart::None,
      error: None,
      night_mode,
      side_panel: false,
      ui_enabled: true,
    }
  }

  fn select_chart_zip(&mut self) {
    let path = some!(dirs::download_dir());
    let mut file_dlg = egui_file::FileDialog::open_file(Some(path))
      .filter("zip".into())
      .show_new_folder(false)
      .show_rename(false)
      .resizable(false);
    file_dlg.open();
    self.file_dlg = Some(file_dlg);
  }

  fn open_chart(&mut self, path: &path::Path, file: &path::Path) {
    match self.chart_reader.open(&path, &file) {
      Ok(transform) => {
        self.chart = Chart::Ready {
          name: file.file_stem().unwrap().to_str().unwrap().into(),
          transform: sync::Arc::new(transform),
          image: None,
          requests: collections::HashSet::new(),
          scroll: Some(emath::Pos2::new(0.0, 0.0)),
          zoom: 1.0,
        };
      }
      Err(err) => self.error = Some(format!("Unable to open chart: {:?}", err).into()),
    }
  }

  fn request_image(&mut self, rect: util::Rect, zoom: f32) {
    let dark = self.night_mode;
    let part = chart::ImagePart::new(rect, zoom, dark);
    if self.insert_chart_request(part.clone()) {
      self.chart_reader.read_image(part);
    }
  }

  fn get_chart_name(&self) -> Option<&str> {
    if let Chart::Ready { name, .. } = &self.chart {
      return Some(name);
    }
    None
  }

  fn get_chart_transform(&self) -> Option<sync::Arc<chart::Transform>> {
    if let Chart::Ready { transform, .. } = &self.chart {
      return Some(transform.clone());
    }
    None
  }

  fn get_chart_part(&self) -> Option<&chart::ImagePart> {
    if let Chart::Ready {
      image: Some(image), ..
    } = &self.chart
    {
      let (part, _) = image.as_ref();
      return Some(part);
    }
    None
  }

  fn get_chart_zoom(&self) -> Option<f32> {
    if let Chart::Ready { zoom, .. } = &self.chart {
      return Some(*zoom);
    }
    None
  }

  fn set_chart_zoom(&mut self, value: f32) {
    if let Chart::Ready { zoom, .. } = &mut self.chart {
      *zoom = value;
    }
  }

  fn get_chart_image(&self) -> Option<&egui_extras::RetainedImage> {
    if let Chart::Ready {
      image: Some(image), ..
    } = &self.chart
    {
      let (_, image) = image.as_ref();
      return Some(image);
    }
    None
  }

  fn set_chart_image(&mut self, part: chart::ImagePart, img: egui_extras::RetainedImage) {
    if let Chart::Ready { image, .. } = &mut self.chart {
      *image = Some(Box::new((part, img)));
    }
  }

  fn insert_chart_request(&mut self, part: chart::ImagePart) -> bool {
    if let Chart::Ready { requests, .. } = &mut self.chart {
      return requests.insert(part);
    }
    false
  }

  fn remove_chart_request(&mut self, part: &chart::ImagePart) -> bool {
    if let Chart::Ready { requests, .. } = &mut self.chart {
      return requests.remove(part);
    }
    false
  }

  fn take_chart_scroll(&mut self) -> Option<emath::Pos2> {
    if let Chart::Ready { scroll, .. } = &mut self.chart {
      return scroll.take();
    }
    None
  }

  fn set_chart_scroll(&mut self, val: emath::Pos2) {
    if let Chart::Ready { scroll, .. } = &mut self.chart {
      *scroll = Some(val);
    }
  }

  fn set_night_mode(
    &mut self,
    ctx: &egui::Context,
    storage: &mut dyn eframe::Storage,
    night_mode: bool,
  ) {
    if self.night_mode != night_mode {
      self.night_mode = night_mode;

      // Set the theme.
      ctx.set_visuals(if night_mode {
        dark_theme()
      } else {
        self.default_theme.clone()
      });

      // Store the night mode flag.
      storage.set_string(NIGHT_MODE_KEY, format!("{}", night_mode));

      // Request a new image.
      if let Some(part) = self.get_chart_part() {
        self.request_image(part.rect, part.zoom.into());
      }
    }
  }
}

impl eframe::App for App {
  fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
    // Close the side panel on escape.
    if ctx.input().key_pressed(egui::Key::Escape) {
      self.side_panel = false;
    }

    // Process chart reader replies.
    while let Some(reply) = self.chart_reader.get_next_reply() {
      match reply {
        chart::Reply::Image(part, image) => {
          if self.remove_chart_request(&part) {
            let image = egui_extras::RetainedImage::from_color_image("Chart Image", image);
            self.set_chart_image(part, image);
          }
        }
        chart::Reply::Canceled(part) => {
          self.remove_chart_request(&part);
        }
        chart::Reply::GdalError(part, err) => {
          self.remove_chart_request(&part);
          println!("GdalError: ({:?}) {:?}", part, err)
        }
        chart::Reply::ChartSourceNotSet(part) => {
          self.remove_chart_request(&part);
          println!("ChartSourceNotSet: ({:?})", part)
        }
      }
    }

    if let Some(file_dlg) = &mut self.file_dlg {
      if file_dlg.show(ctx).visible() {
        self.ui_enabled = false;
      } else {
        if file_dlg.selected() {
          if let Some(path) = file_dlg.path() {
            let mut files = chart::get_chart_names(&path);
            if files.is_empty() {
              self.chart = Chart::None;
              self.error = Some("Not a chart zip".into());
            } else {
              self.error = None;
              if files.len() > 1 {
                self.chart = Chart::Open { path, files };
              } else {
                let file = files.pop().unwrap();
                self.open_chart(&path, &file);
              }
            }
          }
        }
        self.file_dlg = None;
        self.ui_enabled = true;
      }
    }

    let mut selection = None;
    if let Chart::Open { path, files } = &self.chart {
      self.ui_enabled = false;
      egui::Window::new("🌐  Select Chart Image")
        .collapsible(false)
        .resizable(false)
        .anchor(emath::Align2::CENTER_CENTER, [0.0, 0.0])
        .default_size([0.0, 0.0])
        .show(ctx, |ui| {
          for file in files {
            ui.horizontal(|ui| {
              let text = file.file_stem().unwrap().to_str().unwrap();
              let button = egui::Button::new(text);
              if ui.add_sized(ui.available_size(), button).clicked() {
                selection = Some((path.clone(), file.clone()));
              }
            });
          }
        });
    }

    if let Some((path, file)) = selection.take() {
      self.open_chart(&path, &file);
      self.ui_enabled = true;
    }

    top_panel(ctx, |ui| {
      ui.set_enabled(self.ui_enabled);
      ui.horizontal_centered(|ui| {
        if ui.selectable_label(self.side_panel, "⚙").clicked() {
          self.side_panel = !self.side_panel
        }

        ui.separator();

        if let Some(error) = &self.error {
          let color = if ui.visuals().dark_mode {
            epaint::Color32::LIGHT_RED
          } else {
            epaint::Color32::RED
          };
          ui.label(egui::RichText::new(error.as_ref()).color(color));
        } else if let Some(name) = self.get_chart_name() {
          ui.label(name);
        }
      });
    });

    if self.side_panel {
      side_panel(ctx, |ui| {
        let spacing = ui.spacing().item_spacing;

        ui.horizontal(|ui| {
          let button = egui::Button::new("Open Chart");
          if ui.add_sized(ui.available_size(), button).clicked() {
            self.side_panel = false;
            self.select_chart_zip();
          }
        });

        ui.horizontal(|ui| {
          let button = egui::Button::new("Import NASR");
          if ui.add_sized(ui.available_size(), button).clicked() {}
        });

        ui.horizontal(|ui| {
          let button = egui::Button::new("Add Aircraft");
          if ui.add_sized(ui.available_size(), button).clicked() {}
        });

        ui.add_space(spacing.y);
        ui.separator();

        let mut night_mode = self.night_mode;
        if ui.checkbox(&mut night_mode, "Night Mode").clicked() {
          let storage = frame.storage_mut().unwrap();
          self.set_night_mode(ctx, storage, night_mode);
        }
      });
    }

    central_panel(ctx, |ui| {
      ui.set_enabled(self.ui_enabled);
      if let Some(transform) = self.get_chart_transform() {
        let zoom = self.get_chart_zoom().unwrap();
        let scroll = self.take_chart_scroll();
        let widget = if let Some(pos) = &scroll {
          egui::ScrollArea::both().scroll_offset(emath::Vec2::new(pos.x, pos.y))
        } else {
          egui::ScrollArea::both()
        };

        ui.spacing_mut().item_spacing = emath::Vec2::new(0.0, 0.0);
        let response = widget.always_show_scroll(true).show(ui, |ui| {
          let cursor_pos = ui.cursor().left_top();
          let size = transform.px_size();
          let size = emath::Vec2::new(size.w as f32, size.h as f32) * zoom;
          let rect = emath::Rect::from_min_size(cursor_pos, size);

          // Allocate space for the scroll bars.
          let response = ui.allocate_rect(rect, egui::Sense::click());

          // Place the image.
          if let Some(part) = self.get_chart_part() {
            let scale = zoom * part.zoom.inverse();
            let rect = util::scale_rect(part.rect.into(), scale);
            let rect = rect.translate(emath::Vec2::new(cursor_pos.x, cursor_pos.y));
            ui.allocate_ui_at_rect(rect, |ui| {
              self.get_chart_image().unwrap().show_size(ui, rect.size());
            });
          }

          response
        });

        let pos = response.state.offset;
        let size = response.inner_rect.size();
        let display_rect = util::Rect {
          pos: pos.into(),
          size: size.into(),
        };

        // Request a new image if needed.
        if let Some(part) = self.get_chart_part() {
          if part.rect != display_rect || part.zoom != zoom.into() {
            self.request_image(display_rect, zoom);
          }
        } else if scroll.is_some() {
          // Set zoom to the minimum for the initial image.
          let zoom = size.x / transform.px_size().w as f32;
          let zoom = zoom.max(size.y / transform.px_size().h as f32);
          self.set_chart_zoom(zoom);

          // Set the scroll position to the center.
          let image_size = transform.px_size();
          let image_size = emath::Vec2::new(image_size.w as f32, image_size.h as f32) * zoom;
          let scroll = (image_size - size) * 0.5;
          self.set_chart_scroll(emath::Pos2::new(scroll.x, scroll.y));

          // Request the initial image.
          self.request_image(display_rect, zoom);
        }

        if let Some(hover_pos) = response.inner.hover_pos() {
          let new_zoom = {
            let mut zoom = zoom;
            let input = ctx.input();

            // Process zoom events.
            for event in &input.events {
              if let egui::Event::Zoom(val) = event {
                zoom *= val;
              }
            }
            zoom
          };

          if new_zoom != zoom {
            let min_zoom = size.x / transform.px_size().w as f32;
            let min_zoom = min_zoom.max(size.y / transform.px_size().h as f32);
            let new_zoom = new_zoom.clamp(min_zoom, 1.0);
            self.set_chart_zoom(new_zoom);

            let hover_pos = hover_pos - response.inner_rect.min;
            let pos = (pos + hover_pos) * new_zoom / zoom - hover_pos;
            self.set_chart_scroll(emath::Pos2::new(pos.x, pos.y));

            ctx.request_repaint();
          }

          if response.inner.clicked() {
            let pos = (hover_pos - response.inner_rect.min + pos) / zoom;
            if let Ok(coord) = transform.px_to_nad83(pos.into()) {
              let lat = util::format_lat(coord.y);
              let lon = util::format_lon(coord.x);
              println!("{} {}", lat, lon);
            }
          }
        }
      }
    });
  }

  fn clear_color(&self, visuals: &egui::Visuals) -> epaint::Rgba {
    visuals.extreme_bg_color.into()
  }

  fn persist_egui_memory(&self) -> bool {
    false
  }
}

const NIGHT_MODE_KEY: &str = "night_mode";

fn to_bool(value: Option<String>) -> bool {
  if let Some(value) = value {
    return value == "true";
  }
  false
}

enum Chart {
  None,
  Open {
    path: path::PathBuf,
    files: Vec<path::PathBuf>,
  },
  Ready {
    name: String,
    transform: sync::Arc<chart::Transform>,
    image: Option<Box<(chart::ImagePart, egui_extras::RetainedImage)>>,
    requests: collections::HashSet<chart::ImagePart>,
    scroll: Option<emath::Pos2>,
    zoom: f32,
  },
}

fn dark_theme() -> egui::Visuals {
  let mut visuals = egui::Visuals::dark();
  visuals.extreme_bg_color = epaint::Color32::from_gray(20);
  visuals
}

fn top_panel<R>(ctx: &egui::Context, contents: impl FnOnce(&mut egui::Ui) -> R) {
  let style = ctx.style();
  egui::TopBottomPanel::top("top_panel")
    .frame(egui::Frame {
      inner_margin: egui::style::Margin::symmetric(8.0, 4.0),
      fill: style.visuals.window_fill(),
      stroke: style.visuals.window_stroke(),
      ..Default::default()
    })
    .show(ctx, contents);
}

fn side_panel<R>(ctx: &egui::Context, contents: impl FnOnce(&mut egui::Ui) -> R) {
  let style = ctx.style();
  egui::SidePanel::left("side_panel")
    .frame(egui::Frame {
      inner_margin: egui::style::Margin::same(8.0),
      fill: style.visuals.window_fill(),
      stroke: style.visuals.window_stroke(),
      ..Default::default()
    })
    .resizable(false)
    .default_width(0.0)
    .show(ctx, contents);
}

fn central_panel<R>(ctx: &egui::Context, contents: impl FnOnce(&mut egui::Ui) -> R) {
  let available = ctx.available_rect();
  egui::CentralPanel::default()
    .frame(egui::Frame {
      inner_margin: egui::style::Margin::same(0.0),
      ..Default::default()
    })
    .show(ctx, |ui| {
      ui.set_clip_rect(available);
      contents(ui);
    });
}
