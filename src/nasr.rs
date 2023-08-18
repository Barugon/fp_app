use crate::util;
use eframe::egui;
use gdal::{
  errors,
  spatial_ref::{self, CoordTransform},
  vector,
};
use std::{any, collections, path, sync, thread};
use sync::{atomic, mpsc};

// NASR = National Airspace System Resources

/// Reader is used for opening and reading [NASR 28 day subscription](https://www.faa.gov/air_traffic/flight_info/aeronav/aero_data/NASR_Subscription/) data.
pub struct Reader {
  count: atomic::AtomicI64,
  status: AptStatusSync,
  ctx: egui::Context,
  tx: mpsc::Sender<Request>,
  rx: mpsc::Receiver<Reply>,
}

impl Reader {
  pub fn new(ctx: &egui::Context) -> Self {
    let ctx = ctx.clone();
    let status = AptStatusSync::new();

    // Create the communication channels.
    let (tx, trx) = mpsc::channel();
    let (ttx, rx) = mpsc::channel();

    // Create the thread.
    thread::Builder::new()
      .name(any::type_name::<AptSource>().into())
      .spawn({
        let mut status = status.clone();
        let ctx = ctx.clone();
        move || {
          let nad83 = spatial_ref::SpatialRef::from_epsg(4269).unwrap();
          nad83.set_axis_mapping_strategy(0);

          // Airport source.
          let mut apt_source: Option<AptSource> = None;

          // Chart transformation.
          let mut to_chart: Option<spatial_ref::CoordTransform> = None;

          let send = {
            let ctx = ctx.clone();
            move |reply: Reply| {
              ttx.send(reply).unwrap();
              ctx.request_repaint();
            }
          };

          // Wait for a message. Exit when the connection is closed.
          while let Ok(request) = trx.recv() {
            match request {
              Request::Open(path, file) => match AptSource::open(&path, &file, &to_chart) {
                Ok(source) => {
                  status.set_is_loaded();
                  status.set_has_id_idx(source.has_id_index());
                  status.set_has_sp_idx(source.has_sp_index());
                  apt_source = Some(source);
                  ctx.request_repaint();
                }
                Err(err) => {
                  send(Reply::Error(err));
                }
              },
              Request::SpatialRef(proj4) => match spatial_ref::SpatialRef::from_proj4(&proj4) {
                Ok(sr) => match spatial_ref::CoordTransform::new(&nad83, &sr) {
                  Ok(trans) => {
                    if let Some(source) = &mut apt_source {
                      // Create the airport spatial index.
                      source.create_spatial_index(&trans);
                      status.set_has_sp_idx(source.has_sp_index());
                      ctx.request_repaint();
                    }

                    to_chart = Some(trans);
                  }
                  Err(err) => {
                    send(Reply::Error(err));
                  }
                },
                Err(err) => {
                  send(Reply::Error(err));
                }
              },
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
            }
          }
        }
      })
      .unwrap();

    Self {
      count: atomic::AtomicI64::new(0),
      status,
      ctx,
      tx,
      rx,
    }
  }

  /// Open a NASR CSV zip file.
  pub fn open(&self, path: path::PathBuf, file: path::PathBuf) {
    self.tx.send(Request::Open(path, file)).unwrap();
  }

  /// True if the airport source is loaded.
  pub fn apt_loaded(&self) -> bool {
    self.status.get() >= AptStatus::Loaded
  }

  /// True if the airport source has a name index.
  #[allow(unused)]
  pub fn apt_name_idx(&self) -> bool {
    self.status.get() >= AptStatus::NameIdIdx
  }

  /// True if the airport source has an ID index.
  pub fn apt_id_idx(&self) -> bool {
    self.status.get() >= AptStatus::NameIdIdx
  }

  /// True if the airport source has a spatial index.
  pub fn apt_spatial_idx(&self) -> bool {
    self.status.get() >= AptStatus::SpatialIdx
  }

  /// Set the chart spatial reference using a PROJ4 string.
  /// > **Note**: this is needed for nearby airport searches.
  /// - `proj4`: PROJ4 text
  pub fn set_spatial_ref(&self, proj4: String) {
    self.tx.send(Request::SpatialRef(proj4)).unwrap();
  }

