use crate::{chart, error_dlg, nasr, select_dlg, util};
use eframe::{egui, emath, epaint};
use std::{collections, path, sync};

pub struct App {
  default_theme: egui::Visuals,
  chart_reader: chart::AsyncReader,
  apt_source: Option<nasr::APTSource>,
  file_dlg: Option<egui_file::FileDialog>,
  select_dlg: Option<select_dlg::SelectDlg>,
  error_dlg: Option<error_dlg::ErrorDlg>,
  chart: Option<ChartInfo>,
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
      apt_source: None,
      file_dlg: None,
      select_dlg: None,
      error_dlg: None,
      chart: None,
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
        self.chart = Some(ChartInfo {
          name: file.file_stem().unwrap().to_str().unwrap().into(),
          transform: sync::Arc::new(transform),
          image: None,
          requests: collections::HashSet::new(),
          scroll: Some(emath::Pos2::new(0.0, 0.0)),
          zoom: 1.0,
        });
      }
      Err(err) => {
        let text = format!("Unable to open chart: {:?}", err);
        self.error_dlg = Some(error_dlg::ErrorDlg::open(text));
      }
    }
  }

  fn request_image(&mut self, rect: util::Rect, zoom: f32) {
    let dark = self.night_mode;
    let part = chart::ImagePart::new(rect, zoom, dark);
    if self.insert_chart_request(part.clone()) {
      self.chart_reader.read_image(part);
    }
  }

  fn get_chart_transform(&self) -> Option<sync::Arc<chart::Transform>> {
    if let Some(chart) = &self.chart {
      return Some(chart.transform.clone());
    }
    None
  }

  fn get_chart_zoom(&self) -> Option<f32> {
    if let Some(chart) = &self.chart {
      return Some(chart.zoom);
    }
    None
  }

  fn set_chart_zoom(&mut self, value: f32) {
    if let Some(chart) = &mut self.chart {
      chart.zoom = value;
    }
  }

  fn get_chart_image(&self) -> Option<&(chart::ImagePart, egui_extras::RetainedImage)> {
    if let Some(chart) = &self.chart {
      return chart.image.as_ref();
    }
    None
  }

  fn set_chart_image(&mut self, part: chart::ImagePart, image: egui_extras::RetainedImage) {
    if let Some(chart) = &mut self.chart {
      chart.image = Some((part, image));
    }
  }

  fn insert_chart_request(&mut self, part: chart::ImagePart) -> bool {
    if let Some(chart) = &mut self.chart {
      return chart.requests.insert(part);
    }
    false
  }

  fn remove_chart_request(&mut self, part: &chart::ImagePart) -> bool {
    if let Some(chart) = &mut self.chart {
      return chart.requests.remove(part);
    }
    false
  }

  fn take_chart_scroll(&mut self) -> Option<emath::Pos2> {
    if let Some(chart) = &mut self.chart {
      return chart.scroll.take();
    }
    None
  }

  fn set_chart_scroll(&mut self, val: emath::Pos2) {
    if let Some(chart) = &mut self.chart {
      chart.scroll = Some(val);
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
      if let Some((part, _)) = self.get_chart_image() {
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

    // Process NASR airport replies.
    if let Some(apt_source) = &self.apt_source {
      while let Some(reply) = apt_source.get_next_reply() {
        println!("{:?}", reply);
      }
    }

    if let Some(file_dlg) = &mut self.file_dlg {
      if file_dlg.show(ctx).visible() {
        self.ui_enabled = false;
      } else {
        if file_dlg.selected() {
          if let Some(path) = file_dlg.path() {
            match util::get_zip_info(&path) {
              Ok(info) => match info {
                util::ZipInfo::Chart(files) => {
                  if files.len() > 1 {
                    self.select_dlg = Some(select_dlg::SelectDlg::open(path, files));
                  } else {
                    self.open_chart(&path, files.first().unwrap());
                  }
                }
                util::ZipInfo::Aeronautical => {
                  let ctx = ctx.clone();
                  match nasr::APTSource::open(&path, move || ctx.request_repaint()) {
                    Ok(apt_source) => self.apt_source = Some(apt_source),
                    Err(err) => {
                      self.error_dlg = Some(error_dlg::ErrorDlg::open(format!("{}", err)))
                    }
                  }
                }
                util::ZipInfo::Airspace(_folder) => {
                  self.error_dlg = Some(error_dlg::ErrorDlg::open("Not yet implemented".into()))
                }
              },
              Err(err) => {
                self.error_dlg = Some(error_dlg::ErrorDlg::open(err));
              }
            }
          }
        }
        self.file_dlg = None;
        self.ui_enabled = true;
      }
    }

    if let Some(select_dlg) = &mut self.select_dlg {
      self.ui_enabled = false;
      if let Some((path, file)) = select_dlg.show(ctx) {
        self.open_chart(&path, &file);
        self.select_dlg = None;
        self.ui_enabled = true;
      }
    }

    if let Some(error_dlg) = &mut self.error_dlg {
      self.ui_enabled = false;
      if !error_dlg.show(ctx) {
        self.error_dlg = None;
        self.ui_enabled = true;
      }
    }

    top_panel(ctx, |ui| {
      ui.set_enabled(self.ui_enabled);
      ui.horizontal_centered(|ui| {
        if ui.selectable_label(self.side_panel, "⚙").clicked() {
          self.side_panel = !self.side_panel
        }

        ui.separator();

        if let Some(chart) = &self.chart {
          ui.label(&chart.name);
        }
      });
    });

    if self.side_panel {
      side_panel(ctx, |ui| {
        let spacing = ui.spacing().item_spacing;

        ui.horizontal(|ui| {
          let button = egui::Button::new("Open Zip File");
          if ui.add_sized(ui.available_size(), button).clicked() {
            self.side_panel = false;
            self.select_chart_zip();
          }
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
          egui::ScrollArea::both().scroll_offset(pos.to_vec2())
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
          if let Some((part, image)) = self.get_chart_image() {
            let scale = zoom * part.zoom.inverse();
            let rect = util::scale_rect(part.rect.into(), scale);
            let rect = rect.translate(cursor_pos.to_vec2());
            ui.allocate_ui_at_rect(rect, |ui| {
              image.show_size(ui, rect.size());
            });
          }

          response
        });

        let pos = response.state.offset;
        let size = response.inner_rect.size();
        let min_zoom = size.x / transform.px_size().w as f32;
        let min_zoom = min_zoom.max(size.y / transform.px_size().h as f32);

        if let Some((part, _)) = self.get_chart_image() {
          // Make sure the zoom is not below the minimum.
          let request_zoom = zoom.max(min_zoom);
          let display_rect = util::Rect {
            pos: pos.into(),
            size: size.into(),
          };

          // Request a new image if needed.
          if part.rect != display_rect || part.zoom != request_zoom.into() {
            self.request_image(display_rect, request_zoom);
          }

          if request_zoom != zoom {
            self.set_chart_zoom(request_zoom);
            ctx.request_repaint();
          }
        } else if scroll.is_some() && zoom == 1.0 {
          // Set zoom to the minimum for the initial image.
          self.set_chart_zoom(min_zoom);

          // Set the scroll position to the center.
          let image_size = transform.px_size();
          let image_size = emath::Vec2::new(image_size.w as f32, image_size.h as f32) * min_zoom;
          let scroll = (image_size - size) * 0.5;
          self.set_chart_scroll(scroll.to_pos2());

          // Request the initial image.
          let display_rect = util::Rect {
            pos: (pos + scroll).into(),
            size: size.into(),
          };
          self.request_image(display_rect, min_zoom);

          ctx.request_repaint();
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
            // Correct and set the new zoom value.
            let new_zoom = new_zoom.clamp(min_zoom, 1.0);
            self.set_chart_zoom(new_zoom);

            // Attempt to keep the point under the mouse cursor the same.
            let hover_pos = hover_pos - response.inner_rect.min;
            let pos = (pos + hover_pos) * new_zoom / zoom - hover_pos;
            self.set_chart_scroll(pos.to_pos2());

            ctx.request_repaint();
          }

          if response.inner.secondary_clicked() {
            if let Some(apt_source) = &self.apt_source {
              let pos = (hover_pos - response.inner_rect.min + pos) / zoom;
              let coord = transform.px_to_chart(pos.into());
              let dist = transform.px_to_dist(100.0 / zoom as f64);
              apt_source.request_nearby(coord, dist);

              let coord = transform.chart_to_nad83(coord).unwrap();
              println!(
                "{} {}",
                util::format_lat(coord.y),
                util::format_lon(coord.x)
              );
            }
          }
        }
      }
    });
  }

  fn clear_color(&self, visuals: &egui::Visuals) -> epaint::Rgba {
    if visuals.dark_mode {
      visuals.extreme_bg_color.into()
    } else {
      epaint::Color32::from_gray(220).into()
    }
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

struct ChartInfo {
  name: String,
  transform: sync::Arc<chart::Transform>,
  image: Option<(chart::ImagePart, egui_extras::RetainedImage)>,
  requests: collections::HashSet<chart::ImagePart>,
  scroll: Option<emath::Pos2>,
  zoom: f32,
}

fn dark_theme() -> egui::Visuals {
  let mut visuals = egui::Visuals::dark();
  visuals.extreme_bg_color = epaint::Color32::from_gray(20);
  visuals
}

fn top_panel<R>(ctx: &egui::Context, contents: impl FnOnce(&mut egui::Ui) -> R) {
  let style = ctx.style();
  let fill = if style.visuals.dark_mode {
    epaint::Color32::from_gray(35)
  } else {
    style.visuals.window_fill()
  };

  egui::TopBottomPanel::top("top_panel")
    .frame(egui::Frame {
      inner_margin: egui::style::Margin::symmetric(8.0, 4.0),
      fill,
      ..Default::default()
    })
    .show(ctx, contents);
}

fn side_panel<R>(ctx: &egui::Context, contents: impl FnOnce(&mut egui::Ui) -> R) {
  let style = ctx.style();
  let fill = if style.visuals.dark_mode {
    epaint::Color32::from_gray(35)
  } else {
    style.visuals.window_fill()
  };

  egui::SidePanel::left("side_panel")
    .frame(egui::Frame {
      inner_margin: egui::style::Margin::same(8.0),
      fill,
      ..Default::default()
    })
    .resizable(false)
    .default_width(0.0)
    .show(ctx, contents);
}

fn central_panel<R>(ctx: &egui::Context, contents: impl FnOnce(&mut egui::Ui) -> R) {
  let available = ctx.available_rect();
  let min = emath::Pos2::new(available.min.x + 1.0, available.min.y + 1.0);
  let rect = emath::Rect::from_min_max(min, available.max);
  egui::CentralPanel::default()
    .frame(egui::Frame {
      inner_margin: egui::style::Margin::same(0.0),
      outer_margin: egui::style::Margin {
        left: 1.0,
        right: 0.0,
        top: 1.0,
        bottom: 0.0,
      },
      ..Default::default()
    })
    .show(ctx, |ui| {
      ui.set_clip_rect(rect);
      contents(ui);
    });
}
