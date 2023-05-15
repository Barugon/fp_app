use crate::util::{self, FAIL_ERR, NONE_ERR};
use eframe::egui;
use gdal::{spatial_ref, vector};
use std::{
  any, collections, path,
  sync::{self, atomic, mpsc},
  thread,
};

// NASR = National Airspace System Resources

pub struct Reader {
  request_count: atomic::AtomicI64,
  sender: mpsc::Sender<Request>,
  receiver: mpsc::Receiver<Reply>,
  thread: Option<thread::JoinHandle<()>>,
  apt_status: AptDataStatus,
}

impl Reader {
  pub fn new(ctx: &egui::Context) -> Self {
    let mut apt_data_status = AptDataStatus::new();
    let apt_status = apt_data_status.clone();
    let ctx = ctx.clone();

    // Create the communication channels.
    let (sender, thread_receiver) = mpsc::channel();
    let (thread_sender, receiver) = mpsc::channel();

    // Create the thread.
    let thread = thread::Builder::new()
      .name(any::type_name::<AptSource>().into())
      .spawn(move || {
        let nad83 = spatial_ref::SpatialRef::from_epsg(4269).expect(FAIL_ERR);
        nad83.set_axis_mapping_strategy(0);

        // Airport source.
        let mut apt_source: Option<AptSource> = None;

        // Chart transformation.
        let mut to_chart: Option<spatial_ref::CoordTransform> = None;

        let send_ctx = ctx.clone();
        let send = move |reply: Reply| {
          thread_sender.send(reply).expect(FAIL_ERR);
          send_ctx.request_repaint();
        };

        loop {
          // Wait for the next message.
          let request = thread_receiver.recv().expect(FAIL_ERR);
          match request {
            Request::Open(path) => {
              if let Ok(mut source) = AptSource::open(&path) {
                apt_data_status.set_is_loaded(true);
                apt_data_status.set_has_id_idx(!source.id_idx.is_empty());

                // A new airport source was opened; (re)make the spatial index.
                source.set_to_chart(&to_chart);
                apt_data_status.set_has_sp_idx(source.sp_idx.size() != 0);

                apt_source = Some(source);
                ctx.request_repaint();
              }
            }
            Request::SpatialRef(proj4) => {
              match spatial_ref::SpatialRef::from_proj4(&proj4) {
                Ok(sr) => match spatial_ref::CoordTransform::new(&nad83, &sr) {
                  Ok(trans) => {
                    to_chart = Some(trans);

                    if let Some(source) = &mut apt_source {
                      // A new chart was opened; (re)make the airport spatial index.
                      source.set_to_chart(&to_chart);
                      apt_data_status.set_has_sp_idx(source.sp_idx.size() != 0);
                      ctx.request_repaint();
                    }
                  }
                  Err(_err) => {
                    debugln!("{_err}");
                  }
                },
                Err(_err) => {
                  debugln!("{_err}");
                }
              }
            }
            Request::Airport(id) => {
              let mut coord = None;
              if let Some(source) = &apt_source {
                use vector::LayerAccess;

                let layer = source.layer();
                let id = id.to_uppercase();

                // Get the airport matching the ID.
                if let Some(fid) = source.id_idx.get(&id) {
                  coord = layer.feature(*fid).and_then(|feature| feature.get_coord());
                }
              }

              send(Reply::Airport(coord));
            }
            Request::Nearby(coord, dist) => {
              let mut airports = Vec::new();
              if let Some(source) = &apt_source {
                use vector::LayerAccess;

                let layer = source.layer();
                let coord = [coord.x, coord.y];
                let dsq = dist * dist;

                // Find nearby airports using the spatial index.
                for item in source.sp_idx.locate_within_distance(coord, dsq) {
                  if let Some(info) = layer.feature(item.fid).and_then(AptInfo::new) {
                    airports.push(info);
                  }
                }
              }

              send(Reply::Nearby(airports));
            }
            Request::Search(term) => {
              let mut airports = Vec::new();
              if let Some(source) = &apt_source {
                use vector::LayerAccess;

                let mut layer = source.layer();
                let term = term.to_uppercase();

                // Find the airports with names containing the search term.
                for feature in layer.features() {
                  if let Some(name) = feature.get_string("ARPT_NAME") {
                    if name.contains(&term) {
                      if let Some(info) = AptInfo::new(feature) {
                        airports.push(info);
                      }
                    }
                  }
                }
              }

              send(Reply::Search(airports));
            }
            Request::Exit => return,
          }
        }
      })
      .expect(FAIL_ERR);

    Self {
      request_count: atomic::AtomicI64::new(0),
      sender,
      receiver,
      thread: Some(thread),
      apt_status,
    }
  }