  /// Lookup airport information using it's identifier.
  /// - `id`: airport id
  pub fn airport(&self, id: String) {
    if !id.is_empty() {
      self.tx.send(Request::Airport(id)).unwrap();
      self.count.fetch_add(1, atomic::Ordering::Relaxed);
      self.ctx.request_repaint();
    }
  }

  /// Request nearby airports.
  /// - `coord`: the chart coordinate (LCC)
  /// - `dist`: the search distance in meters
  pub fn nearby(&self, coord: util::Coord, dist: f64) {
    if dist >= 0.0 {
      self.tx.send(Request::Nearby(coord, dist)).unwrap();
      self.count.fetch_add(1, atomic::Ordering::Relaxed);
      self.ctx.request_repaint();
    }
  }

  /// Find airport names that match the text.
  /// - `term`: search term
  #[allow(unused)]
  pub fn search(&self, term: String) {
    if !term.is_empty() {
      self.tx.send(Request::Search(term)).unwrap();
      self.count.fetch_add(1, atomic::Ordering::Relaxed);
      self.ctx.request_repaint();
    }
  }

  /// The number of pending airport requests.
  pub fn request_count(&self) -> i64 {
    self.count.load(atomic::Ordering::Relaxed)
  }

  /// Get the next reply if available.
  pub fn get_next_reply(&self) -> Option<Reply> {
    let reply = self.rx.try_recv().ok();
    if let Some(reply) = &reply {
      if !matches!(reply, Reply::Error(_)) {
        assert!(self.count.fetch_sub(1, atomic::Ordering::Relaxed) > 0);
      }
      self.ctx.request_repaint();
    }
    reply
  }
}

enum Request {
  Open(path::PathBuf, path::PathBuf),
  SpatialRef(String),
  Airport(String),
  Nearby(util::Coord, f64),
  Search(String),
}

pub enum Reply {
  Airport(Option<AptInfo>),
  Nearby(Vec<AptInfo>),
  Search(Vec<AptInfo>),
  Error(errors::GdalError),
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
  fn open(
    path: &path::Path,
    file: &path::Path,
    trans: &Option<CoordTransform>,
  ) -> Result<Self, errors::GdalError> {
    use gdal::vector::LayerAccess;

    // Concatenate the VSI prefix and the file name.
    let path = ["/vsizip//vsizip/", path.to_str().unwrap()].concat();
    let path = path::Path::new(path.as_str());
    let path = path.join(file).join(AptSource::csv_name());

    // Open the dataset and get the layer.
    let dataset = gdal::Dataset::open_ex(path, Self::open_options())?;
    let mut layer = dataset.layer(0)?;
    let count = layer.feature_count();

    // Create the name and ID indexes.
    let (sp_idx, name_idx, id_idx) = {
      let count = count as usize;
      let mut loc_vec = Vec::with_capacity(count);
      let mut name_vec = Vec::with_capacity(count);
      let mut id_map = collections::HashMap::with_capacity(count);
      for feature in layer.features() {
        if let Some(fid) = feature.fid() {
          if let Some(trans) = trans {
            use util::Transform;

            let coord = feature.get_coord().and_then(|c| trans.transform(c).ok());
            if let Some(coord) = coord {
              loc_vec.push(AptLocIdx { coord, fid })
            }
          }

          if let Some(name) = feature.get_string("ARPT_NAME") {
            name_vec.push((name, fid));
          }

          if let Some(id) = feature.get_string("ARPT_ID") {
            id_map.insert(id, fid);
          }
        }
      }
      (rstar::RTree::bulk_load(loc_vec), name_vec, id_map)
    };

    Ok(Self {
      dataset,
      count,
      name_idx,
      id_idx,
      sp_idx,
    })
  }

