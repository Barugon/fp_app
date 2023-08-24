use crate::{chart, config, error_dlg, find_dlg, nasr, select_dlg, select_menu, touch, util};
use eframe::{egui, emath, epaint, glow};
use egui::scroll_area;
use std::{ffi, path, sync};

pub struct App {
  config: config::Storage,
  win_info: Option<util::WinInfo>,
  default_theme: egui::Visuals,
  asset_path: Option<path::PathBuf>,
  file_dlg: Option<egui_file::FileDialog>,
  find_dlg: Option<find_dlg::FindDlg>,
  error_dlg: Option<error_dlg::ErrorDlg>,
  select_dlg: select_dlg::SelectDlg,
  select_menu: select_menu::SelectMenu,
  nasr_reader: nasr::Reader,
  chart: Chart,
  apt_infos: AptInfos,
  long_press: touch::LongPressTracker,
  top_panel_height: f32,
  side_panel_width: f32,
  save_window: bool,
  night_mode: bool,
  side_panel: bool,
  ui_enabled: bool,
}

impl App {
  pub fn new(
    cc: &eframe::CreationContext,
    theme: Option<egui::Visuals>,
    scale: Option<f32>,
    config: config::Storage,
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

    // If starting in night mode then set the dark theme.
    let night_mode = config.get_night_mode().unwrap_or(false);
    if night_mode {
      cc.egui_ctx.set_visuals(dark_theme());
    }

    let asset_path = if let Some(asset_path) = config.get_asset_path() {
      Some(asset_path.into())
    } else {
      dirs::download_dir()
    };

    Self {
      config,
      win_info: None,
      default_theme,
      asset_path,
      file_dlg: None,
      find_dlg: None,
      error_dlg: None,
      select_dlg: select_dlg::SelectDlg::new(),
      select_menu: select_menu::SelectMenu::default(),
      nasr_reader: nasr::Reader::new(&cc.egui_ctx),
      chart: Chart::None,
      apt_infos: AptInfos::None,
      long_press: touch::LongPressTracker::new(cc.egui_ctx.clone()),
      top_panel_height: 0.0,
      side_panel_width: 0.0,
      save_window,
      night_mode,
      side_panel: true,
      ui_enabled: true,
    }
  }

  fn select_zip_file(&mut self) {
    let edit_focus = Box::new(|focused: bool| {
      util::osk(focused);
    });

    let filter = Box::new(|path: &path::Path| -> bool {
      return path.extension() == Some(ffi::OsStr::new("zip"));
    });

    let mut file_dlg = egui_file::FileDialog::open_file(self.asset_path.clone())
      .title("Open ZIP File")
      .anchor(emath::Align2::CENTER_CENTER, [0.0, 0.0])
      .default_size([525.0, 320.0])
      .edit_focus(edit_focus)
      .filter(filter)
      .show_new_folder(false)
      .show_rename(false)
      .resizable(false);
    file_dlg.open();
    self.file_dlg = Some(file_dlg);
  }

  fn open_chart(&mut self, ctx: &egui::Context, path: &path::Path, file: &path::Path) {
    self.chart = Chart::None;
    match chart::Reader::open(path, file, ctx) {
      Ok(source) => {
        let proj4 = source.transform().get_proj4();
        self.nasr_reader.set_spatial_ref(proj4);
        self.chart = Chart::Ready(Box::new(ChartInfo {
          name: util::stem_string(file).unwrap(),
          reader: sync::Arc::new(source),
          image: None,
          disp_rect: util::Rect::default(),
          scroll: Some(emath::pos2(0.0, 0.0)),
          zoom: 1.0,
        }));
      }
      Err(err) => {
        let text = format!("Unable to open chart: {err:?}");
        self.error_dlg = Some(error_dlg::ErrorDlg::open(text));
      }
    }
  }

  fn request_image(&mut self, rect: util::Rect, zoom: f32) {
    if let Some(reader) = self.get_chart_reader() {
      let dark = self.night_mode;
      let part = chart::ImagePart::new(rect, zoom, dark);
      reader.read_image(part);
    }
  }

