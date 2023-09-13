use crate::util;
use eframe::egui;
use gdal::{errors, spatial_ref, vector};
use std::{any, collections, path, sync, thread};
use sync::{atomic, mpsc};

// NASR = National Airspace System Resources

/// Reader is used for opening and reading [NASR 28 day subscription](https://www.faa.gov/air_traffic/flight_info/aeronav/aero_data/NASR_Subscription/) data.
pub struct Reader {
  count: sync::Arc<atomic::AtomicI64>,
  status: AirportStatusSync,
  ctx: egui::Context,
  tx: mpsc::Sender<Request>,
  rx: mpsc::Receiver<Reply>,
}

impl Reader {
  pub fn new(ctx: &egui::Context) -> Self {
    let ctx = ctx.clone();
    let status = AirportStatusSync::new();
    let count = sync::Arc::new(atomic::AtomicI64::new(0));

    // Create the communication channels.
    let (tx, trx) = mpsc::channel();
    let (ttx, rx) = mpsc::channel();

    // Create the thread.
    thread::Builder::new()
      .name(any::type_name::<AirportSource>().into())
      .spawn({
        let mut status = status.clone();
        let count = count.clone();
        let ctx = ctx.clone();
        move || {
          let nad83 = spatial_ref::SpatialRef::from_epsg(4269).unwrap();
          nad83.set_axis_mapping_strategy(0);

          // Airport source.
          let mut airport_source: Option<AirportSource> = None;

          // Chart transformation.
          let mut to_chart: Option<ToChart> = None;

          let send = {
            let ctx = ctx.clone();
            move |reply: Reply, dec: bool| {
              ttx.send(reply).unwrap();
              ctx.request_repaint();
              if dec {
                assert!(count.fetch_sub(1, atomic::Ordering::Relaxed) > 0);
              }
            }
          };

          // Wait for a message. Exit when the connection is closed.
          while let Ok(request) = trx.recv() {
            match request {
              Request::Open(path, file) => match AirportSource::open(&path, &file, &to_chart) {
                Ok(source) => {
                  status.set_is_loaded();
                  status.set_has_sp_idx(source.has_sp_index());
                  airport_source = Some(source);

                  // Request a repaint.
                  ctx.request_repaint();
                }
                Err(err) => {
                  let err = format!("Unable to open airport data source: {err}");
                  send(Reply::Error(err.into()), false);
                }
              },
              Request::SpatialRef(proj4, bounds) => {
                match spatial_ref::SpatialRef::from_proj4(&proj4) {
                  Ok(sr) => match spatial_ref::CoordTransform::new(&nad83, &sr) {
                    Ok(trans) => {
                      if let Some(source) = &mut airport_source {
                        // Create the airport spatial index.
                        source.create_spatial_index(&trans);
                        status.set_has_sp_idx(source.has_sp_index());

                        // Request a repaint.
                        ctx.request_repaint();
                      }
                      to_chart = Some(ToChart { trans, bounds });
                    }
                    Err(err) => {
                      let err = format!("Unable to create coordinate transformation: {err}");
                      send(Reply::Error(err.into()), false);
                    }
                  },
                  Err(err) => {
                    let err = format!("Unable to create spatial reference: {err}");
                    send(Reply::Error(err.into()), false);
                  }
                }
              }
              Request::Airport(id) => {
                let airport_source = airport_source.as_ref().unwrap();
                let to_chart = to_chart.as_ref().unwrap();
                let id = id.trim().to_uppercase();
                let reply = if let Some(info) = airport_source.airport(&id) {
                  // Check if the airport is within the chart bounds.
                  if to_chart.contains(info.coord) {
                    Reply::Airport(info)
                  } else {
                    let err = format!("{}\nis not on this chart", info.desc);
                    Reply::Error(err.into())
                  }
                } else {
                  let err = format!("No airport IDs match\n'{id}'");
                  Reply::Error(err.into())
                };
                send(reply, true);
              }
              Request::Nearby(coord, dist, nph) => {
                let airport_source = airport_source.as_ref().unwrap();
                let to_chart = to_chart.as_ref().unwrap();
                let infos = airport_source.nearby(coord, dist, to_chart, nph);
                send(Reply::Nearby(infos), true);
              }
              Request::Search(term, nph) => {
                let airport_source = airport_source.as_ref().unwrap();
                let to_chart = to_chart.as_ref().unwrap();
                let term = term.trim().to_uppercase();

                // Search for an airport ID first.
                let reply = if let Some(info) = airport_source.airport(&term) {
                  if to_chart.contains(info.coord) {
                    Reply::Airport(info)
                  } else {
                    let err = format!("{}\nis not on this chart", info.desc);
                    Reply::Error(err.into())
                  }
                } else {
                  // Airport ID not found, search the airport names.
                  let infos = airport_source.search(&term, to_chart, nph);
                  if infos.is_empty() {
                    let err = format!("Nothing on this chart matches\n'{term}'");
                    Reply::Error(err.into())
                  } else {
                    Reply::Search(infos)
                  }
                };
                send(reply, true);
              }
            }
          }
        }
      })
      .unwrap();

    Self {
      count,
      status,
      ctx,
      tx,
      rx,
    }
  }

