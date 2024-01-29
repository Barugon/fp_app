use crate::util;
use eframe::{egui, epaint};
use gdal::{raster, spatial_ref};
use std::{any, path, sync::mpsc, thread};

/// RasterReader is used for opening and reading [VFR charts](https://www.faa.gov/air_traffic/flight_info/aeronav/digital_products/vfr/) in zipped GEO-TIFF format.
pub struct RasterReader {
  transform: Transform,
  tx: mpsc::Sender<ImagePart>,
  rx: mpsc::Receiver<RasterReply>,
}

impl RasterReader {
  /// Create a new chart raster reader.
  /// - `path`: chart file path
  /// - `ctx`: egui context for requesting a repaint
  pub fn new<P: AsRef<path::Path>>(path: P, ctx: &egui::Context) -> Result<Self, util::Error> {
    RasterReader::_new(path.as_ref(), ctx.clone())
  }

  fn _new(path: &path::Path, ctx: egui::Context) -> Result<Self, util::Error> {
    // Open the chart source.
    let (source, transform, palette) = RasterSource::open(path)?;

    // Create the communication channels.
    let (tx, trx) = mpsc::channel();
    let (ttx, rx) = mpsc::channel();

    // Create the thread.
    thread::Builder::new()
      .name(any::type_name::<RasterReader>().to_owned())
      .spawn(move || {
        // Convert the color palette.
        let light: Vec<epaint::Color32> = palette.iter().map(util::color).collect();
        let dark: Vec<epaint::Color32> = palette.iter().map(util::inverted_color).collect();
        drop(palette);

        // Wait for a message. Exit when the connection is closed.
        while let Ok(request) = trx.recv() {
          let mut part = request;

          // GDAL doesn't have any way to cancel a raster read operation and the
          // requests can pile up during a long read, so grab all the pending
          // requests in order to get to the most recent.
          while let Ok(request) = trx.try_recv() {
            part = request;
          }

          // Read the image data.
          match source.read(&part) {
            Ok(gdal_image) => {
              let (w, h) = gdal_image.size;
              let mut image = epaint::ColorImage {
                size: [w, h],
                pixels: Vec::with_capacity(w * h),
              };

              // Choose the palette.
              let colors = if part.dark { &dark } else { &light };

              // Convert the image to RGBA.
              for val in gdal_image.data {
                image.pixels.push(colors[val as usize]);
              }

              // Send it.
              ttx.send(RasterReply::Image(part, image)).unwrap();

              // Request a repaint here so that the main thread will wake up and get the message.
              ctx.request_repaint();
            }
            Err(err) => {
              let text = format!("{err}");
              ttx.send(RasterReply::Error(part, text.into())).unwrap();
              ctx.request_repaint();
            }
          }
        }
      })
      .unwrap();

    Ok(Self { transform, tx, rx })
  }

  /// Get the transformation.
  pub fn transform(&self) -> &Transform {
    &self.transform
  }

  /// Kick-off an image read operation.
  /// - `part`: the area to read from the source image.
  pub fn read_image(&self, part: ImagePart) {
    self.tx.send(part).unwrap();
  }

  /// Get the next reply if available.
  pub fn get_next_reply(&self) -> Option<RasterReply> {
    if let Ok(reply) = self.rx.try_recv() {
      Some(reply)
    } else {
      None
    }
  }
}

pub enum RasterReply {
  /// Image result from a read operation.
  Image(ImagePart, epaint::ColorImage),

  /// Error message from a read operation.
  Error(ImagePart, util::Error),
}

/// Transformations between pixel, chart (LCC) and NAD83 coordinates.
pub struct Transform {
  px_size: util::Size,
  spatial_ref: spatial_ref::SpatialRef,
  to_px: gdal::GeoTransform,
  from_px: gdal::GeoTransform,
  to_nad83: spatial_ref::CoordTransform,
  from_nad83: spatial_ref::CoordTransform,
  bounds: util::Bounds,
}

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
    let bounds = util::Bounds {
      min: gdal::GeoTransformEx::apply(&geo_transform, 0.0, px_size.h as f64).into(),
      max: gdal::GeoTransformEx::apply(&geo_transform, px_size.w as f64, 0.0).into(),
    };

    Ok(Transform {
      px_size,
      spatial_ref,
      to_px,
      from_px: geo_transform,
      to_nad83,
      from_nad83,
      bounds,
    })
  }

  /// Get the spatial reference as a proj4 string.
  pub fn get_proj4(&self) -> String {
    self.spatial_ref.to_proj4().unwrap()
  }

  /// Get the full size of the chart in pixels.
  pub fn px_size(&self) -> util::Size {
    self.px_size
  }

  /// Get the bounds as chart (LCC) coordinates.
  pub fn bounds(&self) -> &util::Bounds {
    &self.bounds
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

  /// Convert a pixel coordinate to a NAD83 coordinate.
  /// - `coord`: pixel coordinate
  #[allow(unused)]
  pub fn px_to_nad83(&self, coord: util::Coord) -> Result<util::Coord, gdal::errors::GdalError> {
    self.chart_to_nad83(self.px_to_chart(coord))
  }

  /// Convert a NAD83 coordinate to a pixel coordinate.
  /// - `coord`: NAD83 coordinate
  pub fn nad83_to_px(&self, coord: util::Coord) -> Result<util::Coord, gdal::errors::GdalError> {
    let coord = self.nad83_to_chart(coord);
    coord.map(|coord| self.chart_to_px(coord))
  }
}