  /// Open a NASR CSV zip file.
  pub fn open(&self, path: path::PathBuf) {
    let request = Request::Open(path);
    self.sender.send(request).expect(FAIL_ERR);
  }

  pub fn apt_status(&self) -> AptStatus {
    self.apt_status.status()
  }

  /// Set the spatial reference using a PROJ4 string.
  /// - `proj4`: PROJ4 text
  pub fn set_spatial_ref(&self, proj4: String) {
    let request = Request::SpatialRef(proj4);
    self.sender.send(request).expect(FAIL_ERR);
  }

  /// Lookup airport information using it's identifier.
  /// - `id`: airport id
  pub fn airport(&self, id: String) {
    if !id.is_empty() {
      self.sender.send(Request::Airport(id)).expect(FAIL_ERR);
      self.request_count.fetch_add(1, atomic::Ordering::Relaxed);
    }
  }

  /// Request nearby airports.
  /// - `coord`: the chart coordinate (LCC)
  /// - `dist`: the search distance in meters
  pub fn nearby(&self, coord: util::Coord, dist: f64) {
    if dist >= 0.0 {
      let request = Request::Nearby(coord, dist);
      self.sender.send(request).expect(FAIL_ERR);
      self.request_count.fetch_add(1, atomic::Ordering::Relaxed);
    }
  }

  /// Find airport names that match the text.
  /// - `term`: search term
  #[allow(unused)]
  pub fn search(&self, term: String) {
    if !term.is_empty() {
      self.sender.send(Request::Search(term)).expect(FAIL_ERR);
      self.request_count.fetch_add(1, atomic::Ordering::Relaxed);
    }
  }

  pub fn get_next_reply(&self) -> Option<Reply> {
    let reply = self.receiver.try_recv().ok();
    if reply.is_some() {
      assert!(self.request_count.fetch_sub(1, atomic::Ordering::Relaxed) > 0);
    }
    reply
  }

  pub fn request_count(&self) -> i64 {
    self.request_count.load(atomic::Ordering::Relaxed)
  }
}

impl Drop for Reader {
  fn drop(&mut self) {
    // Send an exit request.
    self.sender.send(Request::Exit).expect(FAIL_ERR);
    if let Some(thread) = self.thread.take() {
      // Wait for the thread to join.
      thread.join().expect(FAIL_ERR);
    }
  }
}

enum Request {
  Open(path::PathBuf),
  SpatialRef(String),
  Airport(String),
  Nearby(util::Coord, f64),
  Search(String),
  Exit,
}

pub enum Reply {
  Airport(Option<util::Coord>),
  Nearby(Vec<AptInfo>),
  Search(Vec<AptInfo>),
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub enum AptStatus {
  None,
  Loaded,
  HasIdIdx,
  HasSpIdx,
}

#[derive(Clone)]
struct AptDataStatus {
  status: sync::Arc<atomic::AtomicU8>,
}

impl AptDataStatus {
  fn new() -> Self {
    let status = AptStatus::None as u8;
    let status = atomic::AtomicU8::new(status);
    Self {
      status: sync::Arc::new(status),
    }
  }

  fn set_is_loaded(&mut self, loaded: bool) {
    if loaded {
      let status = AptStatus::Loaded as u8;
      self.status.store(status, atomic::Ordering::Relaxed);
    }
  }

  fn set_has_id_idx(&mut self, has_idx: bool) {
    if has_idx {
      let status = AptStatus::HasIdIdx as u8;
      self.status.store(status, atomic::Ordering::Relaxed);
    }
  }