  /// Open a NASR CSV zip file.
  /// - `path`: path to the NASR zip file.
  /// - `csv`: airport CSV path within the zip file.
  pub fn open(&self, path: path::PathBuf, csv: path::PathBuf) {
    self.tx.send(Request::Open(path, csv)).unwrap();
  }

  /// True if the airport source is loaded.
  pub fn airport_loaded(&self) -> bool {
    self.status.get() >= AirportStatus::Loaded
  }

  /// True if the airport source has a spatial index.
  pub fn airport_spatial_idx(&self) -> bool {
    self.status.get() >= AirportStatus::SpatialIdx
  }

  /// Set the chart spatial reference using a PROJ4 string.
  /// > **Note**: this is required for all airport searches.
  /// - `proj4`: PROJ4 text
  /// - `bounds`: Chart bounds in LCC coordinates.
  pub fn set_spatial_ref(&self, proj4: String, bounds: util::Bounds) {
    let request = Request::SpatialRef(proj4, bounds);
    self.tx.send(request).unwrap();
  }

  /// Lookup airport information using it's identifier.
  /// - `id`: airport id
  #[allow(unused)]
  pub fn airport(&self, id: String) {
    if !id.is_empty() {
      self.tx.send(Request::Airport(id)).unwrap();
      self.count.fetch_add(1, atomic::Ordering::Relaxed);
      self.ctx.request_repaint();
    }
  }

  /// Request nearby airports.
  /// - `coord`: chart coordinate (LCC)
  /// - `dist`: search distance in meters
  /// - `nph`: include non-public heliports
  pub fn nearby(&self, coord: util::Coord, dist: f64, nph: bool) {
    if dist >= 0.0 {
      self.tx.send(Request::Nearby(coord, dist, nph)).unwrap();
      self.count.fetch_add(1, atomic::Ordering::Relaxed);
      self.ctx.request_repaint();
    }
  }

  /// Find an airport by ID or airport(s) by (partial) name match.
  /// - `term`: search term
  /// - `nph`: include non-public heliports
  pub fn search(&self, term: String, nph: bool) {
    if !term.is_empty() {
      self.tx.send(Request::Search(term, nph)).unwrap();
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
    self.rx.try_recv().ok()
  }
}

enum Request {
  Open(path::PathBuf, path::PathBuf),
  SpatialRef(String, util::Bounds),
  Airport(String),
  Nearby(util::Coord, f64, bool),
  Search(String, bool),
}

pub enum Reply {
  /// Airport info from ID search.
  Airport(AirportInfo),

  /// Airport infos from a nearby search.
  Nearby(Vec<AirportInfo>),

