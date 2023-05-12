use crate::util;
use eframe::{egui, epaint};
use std::{any, collections, sync::mpsc, thread, time};

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
  pub pos: Option<epaint::Pos2>,
}

impl LongPressTracker {
  pub fn new(ctx: egui::Context) -> Self {
    let (sender, receiver) = mpsc::channel();
    let thread = Some(
      thread::Builder::new()
        .name(any::type_name::<LongPressTracker>().to_owned())
        .spawn(move || loop {
          let mut request = Some(receiver.recv().expect(util::FAIL_ERR));
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
        .expect(util::FAIL_ERR),
    );

    Self {
      sender,
      thread,
      ids: collections::HashSet::new(),
      info: None,
      pos: None,
    }
  }

  pub fn set(&mut self, id: egui::TouchId, phase: egui::TouchPhase, pos: epaint::Pos2) {
    match phase {
      egui::TouchPhase::Start => {
        // Only allow one touch.
        if self.ids.is_empty() {
          let time = time::SystemTime::now();
          let request = Request::Refresh(time);
          self.info = Some(TouchInfo { time, pos });
          self.sender.send(request).expect(util::FAIL_ERR);
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

  pub fn update(&mut self) {
    if let Some(info) = self.info.take() {
      if let Ok(duration) = time::SystemTime::now().duration_since(info.time) {
        if duration >= LONG_PRESS_DUR {
          self.pos = Some(info.pos);
          return;
        }
        self.info = Some(info);
      }
    }
  }

  fn remove_info(&mut self) {
    if let Some(_) = self.info.take() {
      self.sender.send(Request::Cancel).expect(util::FAIL_ERR);
    }
  }
}

impl Drop for LongPressTracker {
  fn drop(&mut self) {
    // Send an exit request.
    self.sender.send(Request::Exit).expect(util::FAIL_ERR);
    if let Some(thread) = self.thread.take() {
      // Wait for the thread to join.
      thread.join().expect(util::FAIL_ERR);
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
