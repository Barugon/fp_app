use eframe::{emath, epaint};
use gdal::spatial_ref;
use std::{collections, ops, path};

#[macro_export]
macro_rules! debugln {
  ($($arg:tt)*) => (#[cfg(debug_assertions)] println!($($arg)*));
}

#[macro_export]
macro_rules! some {
  ($opt:expr) => {
    match $opt {
      Some(val) => val,
      None => return,
    }
  };
  ($opt:expr, $ret:expr) => {
    match $opt {
      Some(val) => val,
      None => return $ret,
    }
  };
}

#[macro_export]
macro_rules! ok {
  ($res:expr) => {
    match $res {
      Ok(val) => val,
      Err(_) => {
        return;
      }
    }
  };
  ($res:expr, $ret:expr) => {
    match $res {
      Ok(val) => val,
      Err(_err) => {
        debugln!("{}", _err);
        return $ret;
      }
    }
  };
}

pub enum ZipInfo {
  Chart(Vec<path::PathBuf>),
  Airspace(path::PathBuf),
  Aeronautical,
}

pub fn get_zip_info<P: AsRef<path::Path>>(path: P) -> Result<ZipInfo, String> {
  _get_zip_info(path.as_ref())
}

fn _get_zip_info(path: &path::Path) -> Result<ZipInfo, String> {
  let path = if let Some(path) = path.to_str() {
    ["/vsizip/", path].concat()
  } else {
    return Err("Invalid unicode in zip file path".into());
  };

  match gdal::vsi::read_dir(path, false) {
    Ok(files) => {
      let mut tfws = collections::HashSet::new();
      let mut tifs: Option<Vec<path::PathBuf>> = None;
      for file in files {
        if let Some(ext) = file.extension() {
          if ext.eq_ignore_ascii_case("tfw") {
            // Keep track of tfws.
            if let Some(stem) = file.file_stem() {
              tfws.insert(stem.to_owned());
            }
          } else if let Some(tifs) = &mut tifs {
            if ext.eq_ignore_ascii_case("tif") {
              tifs.push(file);
            }
          } else if ext.eq_ignore_ascii_case("tif") {
            tifs = Some(vec![file]);
          } else if ext.eq_ignore_ascii_case("csv") {
            return Ok(ZipInfo::Aeronautical);
          }
        } else if tifs.is_none() {
          if let Some(file_name) = file.to_str() {
            if file_name.eq_ignore_ascii_case("Shape_Files") {
              return Ok(ZipInfo::Airspace(file));
            }
          }
        }
      }

      if let Some(tifs) = tifs {
        // Only accept tif files that have a matching tfw.
        let mut files = Vec::with_capacity(tifs.len());
        for file in tifs {
          if let Some(stem) = file.file_stem() {
            if tfws.contains(stem) {
              files.push(file);
            }
          }
        }
        return Ok(ZipInfo::Chart(files));
      }
    }
    Err(_) => {
      return Err("Unable to read zip file".into());
    }
  }
  Err("Zip file does not contain aeronautical data".into())
}

pub trait Transform {
  fn transform(&self, coord: Coord) -> Result<Coord, gdal::errors::GdalError>;
}

impl Transform for spatial_ref::CoordTransform {
  fn transform(&self, coord: Coord) -> Result<Coord, gdal::errors::GdalError> {
    let mut x = [coord.x];
    let mut y = [coord.y];
    self.transform_coords(&mut x, &mut y, &mut [])?;
    Ok(Coord { x: x[0], y: y[0] })
  }
}

#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct Coord {
  pub x: f64,
  pub y: f64,
}

impl From<(f64, f64)> for Coord {
  fn from(coord: (f64, f64)) -> Self {
    let (x, y) = coord;
    Self { x, y }
  }
}

impl From<emath::Pos2> for Coord {
  fn from(pos: emath::Pos2) -> Self {
    Self {
      x: pos.x as f64,
      y: pos.y as f64,
    }
  }
}

impl From<emath::Vec2> for Coord {
  fn from(pos: emath::Vec2) -> Self {
    Self {
      x: pos.x as f64,
      y: pos.y as f64,
    }
  }
}

impl ops::Mul<f64> for Coord {
  type Output = Coord;

  fn mul(self, scale: f64) -> Self::Output {
    Coord {
      x: self.x * scale,
      y: self.y * scale,
    }
  }
}

#[derive(Copy, Clone, Debug, Default, Hash, PartialEq, Eq)]
pub struct Pos {
  pub x: i32,
  pub y: i32,
}

impl From<emath::Vec2> for Pos {
  fn from(pos: emath::Vec2) -> Self {
    Self {
      x: pos.x as i32,
      y: pos.y as i32,
    }
  }
}

impl From<emath::Pos2> for Pos {
  fn from(pos: emath::Pos2) -> Self {
    Self {
      x: pos.x as i32,
      y: pos.y as i32,
    }
  }
}

impl From<Pos> for emath::Pos2 {
  fn from(pos: Pos) -> Self {
    Self {
      x: pos.x as f32,
      y: pos.y as f32,
    }
  }
}

impl From<Pos> for (isize, isize) {
  fn from(pos: Pos) -> Self {
    (pos.x as isize, pos.y as isize)
  }
}

#[derive(Copy, Clone, Debug, Default, Hash, PartialEq, Eq)]
pub struct Size {
  pub w: u32,
  pub h: u32,
}

impl Size {
  pub fn is_valid(&self) -> bool {
    self.w > 0 && self.h > 0
  }
}

impl From<emath::Vec2> for Size {
  fn from(size: emath::Vec2) -> Self {
    Self {
      w: size.x.round() as u32,
      h: size.y.round() as u32,
    }
  }
}