  /// Airport infos matching a name search.
  Search(Vec<AirportInfo>),

  /// Request resulted in an error.
  Error(util::Error),
}

struct ToChart {
  /// Coordinate transformation from NAD83 to LCC.
  trans: spatial_ref::CoordTransform,

  /// Chart bounds in LCC coordinates.
  bounds: util::Bounds,
}

impl ToChart {
  /// Test if a NAD83 coordinate is contained within the chart bounds.
  fn contains(&self, coord: util::Coord) -> bool {
    use util::Transform;
    match self.trans.transform(coord) {
      Ok(coord) => return self.bounds.contains(coord),
      Err(err) => println!("{err}"),
    }
    false
  }
}

#[derive(Eq, Ord, PartialEq, PartialOrd)]
enum AirportStatus {
  None,

  /// Airport database is loaded (ID and name indexes are ready).
  Loaded,

  /// Has a spatial index.
  SpatialIdx,
}

impl From<u8> for AirportStatus {
  fn from(value: u8) -> Self {
    const NONE: u8 = AirportStatus::None as u8;
    const LOADED: u8 = AirportStatus::Loaded as u8;
    const SP_IDX: u8 = AirportStatus::SpatialIdx as u8;
    match value {
      NONE => AirportStatus::None,
      LOADED => AirportStatus::Loaded,
      SP_IDX => AirportStatus::SpatialIdx,
      _ => unreachable!(),
    }
  }
}

#[derive(Clone)]
struct AirportStatusSync {
  status: sync::Arc<atomic::AtomicU8>,
}

impl AirportStatusSync {
  fn new() -> Self {
    let status = atomic::AtomicU8::new(AirportStatus::None as u8);
    Self {
      status: sync::Arc::new(status),
    }
  }

  fn set_is_loaded(&mut self) {
    self.set(AirportStatus::Loaded);
  }

  fn set_has_sp_idx(&mut self, has_idx: bool) {
    if has_idx {
      self.set(AirportStatus::SpatialIdx);
    }
  }

  fn set(&mut self, status: AirportStatus) {
    self.status.store(status as u8, atomic::Ordering::Relaxed);
  }

  fn get(&self) -> AirportStatus {
    self.status.load(atomic::Ordering::Relaxed).into()
  }
}

struct AirportSource {
  dataset: gdal::Dataset,
  count: u64,
  name_vec: Vec<(String, u64)>,
  id_idx: collections::HashMap<String, u64>,
  sp_idx: rstar::RTree<LocIdx>,
}

impl AirportSource {
  fn open_options<'a>() -> gdal::DatasetOptions<'a> {
    gdal::DatasetOptions {
      open_flags: gdal::GdalOpenFlags::GDAL_OF_READONLY | gdal::GdalOpenFlags::GDAL_OF_VECTOR,
      ..Default::default()
    }
  }

  /// Open an airport data source.
  /// - `path`: NASR zip file path
  /// - `file`: airport zip file within NASR zip
  /// - `to_chart`: coordinate transformation and chart bounds
  fn open(
    path: &path::Path,
    file: &path::Path,
    to_chart: &Option<ToChart>,
  ) -> Result<Self, errors::GdalError> {
    use gdal::vector::LayerAccess;

    // Concatenate the VSI prefix and the file name.
    let path = ["/vsizip//vsizip/", path.to_str().unwrap()].concat();
    let path = path::Path::new(path.as_str());
    let path = path.join(file).join(AirportSource::CSV_NAME);

    // Open the dataset and get the layer.
    let dataset = gdal::Dataset::open_ex(path, Self::open_options())?;
    let mut layer = dataset.layer(0)?;
    let count = layer.feature_count();

    // Create the indexes.
    let (name_vec, id_idx, sp_idx) = {
      let count = count as usize;
      let trans = to_chart.as_ref().map(|tc| &tc.trans);
      let mut name_vec = Vec::with_capacity(count);
      let mut id_map = collections::HashMap::with_capacity(count);
      let mut loc_vec = Vec::with_capacity(count);
      for feature in layer.features() {
        if let Some(fid) = feature.fid() {
          // Add the airport name to the name vector.
          if let Some(name) = feature.get_string(AirportInfo::AIRPORT_NAME) {
            name_vec.push((name, fid));
          }

          // Add the airport IDs to the ID index.
          if let Some(id) = feature.get_string(AirportInfo::AIRPORT_ID) {
            id_map.insert(id, fid);
          }

          // Also populate the spatial index if there's a coordinate transformation.
          if let Some(trans) = trans {
            use util::Transform;
            let coord = feature.get_coord().and_then(|c| trans.transform(c).ok());
            if let Some(coord) = coord {
              loc_vec.push(LocIdx { coord, fid })
            }
          }
        }
      }
      (name_vec, id_map, rstar::RTree::bulk_load(loc_vec))
    };

    Ok(Self {
      dataset,
      count,
      name_vec,
      id_idx,
      sp_idx,
    })
  }

