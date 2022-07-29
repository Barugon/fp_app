#![allow(unused)]

use crate::util;
use gdal::{spatial_ref, vector};
use std::{path, sync::mpsc, thread};

// NASR = National Airspace System Resources

// There's no authority code for the FAA's LCC spatial reference.
const LCC_PROJ4: &str = "+proj=lcc +lat_0=34.1666666666667 +lon_0=-118.466666666667 +lat_1=38.6666666666667 +lat_2=33.3333333333333 +x_0=0 +y_0=0 +datum=NAD83 +units=m +no_defs";

pub struct APTSource {
  sender: mpsc::Sender<APTRequest>,
  receiver: mpsc::Receiver<APTReply>,
  thread: Option<thread::JoinHandle<()>>,
}

impl APTSource {
  pub fn open(path: &path::Path) -> Result<Self, gdal::errors::GdalError> {
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
            loop {
              // Wait for the next message.
              let request = thread_receiver.recv().unwrap();
              match request {
                APTRequest::Exit => return,
              }
            }
          })
          .unwrap(),
      ),
    })
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
  Exit,
}

pub enum APTReply {
  GdalError(gdal::errors::GdalError),
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

fn get_field(feature: &vector::Feature, field: &str) -> Option<vector::FieldValue> {
  match feature.field(field) {
    Ok(value) => return value,
    Err(err) => println!("{}", err),
  }
  None
}

fn get_field_as_f64(feature: &vector::Feature, field: &str) -> Option<f64> {
  if let Some(value) = get_field(feature, field) {
    match value {
      vector::FieldValue::IntegerValue(value) => return Some(value as f64),
      vector::FieldValue::Integer64Value(value) => return Some(value as f64),
      vector::FieldValue::StringValue(text) => return Some(text.parse().ok()?),
      vector::FieldValue::RealValue(value) => return Some(value),
      _ => (),
    }
  }
  None
}

fn get_coord(feature: &vector::Feature) -> Option<util::Coord> {
  let lat_deg = get_field_as_f64(feature, "LAT_DEG")?;
  let lat_min = get_field_as_f64(feature, "LAT_MIN")?;
  let lat_sec = get_field_as_f64(feature, "LAT_SEC")?;
  let lat_hemis = get_field(feature, "LAT_HEMIS")?.into_string()?;
  let lat_deg = if lat_hemis.eq_ignore_ascii_case("S") {
    -lat_deg
  } else {
    lat_deg
  };

  let lon_deg = get_field_as_f64(feature, "LON_DEG")?;
  let lon_min = get_field_as_f64(feature, "LON_MIN")?;
  let lon_sec = get_field_as_f64(feature, "LON_SEC")?;
  let lon_hemis = get_field(feature, "LON_HEMIS")?.into_string()?;
  let lon_deg = if lat_hemis.eq_ignore_ascii_case("W") {
    -lon_deg
  } else {
    lon_deg
  };

  Some(util::Coord {
    x: util::to_dec_deg(lon_deg, lon_min, lon_sec),
    y: util::to_dec_deg(lat_deg, lat_min, lat_sec),
  })
}
