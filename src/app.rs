use crate::{
  chart, error_dlg, find_dlg, nasr, select_dlg, select_menu,
  util::{self, NONE_ERR},
};
use eframe::{
  egui::{self, scroll_area},
  emath, epaint,
};
use std::{
  collections, ffi, path,
  sync::{self, mpsc},
  thread, time,
};

struct InputEvents {
  zoom_mod: f32,
  zoom_pos: Option<epaint::Pos2>,
  secondary_click: bool,
  quit: bool,
}

impl InputEvents {
  fn new() -> Self {
    Self {
      zoom_mod: 1.0,
      zoom_pos: None,
      secondary_click: false,
      quit: false,
    }
  }
}

pub struct App {
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
  touch_tracker: TouchTracker,
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
    let night_mode = to_bool(cc.storage.expect(NONE_ERR).get_string(NIGHT_MODE_KEY));
    if night_mode {
      cc.egui_ctx.set_visuals(dark_theme());
    }

    let asset_path =
      if let Some(asset_path) = cc.storage.expect(NONE_ERR).get_string(ASSET_PATH_KEY) {
        Some(asset_path.into())
      } else {
        dirs::download_dir()
      };

    Self {
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
      touch_tracker: TouchTracker::new(cc.egui_ctx.clone()),
      save_window,
      night_mode,
      side_panel: true,
      ui_enabled: true,
    }
  }

  fn select_chart_zip(&mut self) {
    let filter = Box::new(|path: &path::Path| -> bool {
      return path.extension() == Some(ffi::OsStr::new("zip"));
    });

    let mut file_dlg = egui_file::FileDialog::open_file(self.asset_path.clone())
      .anchor(emath::Align2::CENTER_CENTER, [0.0, 0.0])
      .default_size([525.0, 320.0])
      .filter(filter)
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
          name: util::file_stem(file).expect(NONE_ERR),
          source: sync::Arc::new(source),
          image: None,
          requests: collections::HashSet::new(),
          disp_rect: util::Rect::default(),
          scroll: Some(emath::Pos2::new(0.0, 0.0)),
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
      if chart.disp_rect != rect {
        chart.disp_rect = rect;

        // Reset the choices on rect change.
        self.choices = None;
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
      chart.scroll = Some(pos);
    }
  }

  fn get_next_chart_reply(&self) -> Option<chart::Reply> {
    if let Some(chart_source) = &self.get_chart_source() {
      return chart_source.get_next_reply();
    }
    None
  }

  fn get_next_apt_reply(&self) -> Option<nasr::APTReply> {
    if let Some(apt_source) = &self.apt_source {
      return apt_source.get_next_reply();
    }
    None
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
      storage.set_string(NIGHT_MODE_KEY, format!("{night_mode}"));

      // Request a new image.
      if let Some((part, _)) = self.get_chart_image() {
        self.request_image(part.rect, part.zoom.into());
      }
    }
  }

  fn process_input_events(&mut self, ctx: &egui::Context) -> InputEvents {
    let mut app_events = InputEvents::new();

    if let Some(multi_touch) = ctx.multi_touch() {
      app_events.zoom_pos = Some(multi_touch.start_pos);
      app_events.zoom_mod = multi_touch.zoom_delta;
    }

    self.touch_tracker.update();
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
                app_events.quit = true;
                self.choices = None;
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
          } => self.touch_tracker.set(*id, *phase, *pos),
          egui::Event::PointerButton {
            pos: _,
            button,
            pressed,
            modifiers,
          } if *button == egui::PointerButton::Secondary && !pressed && modifiers.is_none() => {
            app_events.secondary_click = true;
          }
          egui::Event::Zoom(val) => {
            app_events.zoom_pos = ctx.pointer_hover_pos();
            app_events.zoom_mod *= val;
          }
          _ => (),
        }
      }
    });
    app_events
  }

  fn get_secondary_click_pos(
    &mut self,
    app_events: &InputEvents,
    ctx: &egui::Context,
  ) -> Option<epaint::Pos2> {
    if app_events.secondary_click {
      if let Some(hover_pos) = ctx.pointer_hover_pos() {
        return Some(hover_pos);
      }
    }

    if let Some(touch_pos) = self.touch_tracker.pos.take() {
      return Some(touch_pos);
    }

    None
  }
}

impl eframe::App for App {
  fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
    // Process inputs.
    let app_events = self.process_input_events(ctx);

