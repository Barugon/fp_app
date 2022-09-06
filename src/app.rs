use crate::{chart, error_dlg, find_dlg, nasr, select_dlg, select_menu, util};
use eframe::{egui, emath, epaint};
use std::{collections, path, sync};

pub struct App {
  save_window: bool,
  default_theme: egui::Visuals,
  asset_path: Option<path::PathBuf>,
  file_dlg: Option<egui_file::FileDialog>,
  find_dlg: Option<find_dlg::FindDlg>,
  error_dlg: Option<error_dlg::ErrorDlg>,
  select_dlg: select_dlg::SelectDlg,
  select_menu: select_menu::SelectMenu,
  choices: Option<Vec<String>>,
  apt_source: Option<nasr::APTSource>,
  chart: Chart,
  night_mode: bool,
  side_panel: bool,
  ui_enabled: bool,
}

impl App {
  pub fn new(
    cc: &eframe::CreationContext,
    theme: Option<egui::Visuals>,
    scale: Option<f32>,
  ) -> Self {
    if let Some(theme) = theme {
      cc.egui_ctx.set_visuals(theme);
    }

    let save_window = scale.is_none();
    if let Some(scale) = scale {
      cc.egui_ctx.set_pixels_per_point(scale);
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

    // If we're starting in night mode then set the dark theme.
    let night_mode = to_bool(cc.storage.unwrap().get_string(NIGHT_MODE_KEY));
    if night_mode {
      cc.egui_ctx.set_visuals(dark_theme());
    }

    let asset_path = if let Some(asset_path) = cc.storage.unwrap().get_string(ASSET_PATH_KEY) {
      Some(asset_path.into())
    } else {
      dirs::download_dir()
    };

    Self {
      save_window,
      default_theme,
      asset_path,
      file_dlg: None,
      find_dlg: None,
      error_dlg: None,
      select_dlg: select_dlg::SelectDlg,
      select_menu: select_menu::SelectMenu::default(),
      choices: None,
      apt_source: None,
      chart: Chart::None,
      night_mode,
      side_panel: true,
      ui_enabled: true,
    }
  }

  fn select_chart_zip(&mut self) {
    let mut file_dlg = egui_file::FileDialog::open_file(self.asset_path.clone())
      .filter("zip".into())
      .show_new_folder(false)
      .show_rename(false)
      .resizable(false);
    file_dlg.open();
    self.file_dlg = Some(file_dlg);
  }

  fn open_chart(&mut self, ctx: &egui::Context, path: &path::Path, file: &path::Path) {
    self.chart = Chart::None;
    match chart::Source::open(path, file, ctx) {
      Ok(source) => {
        if let Some(apt_source) = &self.apt_source {
          apt_source.set_spatial_ref(source.transform().get_proj4());
        }

        self.chart = Chart::Ready(Box::new(ChartInfo {
          name: util::file_stem(file).unwrap(),
          source: sync::Arc::new(source),
          image: None,
          requests: collections::HashSet::new(),
          disp_rect: util::Rect::default(),
          scroll: Some(emath::Pos2::new(0.0, 0.0)),
          zoom: 1.0,
        }));
      }
      Err(err) => {
        let text = format!("Unable to open chart: {:?}", err);
        self.error_dlg = Some(error_dlg::ErrorDlg::open(text));
      }
    }
  }

  fn request_image(&mut self, rect: util::Rect, zoom: f32) {
    if let Some(source) = self.get_chart_source() {
      let dark = self.night_mode;
      let part = chart::ImagePart::new(rect, zoom, dark);
      if self.insert_chart_request(part.clone()) {
        source.read_image(part);
      }
    }
  }

  fn get_chart_source(&self) -> Option<sync::Arc<chart::Source>> {
    if let Chart::Ready(chart) = &self.chart {
      return Some(chart.source.clone());
    }
    None
  }

  fn get_chart_zoom(&self) -> Option<f32> {
    if let Chart::Ready(chart) = &self.chart {
      return Some(chart.zoom);
    }
    None
  }

  fn set_chart_zoom(&mut self, val: f32) {
    if let Chart::Ready(chart) = &mut self.chart {
      if chart.zoom != val {
        chart.zoom = val;

        // Reset the choices on zoom change.
        self.choices = None;
      }
    }
  }

  fn get_chart_image(&self) -> Option<&(chart::ImagePart, egui_extras::RetainedImage)> {
    if let Chart::Ready(chart) = &self.chart {
      return chart.image.as_ref();
    }
    None
  }

  fn set_chart_image(&mut self, part: chart::ImagePart, image: egui_extras::RetainedImage) {
    if let Chart::Ready(chart) = &mut self.chart {
      chart.image = Some((part, image));
    }
  }

  fn insert_chart_request(&mut self, part: chart::ImagePart) -> bool {
    if let Chart::Ready(chart) = &mut self.chart {
      return chart.requests.insert(part);
    }
    false
  }

  fn remove_chart_request(&mut self, part: &chart::ImagePart) -> bool {
    if let Chart::Ready(chart) = &mut self.chart {
      return chart.requests.remove(part);
    }
    false
  }

  fn set_chart_disp_rect(&mut self, rect: util::Rect) {
    if let Chart::Ready(chart) = &mut self.chart {
      if chart.disp_rect.pos != rect.pos {
        // Reset the choices on scroll change.
        self.choices = None;
      }

      chart.disp_rect = rect;
    }
  }

  fn take_chart_scroll(&mut self) -> Option<emath::Pos2> {
    if let Chart::Ready(chart) = &mut self.chart {
      return chart.scroll.take();
    }
    None
  }

  fn set_chart_scroll(&mut self, pos: emath::Pos2) {
    if let Chart::Ready(chart) = &mut self.chart {
      chart.scroll = Some(pos);
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

  fn handle_hotkeys(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
    for event in &ctx.input().events {
      if let egui::Event::Key {
        key,
        pressed,
        modifiers,
      } = event
      {
        if *pressed && self.ui_enabled {
          match key {
            egui::Key::Escape => {
              if self.choices.is_some() {
                // Remove the choices.
                self.choices = None;
              } else if self.file_dlg.is_none() {
                // Close the side panel.
                self.side_panel = false;
              }
            }
            egui::Key::F
              if modifiers.command_only()
                && self.apt_source.is_some()
                && matches!(self.chart, Chart::Ready(_)) =>
            {
              self.find_dlg = Some(find_dlg::FindDlg::open());
              self.choices = None;
            }
            egui::Key::Q if modifiers.command_only() => {
              frame.close();
              self.choices = None;
            }
            _ => (),
          }
        }
      }
    }
  }
}

impl eframe::App for App {
  fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
    self.handle_hotkeys(ctx, frame);

    // Process chart source replies.
    if let Some(chart_source) = &self.get_chart_source() {
      while let Some(reply) = chart_source.get_next_reply() {
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
        }
      }
    }

    // Process NASR airport replies.
    let mut pos = None;
    if let Some(apt_source) = &self.apt_source {
      while let Some(reply) = apt_source.get_next_reply() {
        match reply {
          nasr::APTReply::Airport(airport) => {
            if let Some(airport) = airport {
              if let Chart::Ready(chart) = &self.chart {
                if let Ok(coord) = chart.source.transform().nad83_to_px(airport.coord) {
                  let x = coord.x as f32 - 0.5 * chart.disp_rect.size.w as f32;
                  let y = coord.y as f32 - 0.5 * chart.disp_rect.size.h as f32;
                  pos = Some(emath::Pos2::new(x, y));
                }
              }
            }
          }
          nasr::APTReply::Nearby(nearby) => {
            if let Some(choices) = &mut self.choices {
              for info in nearby {
                if matches!(
                  info.site_type,
                  nasr::SiteType::Airport | nasr::SiteType::Seaplane
                ) {
                  choices.push(format!("{} ({}), {:?}", info.name, info.id, info.site_use));
                }
              }
            }
          }
          nasr::APTReply::Search(_) => (),
        }
      }

      if apt_source.request_count() > 0 {
        ctx.output().cursor_icon = egui::CursorIcon::Progress;
      }
    }

    if let Some(pos) = pos {
      self.set_chart_zoom(1.0);
      self.set_chart_scroll(pos);
    }

    // Show the file dialog if set.
    if let Some(file_dlg) = &mut self.file_dlg {
      if file_dlg.show(ctx).visible() {
        self.ui_enabled = false;
      } else {
        if file_dlg.selected() {
          if let Some(path) = file_dlg.path() {
            // Save the path.
            if let Some(path) = path.parent().and_then(|p| p.to_str()) {
              let storage = frame.storage_mut().unwrap();
              storage.set_string(ASSET_PATH_KEY, path.into());
              self.asset_path = Some(path.into());
            }

            match util::get_zip_info(&path) {
              Ok(info) => match info {
                util::ZipInfo::Chart(files) => {
                  if files.len() > 1 {
                    self.chart = Chart::Load(path, files);
                  } else {
                    self.open_chart(ctx, &path, files.first().unwrap());
                  }
                }
                util::ZipInfo::Aeronautical => {
                  let mut errors = Vec::new();

                  match nasr::APTSource::open(&path, ctx) {
                    Ok(apt_source) => {
                      if let Some(source) = self.get_chart_source() {
                        apt_source.set_spatial_ref(source.transform().get_proj4());
                      }
                      self.apt_source = Some(apt_source);
                    }
                    Err(_err) => {
                      debugln!("{}", _err);
                      errors.push("APT_BASE.csv");
                    }
                  }

                  if !errors.is_empty() {
                    let err_txt = format!("Not found: {:?}", errors);
                    self.error_dlg = Some(error_dlg::ErrorDlg::open(err_txt));
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

    // Show the selection dialog if there's a chart choice to be made.
    if let Chart::Load(path, files) = &self.chart {
      self.ui_enabled = false;
      let choices = files.iter().map(|f| util::file_stem(f).unwrap()).collect();
      if let Some(response) = self.select_dlg.show(ctx, choices) {
        self.ui_enabled = true;
        if let select_dlg::Response::Index(index) = response {
          // Clone the parameters so that we're not borrowing self as immutable and mutable.
          self.open_chart(ctx, &path.clone(), &files[index].clone());
        } else {
          self.chart = Chart::None;
        }
      }
    }

    // Show the find dialog.
    if let Some(find_dialog) = &mut self.find_dlg {
      self.ui_enabled = false;
      match find_dialog.show(ctx) {
        find_dlg::Response::None => (),
        find_dlg::Response::Cancel => {
          self.ui_enabled = true;
          self.find_dlg = None;
        }
        find_dlg::Response::Id(id) => {
          self.ui_enabled = true;
          self.find_dlg = None;
          if let Some(apt_source) = &self.apt_source {
            apt_source.airport(id);
          }
        }
      }
    }

    // Show other choices (such as airports) in a popup.
    if let Some(choices) = &self.choices {
      if let Some(_response) = self.select_menu.show(ctx, choices) {
        self.choices = None;
      }
    }

    // Show the error dialog if there's an error.
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

        if let Some(apt_source) = &self.apt_source {
          const APT: &str = "APT";
          let text = if apt_source.request_count() > 0 {
            egui::RichText::new(APT).strong()
          } else {
            egui::RichText::new(APT)
          };

          ui.separator();
          ui.label(text);
        }

        if let Chart::Ready(chart) = &self.chart {
          if self.apt_source.is_some() {
            if ui.button("🔎").clicked() {
              self.find_dlg = Some(find_dlg::FindDlg::open());
            }
          }

          ui.separator();
          ui.label(&chart.name);
        }
      });
    });

    if self.side_panel {
      side_panel(ctx, |ui| {
        ui.set_enabled(self.ui_enabled);

        let spacing = ui.spacing().item_spacing;
        ui.horizontal(|ui| {
          let button = egui::Button::new("Open Zip File");
          if ui.add_sized(ui.available_size(), button).clicked() {
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

    central_panel(ctx, self.side_panel, |ui| {
      ui.set_enabled(self.ui_enabled);
      if let Some(source) = self.get_chart_source() {
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
          let size = source.transform().px_size();
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
              let mut clip = ui.clip_rect();
              clip.max -= emath::Vec2::splat(ui.spacing().scroll_bar_width * 0.5);
              ui.set_clip_rect(clip);
              image.show_size(ui, rect.size());
            });
          }

          response
        });

        let pos = response.state.offset;
        let size = response.inner_rect.size();
        let min_zoom = size.x / source.transform().px_size().w as f32;
        let min_zoom = min_zoom.max(size.y / source.transform().px_size().h as f32);
        let display_rect = util::Rect {
          pos: pos.into(),
          size: size.into(),
        };

        self.set_chart_disp_rect(display_rect);

        if let Some((part, _)) = self.get_chart_image() {
          // Make sure the zoom is not below the minimum.
          let request_zoom = zoom.max(min_zoom);

          // Request a new image if needed.
          if part.rect != display_rect || part.zoom != request_zoom.into() {
            self.request_image(display_rect, request_zoom);
          }

          if request_zoom != zoom {
            self.set_chart_zoom(request_zoom);
            ctx.request_repaint();
          }
        } else if scroll.is_some() && zoom == 1.0 {
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
              let coord = source.transform().px_to_chart(pos.into());
              apt_source.nearby(coord, 926.0 / zoom as f64);

              if let Ok(coord) = source.transform().chart_to_nad83(coord) {
                let lat = util::format_lat(coord.y);
                let lon = util::format_lon(coord.x);
                self.select_menu.set_pos(hover_pos);
                self.choices = Some(vec![format!("{}, {}", lat, lon)]);
              }
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

  fn persist_native_window(&self) -> bool {
    self.save_window
  }
}

const NIGHT_MODE_KEY: &str = "night_mode";
const ASSET_PATH_KEY: &str = "asset_path";

fn to_bool(value: Option<String>) -> bool {
  if let Some(value) = value {
    return value == "true";
  }
  false
}

struct ChartInfo {
  name: String,
  source: sync::Arc<chart::Source>,
  image: Option<(chart::ImagePart, egui_extras::RetainedImage)>,
  requests: collections::HashSet<chart::ImagePart>,
  disp_rect: util::Rect,
  scroll: Option<emath::Pos2>,
  zoom: f32,
}

enum Chart {
  None,
  Load(path::PathBuf, Vec<path::PathBuf>),
  Ready(Box<ChartInfo>),
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

fn central_panel<R>(ctx: &egui::Context, left: bool, contents: impl FnOnce(&mut egui::Ui) -> R) {
  let available = ctx.available_rect();
  let left = if left { 1.0 } else { 0.0 };
  let min = emath::Pos2::new(available.min.x + left, available.min.y + 1.0);
  let max = available.max;
  egui::CentralPanel::default()
    .frame(egui::Frame {
      inner_margin: egui::style::Margin::same(0.0),
      outer_margin: egui::style::Margin {
        left,
        top: 1.0,
        ..Default::default()
      },
      ..Default::default()
    })
    .show(ctx, |ui| {
      ui.set_clip_rect(emath::Rect::from_min_max(min, max));
      contents(ui);
    });
}