  fn get_chart(&self) -> Option<&ChartInfo> {
    if let Chart::Ready(chart) = &self.chart {
      return Some(chart);
    }
    None
  }

  fn get_chart_reader(&self) -> Option<sync::Arc<chart::Reader>> {
    if let Chart::Ready(chart) = &self.chart {
      return Some(chart.reader.clone());
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
        self.apt_infos = AptInfos::None;
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

  fn set_chart_disp_rect(&mut self, rect: util::Rect) {
    if let Chart::Ready(chart) = &mut self.chart {
      if chart.disp_rect != rect {
        chart.disp_rect = rect;

        // Reset the choices on rect change.
        self.apt_infos = AptInfos::None;
      }
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
      // Make sure the scroll position is on an even pixel.
      let pos = emath::pos2(pos.x.trunc().max(0.0), pos.y.trunc().max(0.0));
      chart.scroll = Some(pos);
    }
  }

  /// Pan the map to a NAD83 coordinate.
  fn goto_coord(&mut self, coord: util::Coord) {
    if let Some(chart) = &self.get_chart() {
      if let Ok(coord) = chart.reader.transform().nad83_to_px(coord) {
        let chart_size = chart.reader.transform().px_size();
        if chart_size.contains(coord) {
          let x = coord.x as f32 - 0.5 * chart.disp_rect.size.w as f32;
          let y = coord.y as f32 - 0.5 * chart.disp_rect.size.h as f32;
          self.set_chart_zoom(1.0);
          self.set_chart_scroll(emath::pos2(x, y));
        }
      }
    }
  }

  fn toggle_side_panel(&mut self, visible: bool) {
    self.side_panel = visible;
    if let Some(chart) = self.get_chart() {
      // Scroll the chart to account for the left panel.
      let pos = chart.disp_rect.pos;
      let offset = self.side_panel_width + 1.0;
      let offset = if !self.side_panel {
        pos.x as f32 - offset
      } else {
        pos.x as f32 + offset
      };

      self.set_chart_scroll(emath::pos2(offset, pos.y as f32));
    }
  }

  fn get_next_chart_reply(&self) -> Option<chart::Reply> {
    if let Some(reader) = &self.get_chart_reader() {
      return reader.get_next_reply();
    }
    None
  }

  fn set_night_mode(&mut self, ctx: &egui::Context, night_mode: bool) {
    if self.night_mode != night_mode {
      self.night_mode = night_mode;

      // Set the theme.
      ctx.set_visuals(if night_mode {
        dark_theme()
      } else {
        self.default_theme.clone()
      });

      // Store the night mode flag.
      self.config.set_night_mode(night_mode);

      // Request a new image.
      if let Some((part, _)) = self.get_chart_image() {
        self.request_image(part.rect, part.zoom.into());
      }
    }
  }

  fn process_input_events(&mut self, ctx: &egui::Context) -> InputEvents {
    let mut events = InputEvents::new(ctx);
    events.secondary_click = self.long_press.check();

    ctx.input(|state| {
      for event in &state.events {
        match event {
          egui::Event::Key {
            key,
            pressed,
            repeat,
            modifiers,
          } if *pressed && !*repeat && self.ui_enabled => {
            match key {
              egui::Key::Escape => {
                if matches!(self.apt_infos, AptInfos::Menu(_, _)) {
                  // Remove the choices.
                  self.apt_infos = AptInfos::None;
                } else if self.file_dlg.is_none() {
                  // Close the side panel.
                  self.toggle_side_panel(false);
                }
              }
              egui::Key::F
                if modifiers.command_only()
                  && self.nasr_reader.apt_loaded()
                  && matches!(self.chart, Chart::Ready(_)) =>
              {
                self.find_dlg = Some(find_dlg::FindDlg::open());
                self.apt_infos = AptInfos::None;
              }
              egui::Key::Q if modifiers.command_only() => {
                events.quit = true;
                self.apt_infos = AptInfos::None;
              }
              _ => (),
            }
          }
          egui::Event::Touch {
            device_id: _,
            id,
            phase,
            pos,
            force: _,
          } => self.long_press.initiate(*id, *phase, *pos),
          egui::Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers,
          } if *button == egui::PointerButton::Secondary && !pressed && modifiers.is_none() => {
            events.secondary_click = Some(*pos);
          }
          egui::Event::Zoom(val) => {
            events.zoom_pos = ctx.pointer_hover_pos();
            events.zoom_mod *= val;
          }
          _ => (),
        }
      }
    });
    events
  }
}

impl eframe::App for App {
  fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
    // Get the window information.
    self.win_info = Some(util::WinInfo::new(frame.info_ref()));

