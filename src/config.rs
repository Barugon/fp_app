use crate::util;
use std::{path, sync};

/// Storage for configuration items, persisted as JSON.
#[derive(Clone)]
pub struct Storage {
  items: sync::Arc<sync::RwLock<inner::Items>>,
  thread: sync::Arc<inner::PersistThread>,
  store_win: bool,
}

impl Storage {
  pub fn new(store_win: bool) -> Option<Self> {
    let path = Storage::path()?;
    let items = sync::Arc::new(sync::RwLock::new(inner::Items::load(path)));
    let thread = sync::Arc::new(inner::PersistThread::new(items.clone()));
    Some(Self {
      items,
      thread,
      store_win,
    })
  }

  pub fn set_win_info(&mut self, win_info: &util::WinInfo) {
    if self.store_win {
      let value = win_info.to_value();
      let mut items = self.items.write().unwrap();
      items.set(Storage::WIN_INFO_KEY, value);
      self.thread.persist();
    }
  }

  pub fn get_win_info(&self) -> util::WinInfo {
    let items = self.items.read().unwrap();
    util::WinInfo::from_value(items.get(Storage::WIN_INFO_KEY))
  }

  pub fn set_night_mode(&mut self, dark: bool) {
    let value = serde_json::Value::Bool(dark);
    let mut items = self.items.write().unwrap();
    items.set(Storage::NIGHT_MODE_KEY, value);
    self.thread.persist();
  }

  pub fn get_night_mode(&self) -> Option<bool> {
    let items = self.items.read().unwrap();
    items.get(Storage::NIGHT_MODE_KEY)?.as_bool()
  }

  #[allow(unused)]
  pub fn set_include_nph(&mut self, dark: bool) {
    let value = serde_json::Value::Bool(dark);
    let mut items = self.items.write().unwrap();
    items.set(Storage::INCLUDE_NPH_KEY, value);
    self.thread.persist();
  }

  pub fn get_include_nph(&self) -> Option<bool> {
    let items = self.items.read().unwrap();
    items.get(Storage::INCLUDE_NPH_KEY)?.as_bool()
  }

  pub fn set_asset_path(&mut self, path: String) {
    let value = serde_json::Value::String(path);
    let mut items = self.items.write().unwrap();
    items.set(Storage::ASSET_PATH_KEY, value);
    self.thread.persist();
  }

  pub fn get_asset_path(&self) -> Option<String> {
    let items = self.items.read().unwrap();
    Some(items.get(Storage::ASSET_PATH_KEY)?.as_str()?.into())
  }

  fn path() -> Option<path::PathBuf> {
    dirs::config_dir().map(|path| path.join(util::APP_NAME).with_extension("json"))
  }

  const WIN_INFO_KEY: &str = "win_info";
  const NIGHT_MODE_KEY: &str = "night_mode";
  const INCLUDE_NPH_KEY: &str = "include_nph";
  const ASSET_PATH_KEY: &str = "asset_path";
}

mod inner {
  use std::{
    fs, io, path,
    sync::{self, atomic, mpsc},
    thread,
  };

  pub struct Items {
    path: path::PathBuf,
    items: serde_json::Value,
    changed: atomic::AtomicBool,
  }

  impl Items {
    pub fn load(path: path::PathBuf) -> Self {
      let items = Self::load_items(&path);
      let changed = atomic::AtomicBool::new(false);
      Self {
        path,
        items,
        changed,
      }
    }

    pub fn get(&self, key: &str) -> Option<&serde_json::Value> {
      self.items.get(key)
    }

    pub fn set(&mut self, key: &str, item: serde_json::Value) {
      if self.items.get(key) == Some(&item) {
        return;
      }
      self.items[key] = item;
      self.changed.store(true, atomic::Ordering::Relaxed);
    }

    #[allow(unused)]
    pub fn remove(&mut self, key: &str) {
      if self.items.as_object_mut().unwrap().remove(key).is_some() {
        self.changed.store(true, atomic::Ordering::Relaxed);
      }
    }

    fn load_items(path: &path::Path) -> serde_json::Value {
      match fs::File::open(path) {
        Ok(file) => {
          let reader = io::BufReader::new(file);
          match serde_json::from_reader(reader) {
            Ok(items) => {
              let items: serde_json::Value = items;
              if items.is_object() {
                return items;
              }
            }
            Err(err) => println!("{path:?}: {err}"),
          }
        }
        Err(err) => println!("{path:?}: {err}"),
      }
      serde_json::json!({})
    }

    fn store_items(&self) {
      if self.changed.swap(false, atomic::Ordering::Relaxed) {
        match fs::File::create(&self.path) {
          Ok(file) => {
            let writer = io::BufWriter::new(file);
            match serde_json::to_writer(writer, &self.items) {
              Ok(()) => (),
              Err(err) => println!("{:?}: {err}", self.path),
            }
          }
          Err(err) => println!("{:?}: {err}", self.path),
        }
      }
    }
  }

  impl Drop for Items {
    fn drop(&mut self) {
      self.store_items();
    }
  }

  pub struct PersistThread {
    join: Option<thread::JoinHandle<()>>,
    tx: Option<mpsc::Sender<()>>,
  }

  impl PersistThread {
    pub fn new(items: sync::Arc<sync::RwLock<Items>>) -> Self {
      let (tx, rx) = mpsc::channel();
      Self {
        join: Some(thread::spawn({
          move || {
            // Wait for a message. Exit when the connection is closed.
            while rx.recv().is_ok() {
              // Persist the items.
              items.read().unwrap().store_items();
            }
          }
        })),
        tx: Some(tx),
      }
    }

    pub fn persist(&self) {
      if let Some(tx) = &self.tx {
        tx.send(()).unwrap();
      }
    }
  }

  impl Drop for PersistThread {
    fn drop(&mut self) {
      // Close the connection by dropping the sender.
      drop(self.tx.take().unwrap());

      // Wait for the thread to exit.
      self.join.take().unwrap().join().unwrap();
    }
  }
}