impl From<(usize, usize)> for Size {
  fn from((x, y): (usize, usize)) -> Self {
    Self {
      w: x as u32,
      h: y as u32,
    }
  }
}

impl From<Size> for emath::Vec2 {
  fn from(size: Size) -> Self {
    Self {
      x: size.w as f32,
      y: size.h as f32,
    }
  }
}

impl From<Size> for (usize, usize) {
  fn from(size: Size) -> Self {
    (size.w as usize, size.h as usize)
  }
}

#[derive(Copy, Clone, Debug, Default, Hash, PartialEq, Eq)]
pub struct Rect {
  pub pos: Pos,
  pub size: Size,
}

impl Rect {
  pub fn scaled(&self, scale: f32) -> Rect {
    Rect {
      pos: Pos {
        x: (self.pos.x as f32 * scale) as i32,
        y: (self.pos.y as f32 * scale) as i32,
      },
      size: Size {
        w: (self.size.w as f32 * scale).round() as u32,
        h: (self.size.h as f32 * scale).round() as u32,
      },
    }
  }
}

impl From<Rect> for emath::Rect {
  fn from(rect: Rect) -> Self {
    Self::from_min_size(rect.pos.into(), rect.size.into())
  }
}

const HASHABLE32_SCALE: f32 = (1 << 22) as f32;

/// Represents a f32 in the 0..=1 range as a hashable value.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Hashable(u32);

impl Hashable {
  pub fn inverse(&self) -> f32 {
    assert!(self.0 > 0);
    HASHABLE32_SCALE / self.0 as f32
  }
}

impl From<f32> for Hashable {
  fn from(val: f32) -> Self {
    assert!((0.0..=1.0).contains(&val));
    Hashable((val * HASHABLE32_SCALE) as u32)
  }
}

impl From<&f32> for Hashable {
  fn from(val: &f32) -> Self {
    assert!((0.0..=1.0).contains(val));
    Hashable((*val * HASHABLE32_SCALE) as u32)
  }
}

impl From<Hashable> for f32 {
  fn from(hashable: Hashable) -> Self {
    hashable.0 as f32 / HASHABLE32_SCALE
  }
}

impl From<&Hashable> for f32 {
  fn from(hashable: &Hashable) -> Self {
    hashable.0 as f32 / HASHABLE32_SCALE
  }
}

pub fn scale_rect(rect: emath::Rect, scale: f32) -> emath::Rect {
  emath::Rect {
    min: emath::Pos2 {
      x: rect.min.x * scale,
      y: rect.min.y * scale,
    },
    max: emath::Pos2 {
      x: rect.max.x * scale,
      y: rect.max.y * scale,
    },
  }
}

pub fn file_stem<P: AsRef<path::Path>>(path: P) -> Option<String> {
  Some(path.as_ref().file_stem()?.to_str()?.to_owned())
}

/// Convert degrees, minutes, seconds to decimal degrees.
pub fn to_dec_deg(deg: f64, min: f64, sec: f64) -> f64 {
  const DEG_PER_MIN: f64 = 1.0 / 60.0;
  const DEG_PER_SEC: f64 = DEG_PER_MIN / 60.0;
  let dd = deg.abs() + min * DEG_PER_MIN + sec * DEG_PER_SEC;
  if deg < 0.0 {
    -dd
  } else {
    dd
  }
}

/// Convert a decimal degree angle to +/- deg, min, sec.
pub fn to_deg_min_sec(dd: f64) -> (f64, f64, f64) {
  let neg = dd < 0.0;
  let dd = dd.abs();
  let deg = dd.trunc();
  let dm = (dd - deg) * 60.0;
  let min = dm.trunc();
  let sec = (dm - min) * 60.0;
  let deg = if neg { -deg } else { deg };
  (deg, min, sec)
}

#[allow(unused)]
pub fn format_lat(dd: f64) -> String {
  assert!((-90.0..=90.0).contains(&dd));
  let (deg, min, sec) = to_deg_min_sec(dd);
  let sn = if deg < 0.0 { 'S' } else { 'N' };
  format!("{:03}°{:02}'{:02.4}\"{}", deg.abs(), min, sec, sn)
}

#[allow(unused)]
pub fn format_lon(dd: f64) -> String {
  assert!((-180.0..=180.0).contains(&dd));
  let (deg, min, sec) = to_deg_min_sec(dd);
  let we = if deg < 0.0 { 'W' } else { 'E' };
  format!("{:03}°{:02}'{:02.4}\"{}", deg.abs(), min, sec, we)
}

pub fn inverted_color(r: i16, g: i16, b: i16, a: i16) -> epaint::Color32 {
  let r = r as f32;
  let g = g as f32;
  let b = b as f32;

  // Convert to YCbCr and invert the luminance.
  let y = 255.0 - (r * 0.299 + g * 0.587 + b * 0.114);
  let cb = b * 0.5 - r * 0.168736 - g * 0.331264;
  let cr = r * 0.5 - g * 0.418688 - b * 0.081312;

  // Convert back to RGB.
  let r = (y + 1.402 * cr) as u8;
  let g = (y - 0.344136 * cb - 0.714136 * cr) as u8;
  let b = (y + 1.772 * cb) as u8;

  epaint::Color32::from_rgba_unmultiplied(r, g, b, a as u8)
}

mod test {
  #[test]
  fn test_dd_lat_lon_conversion() {
    let dd = super::to_dec_deg(34.0, 5.0, 6.9);
    let lat = super::format_lat(dd);
    assert!(lat == "034°05'6.9000\"N");

    let dd = super::to_dec_deg(-117.0, 8.0, 47.0);
    let lon = super::format_lon(dd);
    assert!(lon == "117°08'47.0000\"W");
  }
}