    // Process chart source replies.
    while let Some(reply) = self.get_next_chart_reply() {
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
          println!("GdalError: ({part:?}) {err:?}");
        }
      }
    }

    // Process NASR airport replies.
    while let Some(reply) = self.get_next_apt_reply() {
      match reply {
        nasr::APTReply::Airport(airport) => {
          if let Some(airport) = airport {
            if let Chart::Ready(chart) = &self.chart {
              if let Ok(coord) = chart.source.transform().nad83_to_px(airport.coord) {
                let x = coord.x as f32 - 0.5 * chart.disp_rect.size.w as f32;
                let y = coord.y as f32 - 0.5 * chart.disp_rect.size.h as f32;
                if x > 0.0
                  && y > 0.0
                  && x < chart.source.transform().px_size().w as f32
                  && y < chart.source.transform().px_size().h as f32
                {
                  self.set_chart_zoom(1.0);
                  self.set_chart_scroll(emath::Pos2::new(x, y));
                }
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

    // Show the file dialog if set.
    if let Some(file_dlg) = &mut self.file_dlg {
      if file_dlg.show(ctx).visible() {
        self.ui_enabled = false;
      } else {
        if file_dlg.selected() {
          if let Some(path) = file_dlg.path() {
            // Save the path.
            if let Some(path) = path.parent().and_then(|p| p.to_str()) {
              let storage = frame.storage_mut().expect(NONE_ERR);
              storage.set_string(ASSET_PATH_KEY, path.into());
              self.asset_path = Some(path.into());
            }

            match util::get_zip_info(&path) {
              Ok(info) => match info {
                util::ZipInfo::Chart(files) => {
                  if files.len() > 1 {
                    self.chart = Chart::Load(path, files);
                  } else {
                    self.open_chart(ctx, &path, files.first().expect(NONE_ERR));
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
                    let err_txt = format!("Not found: {errors:?}");
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
      let choices = files
        .iter()
        .map(|f| util::file_stem(f).expect(NONE_ERR))
        .collect();
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
            ctx.output_mut(|state| state.cursor_icon = egui::CursorIcon::Progress);
            egui::RichText::new(APT).strong()
          } else {
            egui::RichText::new(APT)
          };

          ui.separator();
          ui.label(text);
        }

        if let Chart::Ready(chart) = &mut self.chart {
          if self.apt_source.is_some() && ui.button("🔎").clicked() {
            self.find_dlg = Some(find_dlg::FindDlg::open());
          }

          ui.separator();
          ui.label(&chart.name);

          ui.with_layout(egui::Layout::right_to_left(emath::Align::Center), |ui| {
            if let Some(font_id) = ui.style().text_styles.get(&egui::TextStyle::Monospace) {
              let font_id = font_id.clone();
              let plus = egui::RichText::new("+").font(font_id.clone());
              if ui.button(plus).clicked() {
                let new_zoom = (chart.zoom * 1.25).min(1.0);
                if new_zoom != chart.zoom {
                  let pos: emath::Pos2 = chart.disp_rect.pos.into();
                  let size: emath::Vec2 = chart.disp_rect.size.into();
                  let offset = size * 0.5;
                  let ratio = new_zoom / chart.zoom;
                  let x = ratio * (pos.x + offset.x) - offset.x;
                  let y = ratio * (pos.y + offset.y) - offset.y;
                  chart.scroll = Some(emath::Pos2::new(x, y));
                  chart.zoom = new_zoom;
                }
              }

              let minus = egui::RichText::new("-").font(font_id);
              if ui.button(minus).clicked() {
                let chart_size: emath::Vec2 = chart.source.transform().px_size().into();
                let size: emath::Vec2 = chart.disp_rect.size.into();
                let sw = size.x / chart_size.x;
                let sh = size.y / chart_size.y;
                let new_zoom = (chart.zoom * 0.8).max(sw.max(sh));
                if new_zoom != chart.zoom {
                  let pos: emath::Pos2 = chart.disp_rect.pos.into();
                  let offset = size * 0.5;
                  let ratio = new_zoom / chart.zoom;
                  let x = ratio * (pos.x + offset.x) - offset.x;
                  let y = ratio * (pos.y + offset.y) - offset.y;
                  chart.scroll = Some(emath::Pos2::new(x, y));
                  chart.zoom = new_zoom;
                }
              }
            }
          });
        }
      });
    });

    if self.side_panel {
      side_panel(ctx, |ui| {
        ui.set_enabled(self.ui_enabled);

        ui.horizontal(|ui| {
          let button = egui::Button::new("Open Zip File");
          if ui.add_sized(ui.available_size(), button).clicked() {
            self.select_chart_zip();
          }
        });

        ui.add_space(ui.spacing().item_spacing.y);
        ui.separator();

        ui.horizontal(|ui| {
          let mut night_mode = self.night_mode;
          if ui.checkbox(&mut night_mode, "Night Mode").clicked() {
            let storage = frame.storage_mut().expect(NONE_ERR);
            self.set_night_mode(ctx, storage, night_mode);
          }
        });
      });
    }

    central_panel(ctx, self.side_panel, |ui| {
      ui.set_enabled(self.ui_enabled);
      if let Some(source) = self.get_chart_source() {
        let zoom = self.get_chart_zoom().expect(NONE_ERR);
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
          let size = source.transform().px_size();
          let size = emath::Vec2::new(size.w as f32, size.h as f32) * zoom;
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

        if let Some(zoom_pos) = app_events.zoom_pos {
          if response.inner_rect.contains(zoom_pos) {
            let new_zoom = zoom * app_events.zoom_mod;
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

        if let Some(click_pos) = self.get_secondary_click_pos(&app_events, ctx) {
          // Make sure we're actually over the chart area.
          if response.inner_rect.contains(click_pos) {
            if let Some(apt_source) = &self.apt_source {
              let pos = (click_pos - response.inner_rect.min + pos) / zoom;
              let coord = source.transform().px_to_chart(pos.into());

              // 1/2 nautical mile (926 meters) is the search radius at 1.0x zoom.
              apt_source.nearby(coord, 926.0 / zoom as f64);

              if let Ok(coord) = source.transform().chart_to_nad83(coord) {
                let lat = util::format_lat(coord.y);
                let lon = util::format_lon(coord.x);
                self.select_menu.set_pos(click_pos);
                self.choices = Some(vec![format!("{lat}, {lon}")]);
              }
            }
          }
        }
      }
    });

    if app_events.quit {
      frame.close();
    }
  }

  fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
    let color = if visuals.dark_mode {
      visuals.extreme_bg_color
    } else {
      epaint::Color32::from_gray(220)
    };

    [
      color[0] as f32 / 255.0,
      color[1] as f32 / 255.0,
      color[2] as f32 / 255.0,
      color[3] as f32 / 255.0,
    ]
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

enum Request {
  Refresh(f64),
  Clear,
  Exit,
}

struct TouchInfo {
  time: time::SystemTime,
  pos: epaint::Pos2,
}

struct TouchTracker {
  sender: mpsc::Sender<Request>,
  thread: Option<thread::JoinHandle<()>>,
  ids: collections::HashSet<u64>,
  info: Option<TouchInfo>,
  pos: Option<epaint::Pos2>,
}

impl TouchTracker {
  fn new(ctx: egui::Context) -> Self {
    let (sender, receiver) = mpsc::channel();
    let thread = Some(
      thread::Builder::new()
        .name("app::TouchTracker thread".to_owned())
        .spawn(move || loop {
          let mut request = receiver.recv().expect(util::FAIL_ERR);
          let mut refresh_secs;
          loop {
            match request {
              Request::Refresh(secs) => refresh_secs = Some(secs),
              Request::Clear => refresh_secs = None,
              Request::Exit => return,
            }

            // Check for another request.
            match receiver.try_recv() {
              Ok(rqst) => request = rqst,
              Err(_) => break,
            }
          }

          if let Some(secs) = refresh_secs.take() {
            thread::sleep(time::Duration::from_secs_f64(secs));
            ctx.request_repaint();
          }
        })
        .expect(util::FAIL_ERR),
    );

    Self {
      sender,
      thread,
      ids: collections::HashSet::new(),
      info: None,
      pos: None,
    }
  }

  fn set(&mut self, id: egui::TouchId, phase: egui::TouchPhase, pos: epaint::Pos2) {
    match phase {
      egui::TouchPhase::Start => {
        if self.ids.is_empty() {
          let time = time::SystemTime::now();
          self.info = Some(TouchInfo { time, pos });
          self.ids.insert(id.0);

          let refresh = Request::Refresh(1.0);
          self.sender.send(refresh).expect(util::FAIL_ERR);
        } else {
          self.info = None;
          self.sender.send(Request::Clear).expect(util::FAIL_ERR);
        }
      }
      _ => {
        self.ids.remove(&id.0);
        self.info = None;
        self.sender.send(Request::Clear).expect(util::FAIL_ERR);
      }
    }
  }

  fn update(&mut self) {
    if let Some(info) = self.info.take() {
      if let Ok(dur) = time::SystemTime::now().duration_since(info.time) {
        if dur.as_secs_f64() > 1.0 {
          self.pos = Some(info.pos);
          return;
        }
        self.info = Some(info);
      }
    }
  }
}

impl Drop for TouchTracker {
  fn drop(&mut self) {
    // Send an exit request.
    self.sender.send(Request::Exit).expect(util::FAIL_ERR);
    if let Some(thread) = self.thread.take() {
      // Wait for the thread to join.
      thread.join().expect(util::FAIL_ERR);
    }
  }
}

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

  egui::TopBottomPanel::top(format!("{}_top_panel", util::APP_NAME))
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
    .show(ctx, contents);
}

fn side_panel<R>(ctx: &egui::Context, contents: impl FnOnce(&mut egui::Ui) -> R) {
  let style = ctx.style();
  let fill = if style.visuals.dark_mode {
    epaint::Color32::from_gray(35)
  } else {
    style.visuals.window_fill()
  };

  egui::SidePanel::left(format!("{}_side_panel", util::APP_NAME))
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
  let top = 1.0;
  let min = emath::Pos2::new(available.min.x + left, available.min.y + top);
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