  /// Create the spatial index.
  fn create_spatial_index(&mut self, trans: &spatial_ref::CoordTransform) {
    self.sp_idx = {
      use vector::LayerAccess;
      let mut layer = self.layer();
      let mut loc_vec = Vec::with_capacity(self.count as usize);
      for feature in layer.features() {
        if let Some(fid) = feature.fid() {
          use util::Transform;
          let coord = feature
            .get_coord()
            .and_then(|coord| trans.transform(coord).ok());
          if let Some(coord) = coord {
            loc_vec.push(LocIdx { coord, fid })
          }
        }
      }
      rstar::RTree::bulk_load(loc_vec)
    };
  }

  fn has_sp_index(&self) -> bool {
    self.sp_idx.size() > 0
  }

  /// Get `AirportInfo` for the specified airport ID.
  /// - `id`: airport ID
  fn airport(&self, id: &str) -> Option<AirportInfo> {
    use vector::LayerAccess;
    let layer = self.layer();
    if let Some(fid) = self.id_idx.get(id) {
      return layer.feature(*fid).and_then(AirportInfo::new);
    }
    None
  }

  /// Get `AirportInfo` for airports within a search radius.
  /// - `coord`: chart coordinate (LCC)
  /// - `dist`: search distance in meters
  /// - `to_chart`: coordinate transformation and chart bounds
  /// - `nph`: include non-public heliports
  fn nearby(
    &self,
    coord: util::Coord,
    dist: f64,
    to_chart: &ToChart,
    nph: bool,
  ) -> Vec<AirportInfo> {
    use vector::LayerAccess;
    let layer = self.layer();
    let coord = [coord.x, coord.y];
    let dsq = dist * dist;

    // Collect the feature IDs.
    let mut fids = Vec::new();
    for item in self.sp_idx.locate_within_distance(coord, dsq) {
      // Make sure the coordinate (LCC) is within the chart bounds.
      if to_chart.bounds.contains(item.coord) {
        fids.push(item.fid);
      }
    }

    // Sort the feature IDs so that lookups are sequential.
    fids.sort_unstable();

    let mut airports = Vec::with_capacity(fids.len());
    for fid in fids {
      if let Some(info) = layer.feature(fid).and_then(AirportInfo::new) {
        if nph || !info.non_public_heliport() {
          airports.push(info);
        }
      }
    }

    airports.sort_unstable_by(|a, b| a.name.cmp(&b.name));
    airports
  }

  /// Search for airports with names that contain the specified text.
  /// - `term`: search text
  /// - `to_chart`: coordinate transformation and chart bounds
  /// - `nph`: include non-public heliports
  fn search(&self, term: &str, to_chart: &ToChart, nph: bool) -> Vec<AirportInfo> {
    use vector::LayerAccess;
    let layer = self.layer();
    let mut airports = Vec::new();
    for (name, fid) in &self.name_vec {
      if name.contains(term) {
        if let Some(info) = layer.feature(*fid).and_then(AirportInfo::new) {
          // Make sure the coordinate (NAD83) is within the chart bounds.
          if (nph || !info.non_public_heliport()) && to_chart.contains(info.coord) {
            airports.push(info);
          }
        }
      }
    }

    airports.sort_unstable_by(|a, b| a.name.cmp(&b.name));
    airports
  }

