#![allow(unused)]

use crate::util;
use gdal::{spatial_ref, vector};
use std::{collections, path, sync::atomic, sync::mpsc, thread};

// NASR = National Airspace System Resources

pub struct APTSource {
  request_count: atomic::AtomicI64,
  sender: mpsc::Sender<APTRequest>,
  receiver: mpsc::Receiver<APTReply>,
  thread: Option<thread::JoinHandle<()>>,
}

impl APTSource {
  pub fn open<F>(path: &path::Path, repaint: F) -> Result<Self, gdal::errors::GdalError>
  where
    F: Fn() + Send + 'static,
  {
    let file = "APT_BASE.csv";
    let path = ["/vsizip/", path.to_str().unwrap()].concat();
    let path = path::Path::new(path.as_str()).join(file);
    let base = gdal::Dataset::open(path)?;
    let (sender, thread_receiver) = mpsc::channel();
    let (thread_sender, receiver) = mpsc::channel();
    Ok(Self {
      request_count: atomic::AtomicI64::new(0),
      sender,
      receiver,
      thread: Some(
        thread::Builder::new()
          .name("APTSource Thread".into())
          .spawn(move || {
            use vector::LayerAccess;
            let nad83 = spatial_ref::SpatialRef::from_epsg(4269).unwrap();
            nad83.set_axis_mapping_strategy(0);

            let mut indexes = None;
            let apt_id_idx = {
              let mut layer = base.layer(0).unwrap();
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

            loop {
              // Wait for the next message.
              let request = thread_receiver.recv().unwrap();
              match request {
                APTRequest::SpatialRef(proj4) => {
                  // A new chart was opened; we need to (re)make our spatial index.
                  if let Ok(sr) = spatial_ref::SpatialRef::from_proj4(&proj4) {
                    if let Ok(trans) = spatial_ref::CoordTransform::new(&nad83, &sr) {
                      let mut layer = base.layer(0).unwrap();
                      let mut index = rstar::RTree::new();
                      for feature in layer.features() {
                        if let Some(fid) = feature.fid() {
                          // Get the location.
                          if let Some(loc) = feature.get_coord() {
                            // Project to LCC.
                            let mut x = [loc.x];
                            let mut y = [loc.y];
                            if trans.transform_coords(&mut x, &mut y, &mut []).is_ok() {
                              // Add it to the spatial index.
                              index.insert(IndexRec {
                                fid,
                                x: x[0],
                                y: y[0],
                              });
                            }
                          }
                        }
                      }
                      indexes = Some(index);
                    }
                  }
                }
                APTRequest::Airport(val) => {
                  let val = val.to_uppercase();
                  let layer = base.layer(0).unwrap();
                  let mut airports = Vec::new();

                  // Get the feature matching the airport ID.
                  if let Some(fid) = apt_id_idx.get(&val) {
                    if let Some(feature) = layer.feature(*fid) {
                      if let Some(info) = APTInfo::new(&feature) {
                        airports.push(info);
                      }
                    }
                  }

                  thread_sender.send(APTReply::Airport(airports)).unwrap();
                  repaint();
                }
                APTRequest::Nearby(coord, dist) => {
                  let dist = dist * dist;
                  let mut airports = Vec::new();

                  if let Some(index) = &indexes {
                    let layer = base.layer(0).unwrap();
                    for rec in index.locate_within_distance([coord.x, coord.y], dist) {
                      if let Some(feature) = layer.feature(rec.fid) {
                        if let Some(info) = APTInfo::new(&feature) {
                          airports.push(info);
                        }
                      }
                    }
                  }

                  thread_sender.send(APTReply::Airport(airports)).unwrap();
                  repaint();
                }
                APTRequest::Search(term) => {
                  let term = term.to_uppercase();
                  let mut layer = base.layer(0).unwrap();
                  let mut airports = Vec::new();

                  // Find the features with names matching the search term.
                  for feature in layer.features() {
                    if let Some(name) = feature.get_string("ARPT_NAME") {
                      if name.contains(&term) {
                        if let Some(info) = APTInfo::new(&feature) {
                          airports.push(info);
                        }
                      }
                    }
                  }

                  thread_sender.send(APTReply::Airport(airports)).unwrap();
                  repaint();
                }
                APTRequest::Exit => return,
              }
            }
          })
          .unwrap(),
      ),
    })
  }

  /// Set the spatial reference using a PROJ4 string.
  /// - `proj4`: PROJ4 text
  pub fn set_spatial_ref(&self, proj4: String) {
    self.sender.send(APTRequest::SpatialRef(proj4)).unwrap();
  }

  /// Lookup airport information using it's identifier.
  /// - `id`: airport id
  pub fn airport(&self, id: String) {
    self.sender.send(APTRequest::Airport(id)).unwrap();
    self.request_count.fetch_add(1, atomic::Ordering::Relaxed);
  }

  /// Request nearby airports.
  /// - `coord`: the chart coordinate (LCC)
  /// - `dist`: the search distance in meters
  pub fn nearby(&self, coord: util::Coord, dist: f64) {
    self.sender.send(APTRequest::Nearby(coord, dist)).unwrap();
    self.request_count.fetch_add(1, atomic::Ordering::Relaxed);
  }

  /// Find airports that match the text (id or name).
  /// - `term`: search term
  pub fn search(&self, term: String) {
    self.sender.send(APTRequest::Search(term)).unwrap();
    self.request_count.fetch_add(1, atomic::Ordering::Relaxed);
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
    self.sender.send(APTRequest::Exit).unwrap();
    if let Some(thread) = self.thread.take() {
      // Wait for the thread to join.
      thread.join().unwrap();
    }
  }
}

enum APTRequest {
  SpatialRef(String),
  Airport(String),
  Nearby(util::Coord, f64),
  Search(String),
  Exit,
}

#[derive(Debug)]
pub struct APTInfo {
  pub id: String,
  pub name: String,
  pub coord: util::Coord,
  pub site_type: SiteType,
  pub site_use: SiteUse,
}

#[derive(Debug)]
pub enum APTReply {
  GdalError(gdal::errors::GdalError),
  Airport(Vec<APTInfo>),
}

impl APTInfo {
  fn new(feature: &vector::Feature) -> Option<Self> {
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

struct WXLSource {
  base: gdal::Dataset,
}

impl WXLSource {
  fn open(path: &path::Path) -> Result<Self, gdal::errors::GdalError> {
    let file = "WXL_BASE.csv";
    let path = ["/vsizip/", path.to_str().unwrap()].concat();
    let path = path::Path::new(path.as_str()).join(file);
    Ok(Self {
      base: gdal::Dataset::open(path)?,
    })
  }
}

struct NAVSource {
  base: gdal::Dataset,
}

impl NAVSource {
  fn open(path: &path::Path) -> Result<Self, gdal::errors::GdalError> {
    let file = "NAV_BASE.csv";
    let path = ["/vsizip/", path.to_str().unwrap()].concat();
    let path = path::Path::new(path.as_str()).join(file);
    Ok(Self {
      base: gdal::Dataset::open(path)?,
    })
  }
}

struct ShapeSource {
  dataset: gdal::Dataset,
}

impl ShapeSource {
  fn open(path: &path::Path) -> Result<Self, gdal::errors::GdalError> {
    let folder = "Shape_Files";
    let path = ["/vsizip/", path.to_str().unwrap()].concat();
    let path = path::Path::new(path.as_str()).join(folder);
    Ok(Self {
      dataset: gdal::Dataset::open(path)?,
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
        println!("{}", err);
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
        println!("{}", err);
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
      "PU" => Some(SiteUse::Public),
      "PR" => Some(if self.get_string("FACILITY_USE_CODE")? == "PR" {
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

struct IndexRec {
  fid: u64,
  x: f64,
  y: f64,
}

impl rstar::RTreeObject for IndexRec {
  type Envelope = rstar::AABB<[f64; 2]>;

  fn envelope(&self) -> Self::Envelope {
    rstar::AABB::from_point([self.x, self.y])
  }
}

impl rstar::PointDistance for IndexRec {
  fn distance_2(
    &self,
    point: &<Self::Envelope as rstar::Envelope>::Point,
  ) -> <<Self::Envelope as rstar::Envelope>::Point as rstar::Point>::Scalar {
    let dx = self.x - point[0];
    let dy = self.y - point[1];
    dx * dx + dy * dy
  }
}