    // Process inputs.
    let events = self.process_input_events(ctx);

    // Process chart source replies.
    while let Some(reply) = self.get_next_chart_reply() {
      match reply {
        chart::Reply::Image(part, image) => {
          let image = egui_extras::RetainedImage::from_color_image("Chart Image", image);
          self.set_chart_image(part, image);
        }
        chart::Reply::GdalError(_, err) => {
          println!("{err}");
        }
      }
    }

    // Process NASR airport replies.
    while let Some(reply) = self.nasr_reader.get_next_reply() {
      match reply {
        nasr::Reply::Airport(info) => {
          if let Some(info) = info {
            self.goto_coord(info.coord);
          }
        }
        nasr::Reply::Nearby(infos) => {
          // Filter the airport infos.
          let infos: Vec<nasr::AptInfo> = infos
            .into_iter()
            .filter(|info| !info.non_public_heliport())
            .collect();

          if !infos.is_empty() {
            if let AptInfos::Menu(_, apt_list) = &mut self.apt_infos {
              *apt_list = Some(infos);
            }
          }
        }
        nasr::Reply::Search(infos) => {
          let infos: Vec<nasr::AptInfo> = infos
            .into_iter()
            .filter(|info| !info.non_public_heliport())
            .collect();

          match infos.len() {
            0 => (),
            1 => self.goto_coord(infos[0].coord),
            _ => self.apt_infos = AptInfos::Dialog(infos),
          }
        }
        nasr::Reply::Error(err) => {
          let text = format!("NASR error: {err}");
          self.error_dlg = Some(error_dlg::ErrorDlg::open(text));
        }
      }
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
              self.config.set_asset_path(path.into());
              self.asset_path = Some(path.into());
            }

