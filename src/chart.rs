use crate::util;
use eframe::epaint;
use gdal::{raster, spatial_ref};
use std::{ops, path, sync::mpsc, thread};

#[derive(Clone, Debug)]
pub enum SourceError {
  GdalError(gdal::errors::GdalError),

  /// The chart pixel size is not valid.
  InvalidPixelSize,

  /// The spatial reference is not LCC, the datum is not NAD83 or the units are not meters.
  InvalidSpatialReference,

  /// Appropriate PaletteIndex raster band was not found.
  RasterNotFound,

  /// A color table was not found.
  ColorTableNotFound,

  /// The color table does not have required number of entries or an entry cannot be converted to RGB.
  InvalidColorTable,
}

struct Source {
  dataset: gdal::Dataset,
  band_idx: isize,
  palette: Vec<gdal::raster::RgbaEntry>,
}

impl Source {
  fn new(path: &path::Path) -> Result<(Self, Transform), SourceError> {
    let options = gdal::DatasetOptions {
      open_flags: gdal::GdalOpenFlags::GDAL_OF_READONLY
        | gdal::GdalOpenFlags::GDAL_OF_RASTER
        | gdal::GdalOpenFlags::GDAL_OF_VERBOSE_ERROR,
      ..Default::default()
    };

    match gdal::Dataset::open_ex(path, options) {
      Ok(dataset) => {
        // Get and check the dataset's spatial reference.
        let spatial_ref = match dataset.spatial_ref() {
          Ok(sr) => {
            match sr.to_proj4() {
              Ok(proj4) => {
                static ITEMS: [&str; 3] = ["+proj=lcc", "+datum=nad83", "+units=m"];
                let proj4 = proj4.to_lowercase();
                for item in ITEMS {
                  if !proj4.contains(item) {
                    return Err(SourceError::InvalidSpatialReference);
                  }
                }
              }
              Err(err) => return Err(SourceError::GdalError(err)),
            }
            sr
          }
          Err(err) => return Err(SourceError::GdalError(err)),
        };

        // This dataset must have a geo-transformation.
        let geo_transform = match dataset.geo_transform() {
          Ok(gt) => gt,
          Err(err) => return Err(SourceError::GdalError(err)),
        };

        let size: util::Size = dataset.raster_size().into();
        if !size.is_valid() {
          return Err(SourceError::InvalidPixelSize);
        }

        let chart_transform = match Transform::new(size, spatial_ref, geo_transform) {
          Ok(trans) => trans,
          Err(err) => return Err(SourceError::GdalError(err)),
        };

        // The raster band index starts at one.
        for index in 1..=dataset.raster_count() {
          let rasterband = dataset.rasterband(index).unwrap();

          // The color interpretation for a FAA chart is PaletteIndex.
          if rasterband.color_interpretation() == raster::ColorInterpretation::PaletteIndex {
            match rasterband.color_table() {
              Some(color_table) => {
                // The color table must have 256 entries.
                let size = color_table.entry_count();
                if size != 256 {
                  return Err(SourceError::InvalidColorTable);
                }

                // Collect the color entries as RGB.
                let mut palette: Vec<gdal::raster::RgbaEntry> = Vec::with_capacity(size);
                for index in 0..size {
                  if let Some(color) = color_table.entry_as_rgb(index) {
                    // All components must be in 0..256 range.
                    if check_color(color) {
                      palette.push(color);
                    } else {
                      return Err(SourceError::InvalidColorTable);
                    }
                  } else {
                    return Err(SourceError::InvalidColorTable);
                  }
                }

                return Ok((
                  Self {
                    dataset,
                    band_idx: index,
                    palette,
                  },
                  chart_transform,
                ));
              }
              None => return Err(SourceError::ColorTableNotFound),
            }
          }
        }
        Err(SourceError::RasterNotFound)
      }
      Err(err) => Err(SourceError::GdalError(err)),
    }
  }

  fn palette(&self) -> &Vec<gdal::raster::RgbaEntry> {
    &self.palette
  }

  fn read(
    &self,
    src_rect: util::Rect,
    dst_size: util::Size,
  ) -> Result<gdal::raster::Buffer<u8>, gdal::errors::GdalError> {
    let raster = self.dataset.rasterband(self.band_idx).unwrap();
    raster.read_as::<u8>(
      src_rect.pos.into(),
      src_rect.size.into(),
      dst_size.into(),
      Some(gdal::raster::ResampleAlg::Average),
    )
  }
}

fn check_color(color: raster::RgbaEntry) -> bool {
  const COMP_RANGE: ops::Range<i16> = 0..256;
  COMP_RANGE.contains(&color.r)
    && COMP_RANGE.contains(&color.g)
    && COMP_RANGE.contains(&color.b)
    && COMP_RANGE.contains(&color.a)
}

/// Transformations between pixel, chart (LCC) and NAD83 coordinates.
pub struct Transform {
  px_size: util::Size,
  spatial_ref: spatial_ref::SpatialRef,
  to_px: gdal::GeoTransform,
  from_px: gdal::GeoTransform,
  to_nad83: spatial_ref::CoordTransform,
  from_nad83: spatial_ref::CoordTransform,
}

