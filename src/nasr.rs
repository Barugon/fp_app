use crate::util::{self, FAIL_ERR, NONE_ERR};
use eframe::egui;
use gdal::{spatial_ref, vector};
use std::{
  any, collections, path,
  sync::{self, atomic, mpsc},
  thread,
};

// NASR = National Airspace System Resources

/// Reader is used for opening and reading [NASR 28 day subscription](https://www.faa.gov/air_traffic/flight_info/aeronav/aero_data/NASR_Subscription/) data.
pub struct Reader {
  request_count: atomic::AtomicI64,
  sender: mpsc::Sender<Request>,
  receiver: mpsc::Receiver<Reply>,
  thread: Option<thread::JoinHandle<()>>,
  apt_status: AptStatusSync,
  ctx: egui::Context,
}

impl Reader {
  pub fn new(ctx: &egui::Context) -> Self {
    let mut apt_data_status = AptStatusSync::new();
    let apt_status = apt_data_status.clone();
    let thread_ctx = ctx.clone();

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

        let send_ctx = thread_ctx.clone();
        let send = move |reply: Reply| {
          thread_sender.send(reply).expect(FAIL_ERR);
          send_ctx.request_repaint();
        };

        loop {
          // Wait for the next message.
          let request = thread_receiver.recv().expect(FAIL_ERR);
          match request {
            Request::Open(path, file) => {
              if let Ok(mut source) = AptSource::open(&path, &file) {
                apt_data_status.set_is_loaded();
                apt_data_status.set_has_id_idx(!source.id_idx.is_empty());

                // A new airport source was opened; (re)make the spatial index if we have a to-chart transformation.
                if let Some(trans) = &to_chart {
                  source.create_spatial_index(trans);
                  apt_data_status.set_has_sp_idx(source.sp_idx.size() != 0);
                }

                apt_source = Some(source);
                thread_ctx.request_repaint();
              }
            }
            Request::SpatialRef(proj4) => {
              match spatial_ref::SpatialRef::from_proj4(&proj4) {
                Ok(sr) => match spatial_ref::CoordTransform::new(&nad83, &sr) {
                  Ok(trans) => {
                    if let Some(source) = &mut apt_source {
                      // A new chart was opened; (re)make the airport spatial index.
                      source.create_spatial_index(&trans);
                      apt_data_status.set_has_sp_idx(source.sp_idx.size() != 0);
                      thread_ctx.request_repaint();
                    }

                    to_chart = Some(trans);
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
              let info = apt_source.as_ref().and_then(|source| source.airport(&id));
              send(Reply::Airport(info));
            }
            Request::Nearby(coord, dist) => {
              let airports = apt_source
                .as_ref()
                .map(|source| source.nearby(coord, dist))
                .unwrap_or_default();
              send(Reply::Nearby(airports));
            }
            Request::Search(term) => {
              let airports = apt_source
                .as_ref()
                .map(|source| source.search(&term))
                .unwrap_or_default();
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
      ctx: ctx.clone(),
    }
  }

  /// Open a NASR CSV zip file.
  pub fn open(&self, path: path::PathBuf, file: path::PathBuf) {
    let request = Request::Open(path, file);
    self.sender.send(request).expect(FAIL_ERR);
  }

  /// True if the airport source is loaded.
  pub fn apt_loaded(&self) -> bool {
    self.apt_status.get() >= AptStatus::Loaded
  }

  /// True if the airport source has a name index.
  #[allow(unused)]
  pub fn apt_name_idx(&self) -> bool {
    self.apt_status.get() >= AptStatus::NameIdIdx
  }

  /// True if the airport source has an ID index.
  pub fn apt_id_idx(&self) -> bool {
    self.apt_status.get() >= AptStatus::NameIdIdx
  }

  /// True if the airport source has a spatial index.
  pub fn apt_spatial_idx(&self) -> bool {
    self.apt_status.get() >= AptStatus::SpatialIdx
  }

  /// Set the chart spatial reference using a PROJ4 string.
  /// > **Note**: this is needed for nearby airport searches.
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
      self.ctx.request_repaint();
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
      self.ctx.request_repaint();
    }
  }

  /// Find airport names that match the text.
  /// - `term`: search term
  #[allow(unused)]
  pub fn search(&self, term: String) {
    if !term.is_empty() {
      self.sender.send(Request::Search(term)).expect(FAIL_ERR);
      self.request_count.fetch_add(1, atomic::Ordering::Relaxed);
      self.ctx.request_repaint();
    }
  }

  /// The number of pending airport requests.
  pub fn request_count(&self) -> i64 {
    self.request_count.load(atomic::Ordering::Relaxed)
  }

  /// Get the next reply if available.
  pub fn get_next_reply(&self) -> Option<Reply> {
    let reply = self.receiver.try_recv().ok();
    if reply.is_some() {
      assert!(self.request_count.fetch_sub(1, atomic::Ordering::Relaxed) > 0);
      self.ctx.request_repaint();
    }
    reply
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
  Open(path::PathBuf, path::PathBuf),
  SpatialRef(String),
  Airport(String),
  Nearby(util::Coord, f64),
  Search(String),
  Exit,
}

pub enum Reply {
  Airport(Option<AptInfo>),
  Nearby(Vec<AptInfo>),
  Search(Vec<AptInfo>),
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum AptStatus {
  None,

  /// Is loaded.
  Loaded,

  /// Has name and ID indexes.
  NameIdIdx,

  /// Has a spatial index.
  SpatialIdx,
}

impl From<u8> for AptStatus {
  fn from(value: u8) -> Self {
    const NONE: u8 = AptStatus::None as u8;
    const LOADED: u8 = AptStatus::Loaded as u8;
    const ID_IDX: u8 = AptStatus::NameIdIdx as u8;
    const SP_IDX: u8 = AptStatus::SpatialIdx as u8;
    match value {
      NONE => AptStatus::None,
      LOADED => AptStatus::Loaded,
      ID_IDX => AptStatus::NameIdIdx,
      SP_IDX => AptStatus::SpatialIdx,
      _ => unreachable!(),
    }
  }
}

#[derive(Clone)]
struct AptStatusSync {
  status: sync::Arc<atomic::AtomicU8>,
}

impl AptStatusSync {
  fn new() -> Self {
    let status = atomic::AtomicU8::new(AptStatus::None as u8);
    Self {
      status: sync::Arc::new(status),
    }
  }

  fn set_is_loaded(&mut self) {
    self.set(AptStatus::Loaded);
  }

  fn set_has_id_idx(&mut self, has_idx: bool) {
    if has_idx {
      self.set(AptStatus::NameIdIdx);
    }
  }

  fn set_has_sp_idx(&mut self, has_idx: bool) {
    if has_idx {
      self.set(AptStatus::SpatialIdx);
    }
  }

  fn set(&mut self, status: AptStatus) {
    self.status.store(status as u8, atomic::Ordering::Relaxed);
  }

  fn get(&self) -> AptStatus {
    self.status.load(atomic::Ordering::Relaxed).into()
  }
}

struct AptSource {
  dataset: gdal::Dataset,
  count: u64,
  name_idx: Vec<(String, u64)>,
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
  fn open(path: &path::Path, file: &path::Path) -> Result<Self, gdal::errors::GdalError> {
    use gdal::vector::LayerAccess;

    // Concatenate the VSI prefix and the file name.
    let path = ["/vsizip//vsizip/", path.to_str().expect(NONE_ERR)].concat();
    let path = path::Path::new(path.as_str());
    let path = path.join(file).join(AptSource::csv_name());

    // Open the dataset and get the layer.
    let dataset = gdal::Dataset::open_ex(path, Self::open_options())?;
    let mut layer = dataset.layer(0)?;
    let count = layer.feature_count();

    // Create the name and ID indexes.
    let (name_idx, id_idx) = {
      let mut vec = Vec::with_capacity(count as usize);
      let mut map = collections::HashMap::with_capacity(count as usize);
      for feature in layer.features() {
        if let Some(fid) = feature.fid() {
          if let Some(name) = feature.get_string("ARPT_NAME") {
            vec.push((name, fid));
          }

          if let Some(id) = feature.get_string("ARPT_ID") {
            map.insert(id, fid);
          }
        }
      }
      (vec, map)
    };

    Ok(Self {
      dataset,
      count,
      name_idx,
      id_idx,
      sp_idx: rstar::RTree::new(),
    })
  }

  /// Create the spatial index.
  fn create_spatial_index(&mut self, trans: &spatial_ref::CoordTransform) {
    self.sp_idx = {
      use util::Transform;
      use vector::LayerAccess;

      let mut layer = self.layer();
      let mut vec = Vec::with_capacity(self.count as usize);

      for feature in layer.features() {
        if let Some(fid) = feature.fid() {
          let coord = feature.get_coord().and_then(|c| trans.transform(c).ok());
          if let Some(coord) = coord {
            vec.push(AptLocIdx { coord, fid })
          }
        }
      }
      rstar::RTree::bulk_load(vec)
    };
  }

  /// Get `AptInfo` for the specified airport ID.
  fn airport(&self, id: &str) -> Option<AptInfo> {
    use vector::LayerAccess;

    let layer = self.layer();
    let id = id.to_uppercase();

    if let Some(fid) = self.id_idx.get(&id) {
      return layer.feature(*fid).and_then(AptInfo::new);
    }

    None
  }

  /// Get `AptInfo` for airports within the search area.
  fn nearby(&self, coord: util::Coord, dist: f64) -> Vec<AptInfo> {
    use vector::LayerAccess;

    let layer = self.layer();
    let coord = [coord.x, coord.y];
    let dsq = dist * dist;

    // Collect the feature IDs.
    let mut fids = Vec::new();
    for item in self.sp_idx.locate_within_distance(coord, dsq) {
      fids.push(item.fid);
    }

    // Sort the feature IDs so that lookups are sequential.
    fids.sort_unstable();

    let mut airports = Vec::with_capacity(fids.len());
    for fid in fids {
      if let Some(info) = layer.feature(fid).and_then(AptInfo::new) {
        airports.push(info);
      }
    }

    airports
  }

  /// Search for airports with names that contain the specified text.
  fn search(&self, term: &str) -> Vec<AptInfo> {
    use vector::LayerAccess;

    let layer = self.layer();
    let term = term.to_uppercase();
    let mut airports = Vec::new();

    for (name, fid) in &self.name_idx {
      if name.contains(&term) {
        if let Some(info) = layer.feature(*fid).and_then(AptInfo::new) {
          airports.push(info);
        }
      }
    }

    airports
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

/// Airport information.
#[derive(Debug)]
pub struct AptInfo {
  /// Feature record ID.
  pub fid: u64,

  /// Airport ID.
  pub id: String,

  /// Airport name.
  pub name: String,

  /// Coordinate in decimal degrees (NAD 83).
  pub coord: util::Coord,

  /// Airport type.
  pub site_type: SiteType,

  /// Airport usage.
  pub site_use: SiteUse,
}

impl AptInfo {
  fn new(feature: vector::Feature) -> Option<Self> {
    let fid = feature.fid()?;
    let id = feature.get_string("ARPT_ID")?;
    let name = feature.get_string("ARPT_NAME")?;
    let coord = feature.get_coord()?;
    let site_type = feature.get_site_type()?;
    let site_use = feature.get_site_use()?;
    Some(Self {
      fid,
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
    Some(util::Coord {
      x: self.get_f64("LONG_DECIMAL")?,
      y: self.get_f64("LAT_DECIMAL")?,
    })
  }
}
