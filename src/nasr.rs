#![allow(unused)]

// NASR = National Airspace System Resources

use gdal::spatial_ref;
use std::{path, sync::mpsc, thread};

// There's no authority code for the FAA's LCC spatial reference.
const LCC_PROJ4: &str = "+proj=lcc +lat_0=34.1666666666667 +lon_0=-118.466666666667 +lat_1=38.6666666666667 +lat_2=33.3333333333333 +x_0=0 +y_0=0 +datum=NAD83 +units=m +no_defs";

struct APTSource {
  dataset: gdal::Dataset,
}

impl APTSource {
  fn open(path: &path::Path) -> Result<Self, gdal::errors::GdalError> {
    let file = "Additional_Data/AIXM/AIXM_5.1/XML-Subscriber-Files/APT_AIXM.zip";
    let path = ["/vsizip/", path.to_str().unwrap()].concat();
    let path = path::Path::new(path.as_str()).join(file);
    Ok(Self {
      dataset: gdal::Dataset::open(path)?,
    })
  }
}

struct AWOSSource {
  dataset: gdal::Dataset,
}

impl AWOSSource {
  fn open(path: &path::Path) -> Result<Self, gdal::errors::GdalError> {
    let file = "Additional_Data/AIXM/AIXM_5.1/XML-Subscriber-Files/AWOS_AIXM.zip";
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
    let file = "Additional_Data/AIXM/AIXM_5.1/XML-Subscriber-Files/NAV_AIXM.zip";
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
  Import(path::PathBuf),
  Cancel,
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

/// AsyncImporter is used to import NASR data via a separate thread.
pub struct AsyncImporter {
  sender: mpsc::Sender<Request>,
  receiver: mpsc::Receiver<Reply>,
  thread: Option<thread::JoinHandle<()>>,
}

// Process any outstanding messages.
// - Does not block.
// - Result is false if a cancel message is received.
// - Returns from the calling function if an exit message is received.
macro_rules! import_messages {
  ($receiver:ident, $imports:ident) => {{
    let mut cancel = false;
    while let Some(request) = $receiver.try_get_next_msg() {
      match request {
        Request::Import(path) => {
          $imports.push(path);
        }
        Request::Cancel => {
          $imports.clear();
          cancel = true;
          break;
        }
        Request::Exit => return,
      }
    }
    !cancel
  }};
}

impl AsyncImporter {
  pub fn new(name: String) -> Result<Self, gdal::errors::GdalError> {
    // Respect X/Y order when converting from lat/lon coordinates.
    let nad83 = spatial_ref::SpatialRef::from_epsg(4269)?;
    nad83.set_axis_mapping_strategy(0);

    let lcc = spatial_ref::SpatialRef::from_proj4(LCC_PROJ4)?;
    let to_lcc = spatial_ref::CoordTransform::new(&nad83, &lcc)?;
    let (sender, thread_receiver) = mpsc::channel();
    let (thread_sender, receiver) = mpsc::channel();

    Ok(AsyncImporter {
      sender,
      receiver,
      thread: Some(
        thread::Builder::new()
          .name(name)
          .spawn(move || {
            let mut imports = Vec::new();
            loop {
              // Wait for the next message.
              let request = thread_receiver.recv().unwrap();
              match request {
                Request::Import(path) => imports.push(path),
                Request::Cancel => imports.clear(),
                Request::Exit => return,
              }

              let paths = imports;
              imports = Vec::new();

              for _path in paths {
                if !import_messages!(thread_receiver, imports) {
                  break;
                }
                // Import data here....
              }
            }
          })
          .unwrap(),
      ),
    })
  }

  pub fn import<P: AsRef<path::Path>>(&self, path: P) {
    self._import(path.as_ref())
  }

  fn _import(&self, path: &path::Path) {
    let path = path::PathBuf::from(["/vsizip/", path.to_str().unwrap()].concat());
    self.sender.send(Request::Import(path)).unwrap();
  }

  fn get_next_reply(&self) -> Option<Reply> {
    self.receiver.try_get_next_msg()
  }
}

impl Drop for AsyncImporter {
  fn drop(&mut self) {
    // Send an exit request.
    self.sender.send(Request::Exit).unwrap();
    if let Some(thread) = self.thread.take() {
      // Wait for the thread to join.
      thread.join().unwrap();
    }
  }
}
