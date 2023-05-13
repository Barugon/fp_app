#![allow(unused)]
use crate::util::{self, FAIL_ERR, NONE_ERR};
use eframe::egui;
use gdal::{spatial_ref, vector};
use std::{any, collections, path, sync::atomic, sync::mpsc, thread};

// NASR = National Airspace System Resources

#[derive(Debug)]
pub struct APTInfo {
  pub id: String,
  pub name: String,
  pub coord: util::Coord,
  pub site_type: SiteType,
  pub site_use: SiteUse,
}

impl APTInfo {
  fn new(feature: vector::Feature) -> Option<Self> {
    let id = feature.get_string("ARPT_ID")?;
    let name = feature.get_string("ARPT_NAME")?;
    let loc = feature.get_coord()?;
    let site_type = feature.get_site_type()?;
    let site_use = feature.get_site_use()?;
    Some(Self {
      id,
      name,
      coord: loc,
      site_type,
      site_use,
    })
  }
}

pub enum APTReply {
  Airport(Option<APTInfo>),
  Nearby(Vec<APTInfo>),
  Search(Vec<APTInfo>),
}

/// APTSource is used for opening and reading [NASR airport data](https://www.faa.gov/air_traffic/flight_info/aeronav/aero_data/NASR_Subscription/) in zipped CSV format.
pub struct APTSource {
  request_count: atomic::AtomicI64,
  sender: mpsc::Sender<APTRequest>,
  receiver: mpsc::Receiver<APTReply>,
  thread: Option<thread::JoinHandle<()>>,
}