/// The part of the image needed for display.
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
    Self {
      rect,
      zoom: zoom.into(),
      dark,
    }
  }
}

/// Chart data source.
struct RasterSource {
  dataset: gdal::Dataset,
  band_idx: isize,
  px_size: util::Size,
}

impl RasterSource {
  fn open_options<'a>() -> gdal::DatasetOptions<'a> {
    gdal::DatasetOptions {
      open_flags: gdal::GdalOpenFlags::GDAL_OF_READONLY | gdal::GdalOpenFlags::GDAL_OF_RASTER,
      ..Default::default()
    }
  }

  /// Open a chart data source.
  /// - `path`: raster file path
  fn open(
    path: &path::Path,
  ) -> Result<(Self, Transform, Vec<gdal::raster::RgbaEntry>), util::Error> {
    match gdal::Dataset::open_ex(path, Self::open_options()) {
      Ok(dataset) => {
        // Get and check the dataset's spatial reference.
        let spatial_ref = match dataset.spatial_ref() {
          Ok(sr) => {
            match sr.to_proj4() {
              Ok(proj4) => {
                let proj4 = proj4.to_lowercase();
                for item in ["+proj=lcc", "+datum=nad83", "+units=m"] {
                  if !proj4.contains(item) {
                    return Err("Unable to open chart: invalid spatial reference".into());
                  }
                }
              }
              Err(err) => return Err(format!("Unable to open chart: {err}").into()),
            }
            sr
          }
          Err(err) => return Err(format!("Unable to open chart: {err}").into()),
        };

        // This dataset must have a geo-transformation.
        let geo_transform = match dataset.geo_transform() {
          Ok(gt) => gt,
          Err(err) => return Err(format!("Unable to open chart: {err}").into()),
        };

        let px_size: util::Size = dataset.raster_size().into();
        if !px_size.is_valid() {
          return Err("Unable to open chart: invalid pixel size".into());
        }

        let chart_transform = match Transform::new(px_size, spatial_ref, geo_transform) {
          Ok(trans) => trans,
          Err(err) => return Err(format!("Unable to open chart: {err}").into()),
        };

        let (band_idx, palette) = || -> Result<(isize, Vec<raster::RgbaEntry>), util::Error> {
          // The raster bands start at index one.
          for index in 1..=dataset.raster_count() {
            let rasterband = dataset.rasterband(index).unwrap();

            // The color interpretation for a FAA chart is PaletteIndex.
            if rasterband.color_interpretation() == raster::ColorInterpretation::PaletteIndex {
              if let Some(color_table) = rasterband.color_table() {
                // The color table must have 256 entries.
                let size = color_table.entry_count();
                if size != 256 {
                  return Err("Unable to open chart: invalid color table".into());
                }

                // Collect the color entries as RGB.
                let mut palette = Vec::with_capacity(size);
                for index in 0..size {
                  if let Some(color) = color_table.entry_as_rgb(index) {
                    // All components must be in 0..256 range.
                    if util::check_color(color) {
                      palette.push(color);
                      continue;
                    }
                  }
                  return Err("Unable to open chart: invalid color table".into());
                }
                return Ok((index, palette));
              }
              return Err("Unable to open chart: color table not found".into());
            }
          }
          Err("Unable to open chart: raster layer not found".into())
        }()?;

        Ok((
          Self {
            dataset,
            band_idx,
            px_size,
          },
          chart_transform,
          palette,
        ))
      }
      Err(err) => Err(format!("Unable to open chart: {err}").into()),
    }
  }

  fn read(&self, part: &ImagePart) -> Result<gdal::raster::Buffer<u8>, gdal::errors::GdalError> {
    // Scale and correct the source rectangle (GDAL does not tolerate
    // read requests outside the original raster size).
    let src_rect = part.rect.scaled(part.zoom.inverse()).fitted(self.px_size);
    let raster = self.dataset.rasterband(self.band_idx).unwrap();
    raster.read_as::<u8>(
      src_rect.pos.into(),
      src_rect.size.into(),
      part.rect.size.into(),
      Some(gdal::raster::ResampleAlg::Average),
    )
  }
}