  fn set_has_sp_idx(&mut self, has_idx: bool) {
    if has_idx {
      let status = AptStatus::HasSpIdx as u8;
      self.status.store(status, atomic::Ordering::Relaxed);
    }
  }

  fn status(&self) -> AptStatus {
    const NONE: u8 = AptStatus::None as u8;
    const LOADED: u8 = AptStatus::Loaded as u8;
    const HAS_ID: u8 = AptStatus::HasIdIdx as u8;
    const HAS_SP: u8 = AptStatus::HasSpIdx as u8;
    match self.status.load(atomic::Ordering::Relaxed) {
      NONE => AptStatus::None,
      LOADED => AptStatus::Loaded,
      HAS_ID => AptStatus::HasIdIdx,
      HAS_SP => AptStatus::HasSpIdx,
      _ => unreachable!(),
    }
  }
}

/// AptSource is used for opening and reading [NASR airport data](https://www.faa.gov/air_traffic/flight_info/aeronav/aero_data/NASR_Subscription/) in zipped CSV format.
struct AptSource {
  dataset: gdal::Dataset,
  id_idx: collections::HashMap<String, u64>,
  sp_idx: rstar::RTree<AptLocIdx>,
}

impl AptSource {
  const fn csv_name() -> &'static str {
    "APT_BASE.csv"
  }

  fn open_options<'a>() -> gdal::DatasetOptions<'a> {
    gdal::DatasetOptions {
      open_flags: gdal::GdalOpenFlags::GDAL_OF_READONLY | gdal::GdalOpenFlags::GDAL_OF_VECTOR,
      ..Default::default()
    }
  }

  /// Open an airport data source.
  /// - `path`: CSV zip file path
  /// - `ctx`: egui context for requesting a repaint
  fn open(path: &path::Path) -> Result<Self, gdal::errors::GdalError> {
    use gdal::vector::LayerAccess;

    // Concatenate the VSI prefix and the file name.
    let path = ["/vsizip/", path.to_str().expect(NONE_ERR)].concat();
    let path = path::Path::new(path.as_str()).join(AptSource::csv_name());

    // Open the dataset and check for a layer.
    let dataset = gdal::Dataset::open_ex(path, Self::open_options())?;
    let mut layer = dataset.layer(0)?;

    let id_idx = {
      let mut map = collections::HashMap::new();
      for feature in layer.features() {
        if let Some(fid) = feature.fid() {
          if let Some(id) = feature.get_string("ARPT_ID") {
            map.insert(id, fid);
          }
        }
      }
      map
    };

    Ok(Self {
      dataset,
      id_idx,
      sp_idx: rstar::RTree::new(),
    })
  }

  fn set_to_chart(&mut self, trans: &Option<spatial_ref::CoordTransform>) {
    self.sp_idx = {
      let mut tree = rstar::RTree::new();
      if let Some(trans) = trans {
        use util::Transform;
        use vector::LayerAccess;

        let mut layer = self.layer();
        for feature in layer.features() {
          if let Some(fid) = feature.fid() {
            let coord = feature.get_coord().and_then(|c| trans.transform(c).ok());
            if let Some(coord) = coord {
              tree.insert(AptLocIdx { coord, fid })
            }
          }
        }
      }
      tree
    };
  }

  fn layer(&self) -> vector::Layer {
    self.dataset.layer(0).expect(FAIL_ERR)
  }
}

/// Airport location spatial index item.
struct AptLocIdx {
  coord: util::Coord,
  fid: u64,
}

impl rstar::RTreeObject for AptLocIdx {
  type Envelope = rstar::AABB<[f64; 2]>;

  fn envelope(&self) -> Self::Envelope {
    Self::Envelope::from_point([self.coord.x, self.coord.y])
  }
}

impl rstar::PointDistance for AptLocIdx {
  fn distance_2(
    &self,
    point: &<Self::Envelope as rstar::Envelope>::Point,
  ) -> <<Self::Envelope as rstar::Envelope>::Point as rstar::Point>::Scalar {
    let dx = point[0] - self.coord.x;
    let dy = point[1] - self.coord.y;
    dx * dx + dy * dy
  }
}

