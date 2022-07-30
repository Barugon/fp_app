#![allow(unused)]

use crate::util;
use gdal::{spatial_ref, vector};
use std::{ffi, path, sync::mpsc, thread};

// NASR = National Airspace System Resources

pub struct APTSource {
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
      sender,
      receiver,
      thread: Some(
        thread::Builder::new()
          .name("APTSource Thread".into())
          .spawn(move || {
            let mut to_lcc = None;
            let nad83 = spatial_ref::SpatialRef::from_epsg(4269).unwrap();
            nad83.set_axis_mapping_strategy(0);

            loop {
              // Wait for the next message.
              let request = thread_receiver.recv().unwrap();
              match request {
                APTRequest::SpatialRef(proj4) => {
                  if let Ok(lcc) = spatial_ref::SpatialRef::from_proj4(&proj4) {
                    if let Ok(trans) = spatial_ref::CoordTransform::new(&nad83, &lcc) {
                      to_lcc = Some(trans);
                    }
                  }
                }
                APTRequest::Airport(id) => {
                  use vector::LayerAccess;
                  let id = id.to_uppercase();
                  let mut airports = Vec::new();

                  // There's actually only one layer.
                  for mut layer in base.layers() {
                    // Find the feature matching the airport ID.
                    for feature in layer.features() {
                      if let Ok(Some(val)) = feature.field_as_string_by_name("ARPT_ID") {
                        if val == id {
                          if let Some(info) = APTInfo::with_id(&feature, val) {
                            airports.push(info);
                          }
                          break;
                        }
                      }
                    }
                  }

                  thread_sender.send(APTReply::Airport(airports)).unwrap();
                  repaint();
                }
                APTRequest::Nearby(coord, dist) => {
                  use vector::LayerAccess;
                  let dist = dist * dist;
                  let mut airports = Vec::new();

                  if let Some(to_lcc) = &to_lcc {
                    // There's actually only one layer.
                    for mut layer in base.layers() {
                      // Find any feature within the search distance.
                      for feature in layer.features() {
                        // Get the location.
                        if let Some(loc) = get_coord(&feature) {
                          // Project to LCC.
                          let mut x = [loc.x];
                          let mut y = [loc.y];
                          if to_lcc.transform_coords(&mut x, &mut y, &mut []).is_ok() {
                            // Check the distance.
                            let dx = coord.x - x[0];
                            let dy = coord.y - y[0];
                            if dx * dx + dy * dy < dist {
                              if let Some(info) = APTInfo::with_loc(&feature, loc) {
                                airports.push(info);
                              }
                            }
                          }
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
  pub fn set_spatial_ref(&self, proj4: String) {
    self.sender.send(APTRequest::SpatialRef(proj4)).unwrap();
  }

  /// Lookup airport information using it's identifier.
  pub fn request_airport(&self, id: String) {
    self.sender.send(APTRequest::Airport(id)).unwrap();
  }

  /// Request nearby airports.
  /// - `coord`: the chart coordinate (LCC)
  /// - `dist`: the search distance in meters
  pub fn request_nearby(&self, coord: util::Coord, dist: f64) {
    self.sender.send(APTRequest::Nearby(coord, dist)).unwrap();
  }

  pub fn get_next_reply(&self) -> Option<APTReply> {
    self.receiver.try_get_next_msg()
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
  Exit,
}

#[derive(Debug)]
pub struct APTInfo {
  id: String,
  name: String,
  loc: util::Coord,
  site_type: SiteType,
}

#[derive(Debug)]
pub enum APTReply {
  GdalError(gdal::errors::GdalError),
  Airport(Vec<APTInfo>),
}

impl APTInfo {
  fn with_id(feature: &vector::Feature, id: String) -> Option<Self> {
    let name = feature.field_as_string_by_name("ARPT_NAME").ok()??;
    let loc = get_coord(feature)?;
    let site_type = get_site_type(feature)?;
    Some(Self {
      id,
      name,
      loc,
      site_type,
    })
  }

  fn with_loc(feature: &vector::Feature, loc: util::Coord) -> Option<Self> {
    let id = feature.field_as_string_by_name("ARPT_ID").ok()??;
    let name = feature.field_as_string_by_name("ARPT_NAME").ok()??;
    let site_type = get_site_type(feature)?;
    Some(Self {
      id,
      name,
      loc,
      site_type,
    })
  }
}

struct WXLSource {
  dataset: gdal::Dataset,
}

impl WXLSource {
  fn open(path: &path::Path) -> Result<Self, gdal::errors::GdalError> {
    let file = "WXL_BASE.csv";
    let path = ["/vsizip/", path.to_str().unwrap()].concat();
    let path = path::Path::new(path.as_str()).join(file);
    Ok(Self {
      dataset: gdal::Dataset::open(path)?,
    })
  }
}

struct NAVSource {
  dataset: gdal::Dataset,
}

impl NAVSource {
  fn open(path: &path::Path) -> Result<Self, gdal::errors::GdalError> {
    let file = "NAV_BASE.csv";
    let path = ["/vsizip/", path.to_str().unwrap()].concat();
    let path = path::Path::new(path.as_str()).join(file);
    Ok(Self {
      dataset: gdal::Dataset::open(path)?,
    })
  }
}

struct ShapeSource {
  dataset: gdal::Dataset,
}

impl ShapeSource {
  fn open(path: &path::Path) -> Result<Self, gdal::errors::GdalError> {
    let path = path.join("Additional_Data/Shape_Files");
    Ok(Self {
      dataset: gdal::Dataset::open(path)?,
    })
  }
}

enum Request {
  Exit,
}

pub enum Reply {
  GdalError(gdal::errors::GdalError),
}

trait TryGetNextMsg<T> {
  fn try_get_next_msg(&self) -> Option<T>;
}

impl<T> TryGetNextMsg<T> for mpsc::Receiver<T> {
  fn try_get_next_msg(&self) -> Option<T> {
    if let Ok(msg) = self.try_recv() {
      Some(msg)
    } else {
      None
    }
  }
}

#[derive(Debug)]
pub enum SiteType {
  Airport,
  Balloon,
  Seaplane,
  Glider,
  Helicopter,
  Ultralight,
}

fn get_site_type(feature: &vector::Feature) -> Option<SiteType> {
  let site_type = feature.field_as_string_by_name("SITE_TYPE_CODE").ok()??;
  match site_type.as_str() {
    "A" => Some(SiteType::Airport),
    "B" => Some(SiteType::Balloon),
    "C" => Some(SiteType::Seaplane),
    "G" => Some(SiteType::Glider),
    "H" => Some(SiteType::Helicopter),
    "U" => Some(SiteType::Ultralight),
    _ => None,
  }
}

fn get_coord(feature: &vector::Feature) -> Option<util::Coord> {
  let lat_deg = feature.field_as_double_by_name("LAT_DEG").ok()??;
  let lat_min = feature.field_as_double_by_name("LAT_MIN").ok()??;
  let lat_sec = feature.field_as_double_by_name("LAT_SEC").ok()??;
  let lat_hemis = feature.field_as_string_by_name("LAT_HEMIS").ok()??;
  let lat_deg = if lat_hemis.eq_ignore_ascii_case("S") {
    -lat_deg
  } else {
    lat_deg
  };

  let lon_deg = feature.field_as_double_by_name("LONG_DEG").ok()??;
  let lon_min = feature.field_as_double_by_name("LONG_MIN").ok()??;
  let lon_sec = feature.field_as_double_by_name("LONG_SEC").ok()??;
  let lon_hemis = feature.field_as_string_by_name("LONG_HEMIS").ok()??;
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
