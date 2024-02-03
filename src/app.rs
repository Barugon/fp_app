use crate::{chart, config, error_dlg, find_dlg, nasr, select_dlg, select_menu, touch, util};
use eframe::{egui, emath, epaint, glow};
use egui::scroll_area;
use std::{ffi::OsStr, path, rc};

pub struct App {
  config: config::Storage,
  win_info: util::WinInfo,
  default_theme: egui::Visuals,
  asset_path: Option<path::PathBuf>,
  file_dlg: Option<egui_file::FileDialog>,
  find_dlg: Option<find_dlg::FindDlg>,
  error_dlg: Option<error_dlg::ErrorDlg>,
  select_dlg: select_dlg::SelectDlg,
  select_menu: select_menu::SelectMenu,
  airport_reader: Option<nasr::AirportReader>,
  chart: Chart,
  airport_infos: AirportInfos,
  long_press: touch::LongPressTracker,
  top_panel_height: u32,
  side_panel_width: u32,
  night_mode: bool,
  side_panel: bool,
  ui_enabled: bool,
  include_nph: bool,
}

impl App {
  pub fn new(
    cc: &eframe::CreationContext,
    theme: Option<egui::Visuals>,
    scale: Option<f32>,
    config: config::Storage,
  ) -> Self {
    let ctx = &cc.egui_ctx;
    if let Some(theme) = theme {
      ctx.set_visuals(theme);
    }

    if let Some(scale) = scale {
      ctx.set_pixels_per_point(scale);
    }

    let mut style = (*ctx.style()).clone();
    if style.visuals.dark_mode {
      // Make the "extreme" background color somewhat less extreme.
      style.visuals.extreme_bg_color = epaint::Color32::from_gray(20)
    }

    // Make the fonts a bit bigger.
    for font_id in style.text_styles.values_mut() {
      font_id.size *= 1.1;
    }

    let default_theme = style.visuals.clone();
    ctx.set_style(style);

    // If starting in night mode then set the dark theme.
    let night_mode = config.get_night_mode().unwrap_or(false);
    if night_mode {
      ctx.set_visuals(dark_theme());
    }

    let asset_path = if let Some(asset_path) = config.get_asset_path() {
      Some(asset_path.into())
    } else {
      dirs::download_dir()
    };

    Self {
      config,
      win_info: util::WinInfo::default(),
      default_theme,
      asset_path,
      file_dlg: None,
      find_dlg: None,
      error_dlg: None,
      select_dlg: select_dlg::SelectDlg::new(),
      select_menu: select_menu::SelectMenu::default(),
      airport_reader: None,
      chart: Chart::None,
      airport_infos: AirportInfos::None,
      long_press: touch::LongPressTracker::new(ctx),
      top_panel_height: 0,
      side_panel_width: 0,
      night_mode,
      side_panel: true,
      ui_enabled: true,
      include_nph: false,
    }
  }

  fn select_zip_file(&mut self) {
    let filter = Box::new({
      let ext = Some(OsStr::new("zip"));
      move |path: &path::Path| path.extension() == ext
    });

    let mut file_dlg = egui_file::FileDialog::open_file(self.asset_path.clone())
      .title("Open ZIP File")
      .anchor(emath::Align2::CENTER_CENTER, [0.0, 0.0])
      .default_size([525.0, 320.0])
      .show_files_filter(filter)
      .show_new_folder(false)
      .show_rename(false)
      .resizable(false);
    file_dlg.open();
    self.file_dlg = Some(file_dlg);
  }

