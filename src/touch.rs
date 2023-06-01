use crate::util;
use eframe::{egui, epaint};
use std::{any, collections, sync::mpsc, thread, time};
use util::Rely;

const LONG_PRESS_DUR: time::Duration = time::Duration::from_secs(1);

enum Request {
  Refresh(time::SystemTime),
  Cancel,
  Exit,
}

struct TouchInfo {
  time: time::SystemTime,
  pos: epaint::Pos2,
}

pub struct LongPressTracker {
  sender: mpsc::Sender<Request>,
  thread: Option<thread::JoinHandle<()>>,
  ids: collections::HashSet<u64>,
  info: Option<TouchInfo>,
}

impl LongPressTracker {
  pub fn new(ctx: egui::Context) -> Self {
    let (sender, receiver) = mpsc::channel();
    let thread = Some(
      thread::Builder::new()
        .name(any::type_name::<LongPressTracker>().to_owned())
        .spawn(move || loop {
          let mut request = Some(receiver.recv().rely());
          let mut time = None;
          loop {
            if let Some(request) = request.take() {
              match request {
                Request::Refresh(t) => time = Some(t),
                Request::Cancel => time = None,
                Request::Exit => return,
              }
            }

            if check_time(time) {
              ctx.request_repaint();
              time = None;
            }

            // Check for another request.
            request = receiver.try_recv().ok();
            if request.is_none() && time.is_none() {
              break;
            }

            // Sleep for a very short duration so that this tread doesn't peg one of the cores.
            const PAUSE: time::Duration = time::Duration::from_millis(1);
            thread::sleep(PAUSE);
          }
        })
        .rely(),
    );

    Self {
      sender,
      thread,
      ids: collections::HashSet::new(),
      info: None,
    }
  }

  pub fn initiate(&mut self, id: egui::TouchId, phase: egui::TouchPhase, pos: epaint::Pos2) {
    match phase {
      egui::TouchPhase::Start => {
        // Only allow one touch.
        if self.ids.is_empty() {
          let time = time::SystemTime::now();
          let request = Request::Refresh(time);
          self.info = Some(TouchInfo { time, pos });
          self.sender.send(request).rely();
        } else {
          self.remove_info();
        }
        self.ids.insert(id.0);
      }
      egui::TouchPhase::Move => {
        self.remove_info();
      }
      egui::TouchPhase::End | egui::TouchPhase::Cancel => {
        self.ids.remove(&id.0);
        self.remove_info();
      }
    }
  }

  pub fn check(&mut self) -> Option<epaint::Pos2> {
    if let Some(info) = self.info.take() {
      if let Ok(duration) = time::SystemTime::now().duration_since(info.time) {
        if duration >= LONG_PRESS_DUR {
          return Some(info.pos);
        }
        self.info = Some(info);
      }
    }
    None
  }

  fn remove_info(&mut self) {
    if self.info.take().is_some() {
      self.sender.send(Request::Cancel).rely();
    }
  }
}

impl Drop for LongPressTracker {
  fn drop(&mut self) {
    // Send an exit request.
    self.sender.send(Request::Exit).rely();
    if let Some(thread) = self.thread.take() {
      // Wait for the thread to join.
      thread.join().rely();
    }
  }
}

fn check_time(time: Option<time::SystemTime>) -> bool {
  if let Some(time) = time {
    if let Ok(duration) = time::SystemTime::now().duration_since(time) {
      if duration >= LONG_PRESS_DUR {
        return true;
      }
    }
  }
  false
}