  /// Create the spatial index.
  fn create_spatial_index(&mut self, trans: &spatial_ref::CoordTransform) {
    self.sp_idx = {
      use util::Transform;
      use vector::LayerAccess;

      let mut layer = self.layer();
      let mut loc_vec = Vec::with_capacity(self.count as usize);
      for feature in layer.features() {
        if let Some(fid) = feature.fid() {
          let coord = feature.get_coord().and_then(|c| trans.transform(c).ok());
          if let Some(coord) = coord {
            loc_vec.push(AptLocIdx { coord, fid })
          }
        }
      }
      rstar::RTree::bulk_load(loc_vec)
    };
  }

  fn has_id_index(&self) -> bool {
    !self.id_idx.is_empty()
  }

  fn has_sp_index(&self) -> bool {
    self.sp_idx.size() > 0
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

    airports.sort_unstable_by(|a, b| a.name.cmp(&b.name));
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

    airports.sort_unstable_by(|a, b| a.name.cmp(&b.name));
    airports
  }

  fn layer(&self) -> vector::Layer {
    self.dataset.layer(0).unwrap()
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
  pub apt_type: AptType,

  /// Airport usage.
  pub apt_use: AptUse,
}

impl AptInfo {
  fn new(feature: vector::Feature) -> Option<Self> {
    Some(Self {
      fid: feature.fid()?,
      id: feature.get_string("ARPT_ID")?,
      name: feature.get_string("ARPT_NAME")?,
      coord: feature.get_coord()?,
      apt_type: feature.get_apt_type()?,
      apt_use: feature.get_apt_use()?,
    })
  }

  /// Returns a potentially shortened airport name.
  pub fn short_name(&self) -> &str {
    // Attempt to shorten the name by removing extra stuff.
    if let Some(name) = self.name.split(['/', '(']).next() {
      return name.trim_end();
    }
    &self.name
  }

  /// Returns true if this is a non-public heliport.
  pub fn non_public_heliport(&self) -> bool {
    self.apt_type == AptType::Helicopter && self.apt_use != AptUse::Public
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
pub enum AptType {
  Airport,
  Balloon,
  Glider,
  Helicopter,
  Seaplane,
  Ultralight,
}

impl AptType {
  /// Airport type abbreviation.
  pub fn abv(&self) -> &'static str {
    match *self {
      Self::Airport => "A",
      Self::Balloon => "B",
      Self::Glider => "G",
      Self::Helicopter => "H",
      Self::Seaplane => "S",
      Self::Ultralight => "U",
    }
  }
}

trait GetAptType {
  fn get_apt_type(&self) -> Option<AptType>;
}

impl GetAptType for vector::Feature<'_> {
  fn get_apt_type(&self) -> Option<AptType> {
    match self.get_string("SITE_TYPE_CODE")?.as_str() {
      "A" => Some(AptType::Airport),
      "B" => Some(AptType::Balloon),
      "C" => Some(AptType::Seaplane),
      "G" => Some(AptType::Glider),
      "H" => Some(AptType::Helicopter),
      "U" => Some(AptType::Ultralight),
      _ => None,
    }
  }
}

#[derive(Eq, Debug, PartialEq)]
pub enum AptUse {
  AirForce,
  Army,
  CoastGuard,
  Navy,
  Private,
  Public,
}

impl AptUse {
  /// Airport use abbreviation.
  pub fn abv(&self) -> &'static str {
    match *self {
      Self::AirForce => "USAF",
      Self::Army => "ARMY",
      Self::CoastGuard => "USCG",
      Self::Navy => "USN",
      Self::Private => "PVT",
      Self::Public => "PUB",
    }
  }
}

trait GetAptUse {
  fn get_apt_use(&self) -> Option<AptUse>;
}

impl GetAptUse for vector::Feature<'_> {
  fn get_apt_use(&self) -> Option<AptUse> {
    match self.get_string("OWNERSHIP_TYPE_CODE")?.as_str() {
      "CG" => Some(AptUse::CoastGuard),
      "MA" => Some(AptUse::AirForce),
      "MN" => Some(AptUse::Navy),
      "MR" => Some(AptUse::Army),
      "PU" | "PR" => Some(if self.get_string("FACILITY_USE_CODE")? == "PR" {
        AptUse::Private
      } else {
        AptUse::Public
      }),
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