#[derive(Debug)]
pub struct AptInfo {
  pub id: String,
  pub name: String,
  pub coord: util::Coord,
  pub site_type: SiteType,
  pub site_use: SiteUse,
}

impl AptInfo {
  fn new(feature: vector::Feature) -> Option<Self> {
    let id = feature.get_string("ARPT_ID")?;
    let name = feature.get_string("ARPT_NAME")?;
    let coord = feature.get_coord()?;
    let site_type = feature.get_site_type()?;
    let site_use = feature.get_site_use()?;
    Some(Self {
      id,
      name,
      coord,
      site_type,
      site_use,
    })
  }
}

trait GetF64 {
  fn get_f64(&self, field: &str) -> Option<f64>;
}

impl GetF64 for vector::Feature<'_> {
  fn get_f64(&self, field: &str) -> Option<f64> {
    match self.field_as_double_by_name(field) {
      Ok(val) => val,
      Err(err) => {
        println!("{err}");
        None
      }
    }
  }
}

trait GetString {
  fn get_string(&self, field: &str) -> Option<String>;
}

impl GetString for vector::Feature<'_> {
  fn get_string(&self, field: &str) -> Option<String> {
    match self.field_as_string_by_name(field) {
      Ok(val) => val,
      Err(err) => {
        println!("{err}");
        None
      }
    }
  }
}

#[derive(Eq, Debug, PartialEq)]
pub enum SiteType {
  Airport,
  Balloon,
  Seaplane,
  Glider,
  Helicopter,
  Ultralight,
}

trait GetSiteType {
  fn get_site_type(&self) -> Option<SiteType>;
}

impl GetSiteType for vector::Feature<'_> {
  fn get_site_type(&self) -> Option<SiteType> {
    match self.get_string("SITE_TYPE_CODE")?.as_str() {
      "A" => Some(SiteType::Airport),
      "B" => Some(SiteType::Balloon),
      "C" => Some(SiteType::Seaplane),
      "G" => Some(SiteType::Glider),
      "H" => Some(SiteType::Helicopter),
      "U" => Some(SiteType::Ultralight),
      _ => None,
    }
  }
}

#[derive(Eq, Debug, PartialEq)]
pub enum SiteUse {
  Public,
  Private,
  AirForce,
  Navy,
  Army,
  CoastGuard,
}

trait GetSiteUse {
  fn get_site_use(&self) -> Option<SiteUse>;
}

impl GetSiteUse for vector::Feature<'_> {
  fn get_site_use(&self) -> Option<SiteUse> {
    match self.get_string("OWNERSHIP_TYPE_CODE")?.as_str() {
      "PU" | "PR" => Some(if self.get_string("FACILITY_USE_CODE")? == "PR" {
        SiteUse::Private
      } else {
        SiteUse::Public
      }),
      "MA" => Some(SiteUse::AirForce),
      "MN" => Some(SiteUse::Navy),
      "MR" => Some(SiteUse::Army),
      "CG" => Some(SiteUse::CoastGuard),
      _ => None,
    }
  }
}

trait GetCoord {
  fn get_coord(&self) -> Option<util::Coord>;
}

impl GetCoord for vector::Feature<'_> {
  fn get_coord(&self) -> Option<util::Coord> {
    let lat_deg = self.get_f64("LAT_DEG")?;
    let lat_min = self.get_f64("LAT_MIN")?;
    let lat_sec = self.get_f64("LAT_SEC")?;
    let lat_hemis = self.get_string("LAT_HEMIS")?;
    let lat_deg = if lat_hemis.eq_ignore_ascii_case("S") {
      -lat_deg
    } else {
      lat_deg
    };

    let lon_deg = self.get_f64("LONG_DEG")?;
    let lon_min = self.get_f64("LONG_MIN")?;
    let lon_sec = self.get_f64("LONG_SEC")?;
    let lon_hemis = self.get_string("LONG_HEMIS")?;
    let lon_deg = if lon_hemis.eq_ignore_ascii_case("W") {
      -lon_deg
    } else {
      lon_deg
    };

    Some(util::Coord {
      x: util::to_dec_deg(lon_deg, lon_min, lon_sec),
      y: util::to_dec_deg(lat_deg, lat_min, lat_sec),
    })
  }
}