  fn open_chart(&mut self, ctx: &egui::Context, path: &path::Path, file: &path::Path) {
    self.chart = Chart::None;

    // Concatenate the VSI prefix and the file path.
    let path = ["/vsizip/", path.to_str().unwrap()].concat();
    let path = path::Path::new(path.as_str()).join(file);

    match chart::RasterReader::new(path, ctx) {
      Ok(chart_reader) => {
        let proj4 = chart_reader.transform().get_proj4();
        let bounds = chart_reader.transform().bounds().clone();
        self.chart = Chart::Ready(Box::new(ChartInfo {
          name: util::stem_string(file).unwrap(),
          reader: rc::Rc::new(chart_reader),
          texture: None,
          disp_rect: util::Rect::default(),
          scroll: Some(emath::pos2(0.0, 0.0)),
          zoom: 1.0,
        }));

        if let Some(nasr_reader) = &mut self.airport_reader {
          nasr_reader.set_spatial_ref(proj4, bounds);
        }

        // If this is a heliport chart then include non-public heliports in searches.
        self.include_nph = util::stem_str(file).unwrap().ends_with(" HEL");
      }
      Err(err) => {
        self.error_dlg = Some(error_dlg::ErrorDlg::open(err));
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

  fn get_chart_reader(&self) -> Option<rc::Rc<chart::RasterReader>> {
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
        self.reset_airport_menu();
      }
    }
  }

  fn get_chart_texture(&self) -> Option<&(chart::ImagePart, egui::TextureHandle)> {
    if let Chart::Ready(chart) = &self.chart {
      return chart.texture.as_ref();
    }
    None
  }

  fn set_chart_image(
    &mut self,
    ctx: &egui::Context,
    part: chart::ImagePart,
    image: epaint::ColorImage,
  ) {
    if let Chart::Ready(chart) = &mut self.chart {
      let texture = ctx.load_texture("chart_image", image, Default::default());
      chart.texture = Some((part, texture));
    }
  }

  fn set_chart_disp_rect(&mut self, rect: util::Rect) {
    #[cfg(feature = "phosh")]
    let mut offset = emath::Pos2::ZERO;

    if let Chart::Ready(chart) = &mut self.chart {
      if chart.disp_rect != rect {
        #[cfg(feature = "phosh")]
        if chart.disp_rect.size.y != rect.size.y {
          // Recenter on vertical size change to account for the on-screen keyboard.
          offset.x = rect.pos.x as f32;
          offset.y = rect.pos.y as f32 + (chart.disp_rect.size.h as f32 - rect.size.h as f32) * 0.5;
        }

        chart.disp_rect = rect;
        self.reset_airport_menu();
      }
    }

    #[cfg(feature = "phosh")]
    if offset != emath::Pos2::ZERO {
      self.set_chart_scroll(offset);
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
      chart.scroll = Some(pos.floor());
    }
  }

  fn reset_airport_menu(&mut self) -> bool {
    if matches!(self.airport_infos, AirportInfos::Menu(_, _)) {
      self.airport_infos = AirportInfos::None;
      return true;
    }
    false
  }

  /// Pan the map to a NAD83 coordinate.
  fn goto_coord(&mut self, coord: util::Coord) {
    if let Some(chart) = self.get_chart() {
      if let Ok(px) = chart.reader.transform().nad83_to_px(coord) {
        let chart_size = chart.reader.transform().px_size();
        if chart_size.contains(px) {
          let x = px.x as f32 - 0.5 * chart.disp_rect.size.w as f32;
          let y = px.y as f32 - 0.5 * chart.disp_rect.size.h as f32;
          self.set_chart_zoom(1.0);
          self.set_chart_scroll(emath::pos2(x, y));
        }
      }
    }
  }

  fn toggle_side_panel(&mut self, visible: bool) {
    if self.side_panel == visible {
      return;
    }

    self.side_panel = visible;
    if let Some(chart) = self.get_chart() {
      // Scroll the chart to account for the left panel.
      let pos = chart.disp_rect.pos;
      let offset = self.side_panel_width as f32 * 0.5 + 1.0;
      let offset = if !self.side_panel {
        pos.x as f32 - offset
      } else {
        pos.x as f32 + offset
      };

      self.set_chart_scroll(emath::pos2(offset, pos.y as f32));
    }
  }

  fn get_chart_replies(&self) -> Vec<chart::RasterReply> {
    if let Some(chart_reader) = &self.get_chart_reader() {
      return chart_reader.get_replies();
    }
    Vec::new()
  }

  fn get_airport_replies(&self) -> Vec<nasr::AirportReply> {
    if let Some(airport_reader) = &self.airport_reader {
      return airport_reader.get_replies();
    }
    Vec::new()
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
      if let Some((part, _)) = self.get_chart_texture() {
        self.request_image(part.rect, part.zoom.into());
      }
    }
  }