#[allow(unused)]
impl Transform {
  fn new(
    px_size: util::Size,
    spatial_ref: spatial_ref::SpatialRef,
    geo_transform: gdal::GeoTransform,
  ) -> Result<Self, gdal::errors::GdalError> {
    // FAA uses NAD83.
    let nad83 = spatial_ref::SpatialRef::from_epsg(4269)?;

    // Respect X/Y order when converting to/from lat/lon coordinates.
    nad83.set_axis_mapping_strategy(0);

    let to_nad83 = spatial_ref::CoordTransform::new(&spatial_ref, &nad83)?;
    let from_nad83 = spatial_ref::CoordTransform::new(&nad83, &spatial_ref)?;
    let to_px = gdal::GeoTransformEx::invert(&geo_transform)?;

    Ok(Transform {
      px_size,
      spatial_ref,
      to_px,
      from_px: geo_transform,
      to_nad83,
      from_nad83,
    })
  }

  pub fn get_proj4(&self) -> String {
    self.spatial_ref.to_proj4().unwrap()
  }

  /// Get the full size of the chart in pixels.
  pub fn px_size(&self) -> util::Size {
    self.px_size
  }

  /// Get the chart spatial reference.
  pub fn spatial_ref(&self) -> &spatial_ref::SpatialRef {
    &self.spatial_ref
  }

  // Get the chart to NAD83 coordinate transformation.
  pub fn chart_to_nad83_transform(&self) -> &spatial_ref::CoordTransform {
    &self.to_nad83
  }

  // Get the NAD83 to chart coordinate transformation.
  pub fn nad83_to_chart_transform(&self) -> &spatial_ref::CoordTransform {
    &self.from_nad83
  }

  /// Convert a pixel distance to chart distance (meters).
  /// - `px`: pixel distance
  pub fn px_to_dist(&self, px: f64) -> f64 {
    // Use the Y scale for distance.
    px * self.from_px[5]
  }

  /// Convert a pixel coordinate to a chart coordinate.
  /// - `coord`: pixel coordinate
  pub fn px_to_chart(&self, coord: util::Coord) -> util::Coord {
    gdal::GeoTransformEx::apply(&self.from_px, coord.x, coord.y).into()
  }

  /// Convert a chart coordinate to a pixel coordinate.
  /// - `coord`: chart coordinate
  pub fn chart_to_px(&self, coord: util::Coord) -> util::Coord {
    gdal::GeoTransformEx::apply(&self.to_px, coord.x, coord.y).into()
  }

  /// Convert a pixel coordinate to a NAD83 coordinate.
  /// - `coord`: pixel coordinate
  pub fn px_to_nad83(&self, coord: util::Coord) -> Result<util::Coord, gdal::errors::GdalError> {
    self.chart_to_nad83(self.px_to_chart(coord))
  }

  /// Convert a NAD83 coordinate to a pixel coordinate.
  /// - `coord`: NAD83 coordinate
  pub fn nad83_to_px(&self, coord: util::Coord) -> Result<util::Coord, gdal::errors::GdalError> {
    Ok(self.chart_to_px(self.nad83_to_chart(coord)?))
  }

  /// Convert a chart coordinate to a NAD83 coordinate.
  /// - `coord`: chart coordinate
  pub fn chart_to_nad83(&self, coord: util::Coord) -> Result<util::Coord, gdal::errors::GdalError> {
    let mut x = [coord.x];
    let mut y = [coord.y];
    self.to_nad83.transform_coords(&mut x, &mut y, &mut [])?;
    Ok(util::Coord { x: x[0], y: y[0] })
  }

  /// Convert a NAD83 coordinate to a chart coordinate.
  /// - `coord`: NAD83 coordinate
  pub fn nad83_to_chart(&self, coord: util::Coord) -> Result<util::Coord, gdal::errors::GdalError> {
    let mut x = [coord.x];
    let mut y = [coord.y];
    self.from_nad83.transform_coords(&mut x, &mut y, &mut [])?;
    Ok(util::Coord { x: x[0], y: y[0] })
  }
}

/// The part of the image we need for display.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ImagePart {
  pub rect: util::Rect,
  pub zoom: util::Hashable,
  pub dark: bool,
}

impl ImagePart {
  pub fn new(rect: util::Rect, zoom: f32, dark: bool) -> Self {
    // A zoom value of zero is not valid.
    assert!(zoom > 0.0);
    let zoom = zoom.into();
    Self { rect, zoom, dark }
  }
}

enum Request {
  SetSource(Source),
  Read(ImagePart),
  Exit,
}

pub enum Reply {
  /// Image result from a read operation.
  Image(ImagePart, epaint::ColorImage),

  /// Read request was canceled in favor of a more recent read
  /// request or a new source was set.
  Canceled(ImagePart),

  /// GDAL error from a read operation.
  GdalError(ImagePart, gdal::errors::GdalError),

  /// Attempt to read without previously setting a chart source.
  ChartSourceNotSet(ImagePart),
}