  fn layer(&self) -> vector::Layer {
    self.dataset.layer(0).unwrap()
  }

  const CSV_NAME: &str = "APT_BASE.csv";
}

/// Location spatial index item.
struct LocIdx {
  coord: util::Coord,
  fid: u64,
}

impl rstar::RTreeObject for LocIdx {
  type Envelope = rstar::AABB<[f64; 2]>;

  fn envelope(&self) -> Self::Envelope {
    Self::Envelope::from_point([self.coord.x, self.coord.y])
  }
}

impl rstar::PointDistance for LocIdx {
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
pub struct AirportInfo {
  /// Feature record ID.
  pub fid: u64,

  /// Airport ID.
  pub id: String,

  /// Airport name.
  pub name: String,

  /// Coordinate in decimal degrees (NAD 83).
  pub coord: util::Coord,

  /// Airport type.
  pub airport_type: AirportType,

  /// Airport usage.
  pub airport_use: AirportUse,

  /// Short description for UI lists.
  pub desc: String,
}

impl AirportInfo {
  fn new(feature: vector::Feature) -> Option<Self> {
    let mut info = Self {
      fid: feature.fid()?,
      id: feature.get_string(AirportInfo::AIRPORT_ID)?,
      name: feature.get_string(AirportInfo::AIRPORT_NAME)?,
      coord: feature.get_coord()?,
      airport_type: feature.get_airport_type()?,
      airport_use: feature.get_airport_use()?,
      desc: String::new(),
    };

    info.desc = format!(
      "{} ({}), {}, {}",
      info.short_name(),
      info.id,
      info.airport_type.abv(),
      info.airport_use.abv()
    );

    Some(info)
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
    self.airport_type == AirportType::Helicopter && self.airport_use != AirportUse::Public
  }

  const AIRPORT_ID: &str = "ARPT_ID";
  const AIRPORT_NAME: &str = "ARPT_NAME";
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
pub enum AirportType {
  Airport,
  Balloon,
  Glider,
  Helicopter,
  Seaplane,
  Ultralight,
}

impl AirportType {
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

trait GetAirportType {
  fn get_airport_type(&self) -> Option<AirportType>;
}

impl GetAirportType for vector::Feature<'_> {
  fn get_airport_type(&self) -> Option<AirportType> {
    match self.get_string("SITE_TYPE_CODE")?.as_str() {
      "A" => Some(AirportType::Airport),
      "B" => Some(AirportType::Balloon),
      "C" => Some(AirportType::Seaplane),
      "G" => Some(AirportType::Glider),
      "H" => Some(AirportType::Helicopter),
      "U" => Some(AirportType::Ultralight),
      _ => None,
    }
  }
}

#[derive(Eq, Debug, PartialEq)]
pub enum AirportUse {
  AirForce,
  Army,
  CoastGuard,
  Navy,
  Private,
  Public,
}

impl AirportUse {
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

trait GetAirportUse {
  fn get_airport_use(&self) -> Option<AirportUse>;
}

impl GetAirportUse for vector::Feature<'_> {
  fn get_airport_use(&self) -> Option<AirportUse> {
    match self.get_string("OWNERSHIP_TYPE_CODE")?.as_str() {
      "CG" => Some(AirportUse::CoastGuard),
      "MA" => Some(AirportUse::AirForce),
      "MN" => Some(AirportUse::Navy),
      "MR" => Some(AirportUse::Army),
      "PU" | "PR" => Some(if self.get_string("FACILITY_USE_CODE")? == "PR" {
        AirportUse::Private
      } else {
        AirportUse::Public
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
