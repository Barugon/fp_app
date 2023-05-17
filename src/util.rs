use eframe::{emath, epaint};
use gdal::{raster, spatial_ref};
use std::{cmp, collections, ops, path};

#[macro_export]
macro_rules! debugln {
  ($($arg:tt)*) => (#[cfg(debug_assertions)] println!($($arg)*));
}

pub const FAIL_ERR: &str = "Should always be Ok";
pub const NONE_ERR: &str = "Should always be Some";
pub const APP_NAME: &str = env!("CARGO_PKG_NAME");

pub enum ZipInfo {
  Chart(Vec<path::PathBuf>),
  Aero {
    csv: path::PathBuf,
    shp: path::PathBuf,
  },
}

/// Returns information about what type of FAA data (if any) is contained in a zip file.
pub fn get_zip_info<P: AsRef<path::Path>>(path: P) -> Result<ZipInfo, String> {
  _get_zip_info(path.as_ref())
}

fn _get_zip_info(path: &path::Path) -> Result<ZipInfo, String> {
  let path = if let Some(path) = path.to_str() {
    ["/vsizip/", path].concat()
  } else {
    return Err("Invalid unicode in zip file path".into());
  };

  match gdal::vsi::read_dir(path, true) {
    Ok(files) => {
      let mut csv = path::PathBuf::new();
      let mut shp = path::PathBuf::new();
      let mut tfws = collections::HashSet::new();
      let mut tifs = Vec::new();
      for file in files {
        // Make sure there's no invalid unicode.
        if let Some(text) = file.to_str() {
          if let Some(ext) = file.extension() {
            if ext.eq_ignore_ascii_case("tfw") {
              // Keep track of TFWs.
              if let Some(stem) = file.file_stem() {
                tfws.insert(stem.to_owned());
              }
            } else if ext.eq_ignore_ascii_case("tif") {
              tifs.push(file);
            } else if ext.eq_ignore_ascii_case("zip") {
              let text = text.to_uppercase();
              if text.starts_with("CSV_DATA/") && text.ends_with("_APT_CSV.ZIP") {
                csv = file;
              }
            }
          } else {
            let os_str = file.as_os_str();
            if os_str.eq_ignore_ascii_case("Additional_Data/Shape_Files/") {
              shp = file;
            }
          }
        }
      }

      // Both the shape and appropriate CSV(s) must be present for aero data to be valid.
      if !csv.as_os_str().is_empty() && !shp.as_os_str().is_empty() {
        return Ok(ZipInfo::Aero { csv, shp });
      }

      // Only accept TIFF files that have matching TFW files.
      let mut files = Vec::with_capacity(cmp::min(tifs.len(), tfws.len()));
      for file in tifs {
        if let Some(stem) = file.file_stem() {
          if tfws.contains(stem) {
            if let Some(file) = file.to_str() {
              files.push(file.into());
            }
          }
        }
      }

      if !files.is_empty() {
        return Ok(ZipInfo::Chart(files));
      }
    }
    Err(_) => {
      return Err("Unable to read zip file".into());
    }
  }
  Err("Zip file does not contain usable data".into())
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

impl From<Coord> for emath::Pos2 {
  fn from(coord: Coord) -> Self {
    Self {
      x: coord.x as f32,
      y: coord.y as f32,
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
  pub fn scaled(&self, scale: f32) -> Self {
    Self {
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

  pub fn fitted(&self, size: Size) -> Self {
    let max_x = self.pos.x as u32 + self.size.w;
    let x = if self.pos.x < 0 {
      0
    } else if max_x > size.w {
      let d = (max_x - size.w) as i32;
      cmp::max(0, self.pos.x - d)
    } else {
      self.pos.x
    };

    let w = if max_x > size.w {
      size.w - self.pos.x as u32
    } else {
      self.size.w
    };

    let max_y = self.pos.y as u32 + self.size.h;
    let y = if self.pos.y < 0 {
      0
    } else if max_y > size.h {
      let d = (max_y - size.h) as i32;
      cmp::max(0, self.pos.y - d)
    } else {
      self.pos.y
    };

    let h = if max_y > size.h {
      size.h - self.pos.y as u32
    } else {
      self.size.h
    };

    Self {
      pos: Pos { x, y },
      size: Size { w, h },
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

impl From<Hashable> for f32 {
  fn from(hashable: Hashable) -> Self {
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
#[allow(unused)]
pub fn to_dec_deg(deg: f64, min: f64, sec: f64) -> f64 {
  assert!(min >= 0.0 && sec >= 0.0);
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

/// Nicely format a degrees, minutes, seconds string from latitude in decimal degrees.
pub fn format_lat(dd: f64) -> String {
  assert!((-90.0..=90.0).contains(&dd));
  let (deg, min, sec) = to_deg_min_sec(dd);
  let deg = deg.abs();
  let sn = if deg < 0.0 { 'S' } else { 'N' };
  format!("{deg:03}°{min:02}'{sec:02.2}\"{sn}")
}

/// Nicely format a degrees, minutes, seconds string from longitude in decimal degrees.
pub fn format_lon(dd: f64) -> String {
  assert!((-180.0..=180.0).contains(&dd));
  let (deg, min, sec) = to_deg_min_sec(dd);
  let deg = deg.abs();
  let we = if deg < 0.0 { 'W' } else { 'E' };
  format!("{deg:03}°{min:02}'{sec:02.2}\"{we}")
}

/// Check if a GDAL color will fit into an egui color.
pub fn check_color(color: raster::RgbaEntry) -> bool {
  const COMP_RANGE: ops::Range<i16> = 0..256;
  COMP_RANGE.contains(&color.r)
    && COMP_RANGE.contains(&color.g)
    && COMP_RANGE.contains(&color.b)
    && COMP_RANGE.contains(&color.a)
}

/// Convert a GDAL color to an egui color.
pub fn color(color: &raster::RgbaEntry) -> epaint::Color32 {
  epaint::Color32::from_rgba_unmultiplied(
    color.r as u8,
    color.g as u8,
    color.b as u8,
    color.a as u8,
  )
}

/// Convert a GDAL color to an egui color and invert the luminance.
pub fn inverted_color(color: &raster::RgbaEntry) -> epaint::Color32 {
  let r = color.r as f32;
  let g = color.g as f32;
  let b = color.b as f32;

  // Convert to YCbCr and invert the luminance.
  let y = 255.0 - (r * 0.299 + g * 0.587 + b * 0.114);
  let cb = b * 0.5 - r * 0.168736 - g * 0.331264;
  let cr = r * 0.5 - g * 0.418688 - b * 0.081312;

  // Convert back to RGB.
  let r = (y + 1.402 * cr) as u8;
  let g = (y - 0.344136 * cb - 0.714136 * cr) as u8;
  let b = (y + 1.772 * cb) as u8;

  epaint::Color32::from_rgba_unmultiplied(r, g, b, color.a as u8)
}

mod test {
  #[test]
  fn test_dd_lat_lon_conversion() {
    let dd = super::to_dec_deg(0.0, 59.0, 60.0);
    assert!(dd == 1.0);

    let dd = super::to_dec_deg(34.0, 5.0, 6.9);
    let lat = super::format_lat(dd);
    assert!(lat == "034°05'6.9000\"N");

    let dd = super::to_dec_deg(-117.0, 8.0, 47.0);
    let lon = super::format_lon(dd);
    assert!(lon == "117°08'47.0000\"W");
  }
}