/// AsyncReader is used to read a chart image in a separate thread.
pub struct AsyncReader {
  sender: mpsc::Sender<Request>,
  receiver: mpsc::Receiver<Reply>,
  thread: Option<thread::JoinHandle<()>>,
}

impl AsyncReader {
  /// Create a new chart reader.
  /// - `name`: the name to give to the thread
  /// - `repaint`: a closure used to request a repaint
  pub fn new<F>(name: &str, repaint: F) -> Self
  where
    F: Fn() + Send + 'static,
  {
    let (sender, thread_receiver) = mpsc::channel();
    let (thread_sender, receiver) = mpsc::channel();

    AsyncReader {
      sender,
      receiver,
      thread: Some(
        thread::Builder::new()
          .name(name.to_owned())
          .spawn(move || {
            let mut light_colors = [epaint::Color32::default(); 256];
            let mut dark_colors = [epaint::Color32::default(); 256];
            let mut source = None;
            let mut read = None;
            loop {
              // Wait until there's a request.
              let mut request = thread_receiver.recv().unwrap();

              // GDAL doesn't have any way to cancel a raster read operation and the
              // requests can pile up during a long read, so we grab all the pending
              // requests in order to get to the most recent.
              loop {
                match request {
                  Request::SetSource(src) => {
                    // Convert the palette to Color32.
                    for (index, color) in src.palette().iter().enumerate() {
                      // Dark (inverted) palette.
                      dark_colors[index] = util::inverted_color(color.r, color.g, color.b, color.a);

                      // Light (normal) palette.
                      light_colors[index] = epaint::Color32::from_rgba_unmultiplied(
                        color.r as u8,
                        color.g as u8,
                        color.b as u8,
                        color.a as u8,
                      );
                    }
                    source = Some(src);

                    // New source, so clear any previous read request.
                    if let Some(canceled) = read.take() {
                      // Reply that the read request was canceled.
                      thread_sender.send(Reply::Canceled(canceled)).unwrap();
                    }
                  }
                  Request::Read(part) => {
                    if let Some(canceled) = read.take() {
                      // Reply that the previous read request was canceled.
                      thread_sender.send(Reply::Canceled(canceled)).unwrap();
                    }
                    read = Some(part);
                  }
                  Request::Exit => return,
                }

                // Check for another request.
                match thread_receiver.try_recv() {
                  Ok(rqst) => request = rqst,
                  Err(_) => break,
                }
              }

              if let Some(part) = read.take() {
                if let Some(source) = &source {
                  let src_rect = part.rect.scaled(part.zoom.inverse());

                  // Read the image data.
                  match source.read(src_rect, part.rect.size) {
                    Ok(gdal_image) => {
                      let (w, h) = gdal_image.size;
                      let mut image = epaint::ColorImage {
                        size: [w, h],
                        pixels: Vec::with_capacity(w * h),
                      };

                      // Choose the palette.
                      let colors = if part.dark {
                        &dark_colors
                      } else {
                        &light_colors
                      };

                      // Convert the image to RGBA.
                      for val in gdal_image.data {
                        image.pixels.push(colors[val as usize]);
                      }

                      // Send it.
                      thread_sender.send(Reply::Image(part, image)).unwrap();

                      // We need to request a repaint here so that the main thread will wake up and get our message.
                      repaint();
                    }
                    Err(err) => thread_sender.send(Reply::GdalError(part, err)).unwrap(),
                  }
                } else {
                  thread_sender.send(Reply::ChartSourceNotSet(part)).unwrap();
                }
              }
            }
          })
          .unwrap(),
      ),
    }
  }

  /// Open a chart source.
  /// - `path`: zip file path
  /// - `file`: geotiff file within the zip
  pub fn open<P: AsRef<path::Path>, F: AsRef<path::Path>>(
    &self,
    path: P,
    file: F,
  ) -> Result<Transform, SourceError> {
    self._open(path.as_ref(), file.as_ref())
  }

  fn _open(&self, path: &path::Path, file: &path::Path) -> Result<Transform, SourceError> {
    // Concatenate the VSI prefix and the file name.
    let path = ["/vsizip/", path.to_str().unwrap()].concat();
    let path = path::Path::new(path.as_str()).join(file);
    let (source, transform) = Source::new(path.as_path())?;
    self.sender.send(Request::SetSource(source)).unwrap();
    Ok(transform)
  }

  /// Kick-off an image read operation.
  /// - `part`: the area to read from the source image.
  pub fn read_image(&self, part: ImagePart) {
    let request = Request::Read(part);
    self.sender.send(request).unwrap();
  }

  /// Get the next reply if available.
  pub fn get_next_reply(&self) -> Option<Reply> {
    if let Ok(reply) = self.receiver.try_recv() {
      Some(reply)
    } else {
      None
    }
  }
}

impl Drop for AsyncReader {
  fn drop(&mut self) {
    // Send an exit request.
    self.sender.send(Request::Exit).unwrap();
    if let Some(thread) = self.thread.take() {
      // Wait for the thread to join.
      thread.join().unwrap();
    }
  }
}