            let path = path.to_owned();
            match util::get_zip_info(&path) {
              Ok(info) => match info {
                util::ZipInfo::Chart(files) => {
                  if files.len() > 1 {
                    self.chart = Chart::Load(path, files);
                  } else {
                    self.open_chart(ctx, &path, files.first().unwrap());
                  }
                }
                util::ZipInfo::Aero { apt: csv, shp: _ } => {
                  self.nasr_reader.open(path, csv);
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
      let choices = files.iter().map(|f| util::stem_str(f).unwrap());
      if let Some(response) = self.select_dlg.show(ctx, choices) {
        self.ui_enabled = true;
        if let select_dlg::Response::Index(index) = response {
          // Clone the parameters avoid simultaneously borrowing self as immutable and mutable.
          self.open_chart(ctx, &path.clone(), &files[index].clone());
        } else {
          self.chart = Chart::None;
        }
      }
    }

    // Show the selection dialog if there's an airport choice to be made.
    if let AptInfos::Dialog(infos) = &self.apt_infos {
      self.ui_enabled = false;
      let iter = infos.iter().map(|info| info.desc.as_str());
      if let Some(response) = self.select_dlg.show(ctx, iter) {
        self.ui_enabled = true;
        if let select_dlg::Response::Index(index) = response {
          self.goto_coord(infos[index].coord);
        }
        self.apt_infos = AptInfos::None;
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
        find_dlg::Response::Term(term) => {
          let chart = self.get_chart();
          let bounds = chart.map(|chart| chart.reader.transform().bounds().clone());
          self.ui_enabled = true;
          self.find_dlg = None;
          self.nasr_reader.search(term, bounds);
        }
      }
    }

    // Show other choices (such as airports) in a popup.
    if let AptInfos::Menu(lat_lon, infos) = &self.apt_infos {
      let infos = infos.as_ref();
      let iter = infos.map(|v| v.iter().map(|info| info.desc.as_str()));
      if let Some(_response) = self.select_menu.show(ctx, lat_lon, iter) {
        self.apt_infos = AptInfos::None;
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

    self.top_panel_height = top_panel(self.top_panel_height, ctx, |ui| {
      ui.set_enabled(self.ui_enabled);
      ui.horizontal_centered(|ui| {
        let widget = egui::SelectableLabel::new(self.side_panel, " ⚙ ");
        if ui.add_sized([0.0, 21.0], widget).clicked() {
          self.toggle_side_panel(!self.side_panel);
        }

        if self.nasr_reader.apt_loaded() {
          let text = 'text: {
            const APT: &str = "APT";
            if self.nasr_reader.request_count() > 0 {
              ctx.output_mut(|state| state.cursor_icon = egui::CursorIcon::Progress);
              break 'text egui::RichText::new(APT).strong();
            }
            egui::RichText::new(APT)
          };

          ui.separator();
          ui.label(text);
        }

        if let Chart::Ready(chart) = &mut self.chart {
          if self.nasr_reader.apt_loaded() && ui.button("🔎").clicked() {
            self.find_dlg = Some(find_dlg::FindDlg::open());
          }

          ui.separator();
          ui.label(&chart.name);

          ui.with_layout(egui::Layout::right_to_left(emath::Align::Center), |ui| {
            // Zoom-out button.
            ui.add_enabled_ui(chart.zoom < 1.0, |ui| {
              if let Some(font_id) = ui.style().text_styles.get(&egui::TextStyle::Monospace) {
                let text = egui::RichText::new("\u{2009}+\u{2009}").font(font_id.clone());
                let widget = egui::Button::new(text);
                if ui.add_sized([0.0, 21.0], widget).clicked() {
                  let new_zoom = (chart.zoom * 1.25).min(1.0);
                  if new_zoom != chart.zoom {
                    chart.scroll = Some(chart.get_zoom_pos(new_zoom));
                    chart.zoom = new_zoom;
                  }
                }
              }
            });

            // Zoom-in button.
            let min_zoom = chart.get_min_zoom();
            ui.add_enabled_ui(chart.zoom > min_zoom, |ui| {
              if let Some(font_id) = ui.style().text_styles.get(&egui::TextStyle::Monospace) {
                let text = egui::RichText::new("\u{2009}-\u{2009}").font(font_id.clone());
                let widget = egui::Button::new(text);
                if ui.add_sized([0.0, 21.0], widget).clicked() {
                  let new_zoom = (chart.zoom * 0.8).max(min_zoom);
                  if new_zoom != chart.zoom {
                    chart.scroll = Some(chart.get_zoom_pos(new_zoom));
                    chart.zoom = new_zoom;
                  }
                }
              }
            });
          });
        }
      });
    });

    if self.side_panel {
      self.side_panel_width = side_panel(self.side_panel_width, ctx, |ui| {
        ui.set_enabled(self.ui_enabled);

        ui.horizontal(|ui| {
          let button = egui::Button::new("Open Zip File");
          if ui.add_sized(ui.available_size(), button).clicked() {
            self.select_zip_file();
          }
        });

        ui.add_space(ui.spacing().item_spacing.y);
        ui.separator();

        ui.horizontal(|ui| {
          let mut night_mode = self.night_mode;
          if ui.checkbox(&mut night_mode, "Night Mode").clicked() {
            self.set_night_mode(ctx, night_mode);
          }
        });
      });
    }

    central_panel(ctx, self.side_panel, |ui| {
      ui.set_enabled(self.ui_enabled);
      if let Some(reader) = self.get_chart_reader() {
        let zoom = self.get_chart_zoom().unwrap();
        let scroll = self.take_chart_scroll();
        let widget = if let Some(pos) = &scroll {
          egui::ScrollArea::both().scroll_offset(pos.to_vec2())
        } else {
          egui::ScrollArea::both()
        }
        .scroll_bar_visibility(scroll_area::ScrollBarVisibility::AlwaysVisible);

        ui.spacing_mut().scroll_bar_inner_margin = 0.0;

        let response = widget.show(ui, |ui| {
          let cursor_pos = ui.cursor().left_top();
          let size = reader.transform().px_size();
          let size = emath::vec2(size.w as f32, size.h as f32) * zoom;
          let rect = emath::Rect::from_min_size(cursor_pos, size);

          // Reserve space for the scroll bars.
          ui.allocate_rect(rect, egui::Sense::hover());

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
        });

        // Set a new display rectangle.
        let pos = response.state.offset;
        let display_rect = util::Rect {
          pos: pos.into(),
          size: response.inner_rect.size().into(),
        };
        self.set_chart_disp_rect(display_rect);

        // Make sure the image is always scrolled to an even pixel.
        if !response.state.has_momentum() && pos.floor() != pos {
          self.set_chart_scroll(emath::pos2(pos.x, pos.y));
        }

        // Get the minimum zoom.
        let min_zoom = self.get_chart().unwrap().get_min_zoom();

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

        if let Some(zoom_pos) = events.zoom_pos {
          if response.inner_rect.contains(zoom_pos) {
            let new_zoom = zoom * events.zoom_mod;
            if new_zoom != zoom {
              // Correct and set the new zoom value.
              let new_zoom = new_zoom.clamp(min_zoom, 1.0);
              self.set_chart_zoom(new_zoom);

              // Attempt to keep the point under the mouse cursor the same.
              let zoom_pos = zoom_pos - response.inner_rect.min;
              let pos = (pos + zoom_pos) * new_zoom / zoom - zoom_pos;
              self.set_chart_scroll(pos.to_pos2());

              ctx.request_repaint();
            }
          }
        }

        if let Some(click_pos) = events.secondary_click {
          // Make sure the clicked position is actually over the chart area.
          if response.inner_rect.contains(click_pos) {
            let pos = (click_pos - response.inner_rect.min + pos) / zoom;
            let coord = reader.transform().px_to_chart(pos.into());
            if let Ok(coord) = reader.transform().chart_to_nad83(coord) {
              let lat = util::format_lat(coord.y);
              let lon = util::format_lon(coord.x);
              self.select_menu.set_pos(click_pos);
              self.apt_infos = AptInfos::Menu(format!("{lat}, {lon}"), None);
            }

            if self.nasr_reader.apt_spatial_idx() {
              // 1/2 nautical mile (926 meters) is the search radius at 1.0x zoom.
              self.nasr_reader.nearby(coord, 926.0 / zoom as f64);
            }
          }
        }
      }
    });

    if events.quit {
      frame.close();
    }
  }

  fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
    let color = if visuals.dark_mode {
      visuals.extreme_bg_color
    } else {
      epaint::Color32::from_gray(220)
    };

    const CONV: f32 = 1.0 / 255.0;
    [
      color[0] as f32 * CONV,
      color[1] as f32 * CONV,
      color[2] as f32 * CONV,
      color[3] as f32 * CONV,
    ]
  }

  fn on_exit(&mut self, _gl: Option<&glow::Context>) {
    if !self.save_window {
      return;
    }

    if let Some(win_info) = self.win_info.take() {
      self.config.set_win_info(&win_info);
    }
  }
}

enum AptInfos {
  None,
  Menu(String, Option<Vec<nasr::AptInfo>>),
  Dialog(Vec<nasr::AptInfo>),
}

struct InputEvents {
  zoom_mod: f32,
  zoom_pos: Option<emath::Pos2>,
  secondary_click: Option<emath::Pos2>,
  quit: bool,
}

impl InputEvents {
  fn new(ctx: &egui::Context) -> Self {
    // Init zoom with multi-touch if available.
    let (zoom_mod, zoom_pos) = if let Some(multi_touch) = ctx.multi_touch() {
      (multi_touch.zoom_delta, Some(multi_touch.start_pos))
    } else {
      (1.0, None)
    };

    Self {
      zoom_mod,
      zoom_pos,
      secondary_click: None,
      quit: false,
    }
  }
}

const MIN_ZOOM: f32 = 1.0 / 8.0;

struct ChartInfo {
  name: String,
  reader: sync::Arc<chart::Reader>,
  image: Option<(chart::ImagePart, egui_extras::RetainedImage)>,
  disp_rect: util::Rect,
  scroll: Option<emath::Pos2>,
  zoom: f32,
}

impl ChartInfo {
  fn get_min_zoom(&self) -> f32 {
    let chart_size: emath::Vec2 = self.reader.transform().px_size().into();
    let disp_size: emath::Vec2 = self.disp_rect.size.into();
    let sw = disp_size.x / chart_size.x;
    let sh = disp_size.y / chart_size.y;
    sw.max(sh).max(MIN_ZOOM)
  }