impl APTSource {
  /// Open an airport data source.
  /// - `path`: CSV zip file path
  /// - `ctx`: egui context for requesting a repaint
  pub fn open(path: &path::Path, ctx: &egui::Context) -> Result<Self, gdal::errors::GdalError> {
    let ctx = ctx.clone();

    // Concatenate the VSI prefix and the file name.
    let path = ["/vsizip/", path.to_str().expect(NONE_ERR)].concat();
    let path = path::Path::new(path.as_str()).join("APT_BASE.csv");

    // Open the dataset and check for a layer.
    let base = gdal::Dataset::open_ex(path, open_options())?;
    base.layer(0)?;

    // Create the communication channels.
    let (sender, thread_receiver) = mpsc::channel();
    let (thread_sender, receiver) = mpsc::channel();

    // Create the thread.
    let thread = thread::Builder::new()
      .name(any::type_name::<APTSource>().into())
      .spawn(move || {
        use vector::LayerAccess;
        let nad83 = spatial_ref::SpatialRef::from_epsg(4269).expect(FAIL_ERR);
        nad83.set_axis_mapping_strategy(0);

        // Generate the airport ID index.
        let apt_id_idx = {
          let mut layer = base.layer(0).expect(FAIL_ERR);
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

        // We need the chart spatial reference for the nearby search.
        let mut to_chart = None;
        let mut spatial_idx = rstar::RTree::new();

        loop {
          // Wait for the next message.
          let request = thread_receiver.recv().expect(FAIL_ERR);
          match request {
            APTRequest::SpatialRef(proj4) => {
              // A new chart was opened; we need to (re)make our transformation.
              if let Ok(sr) = spatial_ref::SpatialRef::from_proj4(&proj4) {
                if let Ok(trans) = spatial_ref::CoordTransform::new(&nad83, &sr) {
                  // Create a spatial index for nearby search.
                  spatial_idx = {
                    use util::Transform;
                    let mut layer = base.layer(0).expect(FAIL_ERR);
                    let mut tree = rstar::RTree::new();
                    for feature in layer.features() {
                      if let Some(fid) = feature.fid() {
                        let coord = feature.get_coord().and_then(|c| trans.transform(c).ok());
                        if let Some(coord) = coord {
                          tree.insert(AptLocIdx { coord, fid })
                        }
                      }
                    }
                    tree
                  };

                  to_chart = Some(trans);
                }
              }
            }
            APTRequest::Airport(id) => {
              let id = id.to_uppercase();
              let layer = base.layer(0).expect(FAIL_ERR);
              let mut airport = None;

              // Get the airport matching the ID.
              if let Some(fid) = apt_id_idx.get(&id) {
                airport = layer.feature(*fid).and_then(APTInfo::new);
              }

              let reply = APTReply::Airport(airport);
              thread_sender.send(reply).expect(FAIL_ERR);
              ctx.request_repaint();
            }
            APTRequest::Nearby(coord, dist) => {
              let mut airports = Vec::new();
              if let Some(trans) = &to_chart {
                let dsq = dist * dist;
                let mut layer = base.layer(0).expect(FAIL_ERR);
                for item in spatial_idx.locate_within_distance([coord.x, coord.y], dsq) {
                  if let Some(info) = layer.feature(item.fid).and_then(APTInfo::new) {
                    airports.push(info);
                  }
                }
              }

              let reply = APTReply::Nearby(airports);
              thread_sender.send(reply).expect(FAIL_ERR);
              ctx.request_repaint();
            }
            APTRequest::Search(term) => {
              let term = term.to_uppercase();
              let mut layer = base.layer(0).expect(FAIL_ERR);
              let mut airports = Vec::new();

              // Find the airports with names containing the search term.
              for feature in layer.features() {
                if let Some(name) = feature.get_string("ARPT_NAME") {
                  if name.contains(&term) {
                    if let Some(info) = APTInfo::new(feature) {
                      airports.push(info);
                    }
                  }
                }
              }

              let reply = APTReply::Search(airports);
              thread_sender.send(reply).expect(FAIL_ERR);
              ctx.request_repaint();
            }
            APTRequest::Exit => return,
          }
        }
      })
      .expect(FAIL_ERR);

    Ok(Self {
      request_count: atomic::AtomicI64::new(0),
      sender,
      receiver,
      thread: Some(thread),
    })
  }

  /// Set the spatial reference using a PROJ4 string.
  /// - `proj4`: PROJ4 text
  pub fn set_spatial_ref(&self, proj4: String) {
    self
      .sender
      .send(APTRequest::SpatialRef(proj4))
      .expect(FAIL_ERR);
  }

  /// Lookup airport information using it's identifier.
  /// - `id`: airport id
  pub fn airport(&self, id: String) {
    if !id.is_empty() {
      self.sender.send(APTRequest::Airport(id)).expect(FAIL_ERR);
      self.request_count.fetch_add(1, atomic::Ordering::Relaxed);
    }
  }

  /// Request nearby airports.
  /// - `coord`: the chart coordinate (LCC)
  /// - `dist`: the search distance in meters
  pub fn nearby(&self, coord: util::Coord, dist: f64) {
    if dist >= 0.0 {
      let request = APTRequest::Nearby(coord, dist);
      self.sender.send(request).expect(FAIL_ERR);
      self.request_count.fetch_add(1, atomic::Ordering::Relaxed);
    }
  }

  /// Find airport names that match the text.
  /// - `term`: search term
  pub fn search(&self, term: String) {
    if !term.is_empty() {
      self.sender.send(APTRequest::Search(term)).expect(FAIL_ERR);
      self.request_count.fetch_add(1, atomic::Ordering::Relaxed);
    }
  }

  pub fn get_next_reply(&self) -> Option<APTReply> {
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

impl Drop for APTSource {
  fn drop(&mut self) {
    // Send an exit request.
    self.sender.send(APTRequest::Exit).expect(FAIL_ERR);
    if let Some(thread) = self.thread.take() {
      // Wait for the thread to join.
      thread.join().expect(FAIL_ERR);
    }
  }
}

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

enum APTRequest {
  SpatialRef(String),
  Airport(String),
  Nearby(util::Coord, f64),
  Search(String),
  Exit,
}

struct NAVSource {
  base: gdal::Dataset,
}

impl NAVSource {
  fn open(path: &path::Path) -> Result<Self, gdal::errors::GdalError> {
    let file = "NAV_BASE.csv";
    let path = ["/vsizip/", path.to_str().expect(NONE_ERR)].concat();
    let path = path::Path::new(path.as_str()).join(file);
    Ok(Self {
      base: gdal::Dataset::open_ex(path, open_options())?,
    })
  }
}

struct WXLSource {
  base: gdal::Dataset,
}

impl WXLSource {
  fn open(path: &path::Path) -> Result<Self, gdal::errors::GdalError> {
    let file = "WXL_BASE.csv";
    let path = ["/vsizip/", path.to_str().expect(NONE_ERR)].concat();
    let path = path::Path::new(path.as_str()).join(file);
    Ok(Self {
      base: gdal::Dataset::open_ex(path, open_options())?,
    })
  }
}

struct ShapeSource {
  dataset: gdal::Dataset,
}

impl ShapeSource {
  fn open(path: &path::Path) -> Result<Self, gdal::errors::GdalError> {
    let folder = "Shape_Files";
    let path = ["/vsizip/", path.to_str().expect(NONE_ERR)].concat();
    let path = path::Path::new(path.as_str()).join(folder);
    Ok(Self {
      dataset: gdal::Dataset::open_ex(path, open_options())?,
    })
  }
}

fn open_options<'a>() -> gdal::DatasetOptions<'a> {
  gdal::DatasetOptions {
    open_flags: gdal::GdalOpenFlags::GDAL_OF_READONLY
      | gdal::GdalOpenFlags::GDAL_OF_VECTOR
      | gdal::GdalOpenFlags::GDAL_OF_VERBOSE_ERROR,
    ..Default::default()
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