  fn process_input(&mut self, ctx: &egui::Context) -> InputEvents {
    let mut events = InputEvents::new(ctx);
    events.secondary_click = self.long_press.check();

    ctx.input(|state| {
      // Get the window size info.
      self.win_info = util::WinInfo::new(state.viewport());

      // Process events.
      for event in &state.events {
        match event {
          egui::Event::Key {
            key,
            physical_key: _,
            pressed,
            repeat,
            modifiers,
          } if *pressed && !*repeat && self.ui_enabled => {
            match key {
              egui::Key::Escape => {
                // Remove the airport infos.
                if !self.reset_airport_menu() {
                  // No airport menu. Close the side panel.
                  self.toggle_side_panel(false);
                }
              }
              egui::Key::F if modifiers.command_only() => {
                if let Some(nasr_reader) = &self.airport_reader {
                  if nasr_reader.airport_basic_idx() && matches!(self.chart, Chart::Ready(_)) {
                    self.find_dlg = Some(find_dlg::FindDlg::open());
                    self.reset_airport_menu();
                  }
                }
              }
              egui::Key::Q if modifiers.command_only() => {
                events.quit = true;
                self.reset_airport_menu();
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
            events.zoom_pos = state.pointer.hover_pos();
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
  fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
    // Process inputs.
    let events = self.process_input(ctx);

    // Process chart raster replies.
    for reply in self.get_chart_replies() {
      match reply {
        chart::RasterReply::Image(part, image) => {
          self.set_chart_image(ctx, part, image);
        }
        chart::RasterReply::Error(_, err) => {
          println!("{err}");
        }
      }
    }

    // Process NASR airport replies.
    for reply in self.get_airport_replies() {
      match reply {
        nasr::AirportReply::Airport(info) => {
          self.goto_coord(info.coord);
        }
        nasr::AirportReply::Nearby(infos) => {
          if !infos.is_empty() {
            if let AirportInfos::Menu(_, airport_list) = &mut self.airport_infos {
              *airport_list = Some(infos);
            }
          }
        }
        nasr::AirportReply::Search(infos) => match infos.len() {
          0 => unreachable!(),
          1 => self.goto_coord(infos[0].coord),
          _ => self.airport_infos = AirportInfos::Dialog(infos),
        },
        nasr::AirportReply::Error(err) => {
          self.error_dlg = Some(error_dlg::ErrorDlg::open(err));
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
            // Save the folder path.
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

                    // Remove the chart spatial reference from the airport reader.
                    if let Some(airport_reader) = &self.airport_reader {
                      airport_reader.clear_spatial_ref();
                    }
                  } else {
                    self.open_chart(ctx, &path, files.first().unwrap());
                  }
                }
                util::ZipInfo::Aero { apt: csv, shp: _ } => {
                  // Concatenate the VSI prefix and the file path.
                  let path = ["/vsizip//vsizip/", path.to_str().unwrap()].concat();
                  let path = path::Path::new(path.as_str());
                  let path = path.join(csv).join("APT_BASE.csv");
                  self.airport_reader = match nasr::AirportReader::new(path, ctx) {
                    Ok(nasr_reader) => {
                      if let Some(chart_reader) = self.get_chart_reader() {
                        let proj4 = chart_reader.transform().get_proj4();
                        let bounds = chart_reader.transform().bounds().clone();
                        nasr_reader.set_spatial_ref(proj4, bounds);
                      }
                      Some(nasr_reader)
                    }
                    Err(err) => {
                      self.error_dlg = Some(error_dlg::ErrorDlg::open(err));
                      None
                    }
                  }
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
          // Clone the parameters in order to avoid simultaneously borrowing self as immutable and mutable.
          self.open_chart(ctx, &path.clone(), &files[index].clone());
        } else {
          self.chart = Chart::None;
        }
      }
    }

    // Show the selection dialog if there's an airport choice to be made.
    if let AirportInfos::Dialog(infos) = &self.airport_infos {
      self.ui_enabled = false;
      let iter = infos.iter().map(|info| info.desc.as_str());
      if let Some(response) = self.select_dlg.show(ctx, iter) {
        self.ui_enabled = true;
        if let select_dlg::Response::Index(index) = response {
          self.goto_coord(infos[index].coord);
        }
        self.airport_infos = AirportInfos::None;
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
          self.ui_enabled = true;
          self.find_dlg = None;
          if let Some(nasr_reader) = &self.airport_reader {
            nasr_reader.search(term, self.include_nph);
          }
        }
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

    // Show airport choices in a popup.
    if let AirportInfos::Menu(lat_lon, infos) = &self.airport_infos {
      let infos = infos.as_ref();
      let iter = infos.map(|v| v.iter().map(|info| info.desc.as_str()));
      if let Some(_response) = self.select_menu.show(ctx, lat_lon, iter) {
        self.airport_infos = AirportInfos::None;
      }
    }

    self.top_panel_height = top_panel(self.top_panel_height, ctx, |ui| {
      ui.set_enabled(self.ui_enabled);
      ui.horizontal_centered(|ui| {
        let widget = egui::SelectableLabel::new(self.side_panel, " ⚙ ");
        if ui.add_sized([0.0, 21.0], widget).clicked() {
          self.toggle_side_panel(!self.side_panel);
        }

        if let Some(nasr_reader) = &self.airport_reader {
          if nasr_reader.airport_basic_idx() {
            let text = 'text: {
              const APT: &str = "APT";
              if nasr_reader.request_count() > 0 {
                ctx.output_mut(|state| state.cursor_icon = egui::CursorIcon::Progress);
                break 'text egui::RichText::new(APT).strong();
              }
              egui::RichText::new(APT)
            };

            ui.separator();
            ui.label(text);
          }
        }

        if let Chart::Ready(chart) = &mut self.chart {
          if let Some(nasr_reader) = &self.airport_reader {
            if nasr_reader.airport_spatial_idx() && ui.button("🔎").clicked() {
              self.find_dlg = Some(find_dlg::FindDlg::open());
            }
          }

          ui.separator();
          ui.label(&chart.name);

          ui.with_layout(egui::Layout::right_to_left(emath::Align::Center), |ui| {
            // Zoom-in button.
            ui.add_enabled_ui(chart.zoom < 1.0, |ui| {
              if let Some(font_id) = ui.style().text_styles.get(&egui::TextStyle::Monospace) {
                let text = egui::RichText::new("+").font(font_id.clone());
                let widget = egui::Button::new(text);
                if ui.add_sized([21.0, 21.0], widget).clicked() {
                  let new_zoom = (chart.zoom * 2.0).min(1.0);
                  if new_zoom != chart.zoom {
                    chart.scroll = Some(chart.get_zoom_pos(new_zoom).round());
                    chart.zoom = new_zoom;
                  }
                }
              }
            });

            // Zoom-out button.
            let min_zoom = chart.get_min_zoom();
            ui.add_enabled_ui(chart.zoom > min_zoom, |ui| {
              if let Some(font_id) = ui.style().text_styles.get(&egui::TextStyle::Monospace) {
                let text = egui::RichText::new("-").font(font_id.clone());
                let widget = egui::Button::new(text);
                if ui.add_sized([21.0, 21.0], widget).clicked() {
                  let new_zoom = (chart.zoom * 0.5).max(min_zoom);
                  if new_zoom != chart.zoom {
                    chart.scroll = Some(chart.get_zoom_pos(new_zoom).round());
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

        ui.spacing_mut().scroll.bar_inner_margin = 0.0;

        let response = widget.show(ui, |ui| {
          let cursor_pos = ui.cursor().left_top();
          let size = reader.transform().px_size();
          let size = emath::vec2(size.w as f32, size.h as f32) * zoom;
          let rect = emath::Rect::from_min_size(cursor_pos, size);

          // Reserve space for the scroll bars.
          ui.allocate_rect(rect, egui::Sense::hover());

          // Place the image.
          if let Some((part, texture)) = self.get_chart_texture() {
            let scale = zoom * part.zoom.inverse();
            let rect = util::scale_rect(part.rect.into(), scale);
            let rect = rect.translate(cursor_pos.to_vec2());
            ui.allocate_ui_at_rect(rect, |ui| {
              let mut clip = ui.clip_rect();
              clip.max -= emath::Vec2::splat(ui.spacing().scroll.bar_width * 0.5);
              ui.set_clip_rect(clip);
              ui.image((texture.id(), rect.size()));
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

        // Make sure the image position lands on an even pixel.
        if response.state.velocity() == emath::vec2(0.0, 0.0) {
          let floored = pos.floor();
          if floored != pos {
            self.set_chart_scroll(emath::pos2(floored.x, floored.y));
          }
        }

        // Get the minimum zoom.
        let min_zoom = self.get_chart().unwrap().get_min_zoom();

        if let Some((part, _)) = self.get_chart_texture() {
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
              self.set_chart_scroll(pos.to_pos2().round());

              ctx.request_repaint();
            }
          }
        }

        if let Some(click_pos) = events.secondary_click {
          // Make sure the clicked position is actually over the chart area.
          if response.inner_rect.contains(click_pos) {
            let pos = (click_pos - response.inner_rect.min + pos) / zoom;
            let lcc = reader.transform().px_to_chart(pos.into());
            if let Ok(nad83) = reader.transform().chart_to_nad83(lcc) {
              let lat = util::format_lat(nad83.y);
              let lon = util::format_lon(nad83.x);
              self.select_menu.set_pos(click_pos);
              self.airport_infos = AirportInfos::Menu(format!("{lat}, {lon}"), None);
              if let Some(nasr_reader) = &self.airport_reader {
                if nasr_reader.airport_spatial_idx() {
                  // 1/2 nautical mile (926 meters) is the search radius at 1.0x zoom.
                  let radius = 926.0 / zoom as f64;
                  nasr_reader.nearby(lcc, radius, self.include_nph);
                }
              }
            }
          }
        }
      }
    });

    if events.quit {
      ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
  }

  fn on_exit(&mut self, _gl: Option<&glow::Context>) {
    self.config.set_win_info(&self.win_info);
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
}

enum AirportInfos {
  None,
  Menu(String, Option<Vec<nasr::AirportInfo>>),
  Dialog(Vec<nasr::AirportInfo>),
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
  reader: rc::Rc<chart::RasterReader>,
  texture: Option<(chart::ImagePart, egui::TextureHandle)>,
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
  height: u32,
  ctx: &egui::Context,
  contents: impl FnOnce(&mut egui::Ui) -> R,
) -> u32 {
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
    .default_height(height as f32)
    .show(ctx, contents);

  // Round up the width.
  response.response.rect.height().ceil() as u32
}

fn side_panel<R>(
  width: u32,
  ctx: &egui::Context,
  contents: impl FnOnce(&mut egui::Ui) -> R,
) -> u32 {
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
    .default_width(width as f32)
    .show(ctx, contents);

  // Round up the width.
  response.response.rect.width().ceil() as u32
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