  fn get_zoom_pos(&self, zoom: f32) -> emath::Pos2 {
    let pos: emath::Pos2 = self.disp_rect.pos.into();
    let size: emath::Vec2 = self.disp_rect.size.into();
    let offset = size * 0.5;
    let ratio = zoom / self.zoom;
    let x = ratio * (pos.x + offset.x) - offset.x;
    let y = ratio * (pos.y + offset.y) - offset.y;
    emath::pos2(x, y)
  }
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

fn top_panel<R>(
  height: f32,
  ctx: &egui::Context,
  contents: impl FnOnce(&mut egui::Ui) -> R,
) -> f32 {
  let style = ctx.style();
  let fill = if style.visuals.dark_mode {
    epaint::Color32::from_gray(35)
  } else {
    style.visuals.window_fill()
  };

  let response = egui::TopBottomPanel::top(format!("{}_top_panel", util::APP_NAME))
    .frame(egui::Frame {
      inner_margin: egui::style::Margin {
        left: 8.0,
        top: 4.0,
        right: 8.0,
        bottom: 8.0,
      },
      fill,
      ..Default::default()
    })
    .default_height(height)
    .show(ctx, contents);

  // Round up the width.
  response.response.rect.height().ceil()
}

fn side_panel<R>(
  width: f32,
  ctx: &egui::Context,
  contents: impl FnOnce(&mut egui::Ui) -> R,
) -> f32 {
  let style = ctx.style();
  let fill = if style.visuals.dark_mode {
    epaint::Color32::from_gray(35)
  } else {
    style.visuals.window_fill()
  };

  let response = egui::SidePanel::left(format!("{}_side_panel", util::APP_NAME))
    .frame(egui::Frame {
      inner_margin: egui::style::Margin::same(8.0),
      fill,
      ..Default::default()
    })
    .resizable(false)
    .default_width(width)
    .show(ctx, contents);

  // Round up the width.
  response.response.rect.width().ceil()
}

fn central_panel<R>(ctx: &egui::Context, left: bool, contents: impl FnOnce(&mut egui::Ui) -> R) {
  let available = ctx.available_rect();
  let left = if left { 1.0 } else { 0.0 };
  let top = 1.0;
  let min = emath::pos2(available.min.x + left, available.min.y + top);
  let max = available.max;
  let frame = egui::Frame {
    inner_margin: egui::style::Margin::same(0.0),
    outer_margin: egui::style::Margin {
      left,
      top,
      ..Default::default()
    },
    ..Default::default()
  };
  egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
    ui.set_clip_rect(emath::Rect::from_min_max(min, max));
    contents(ui);
  });
}
