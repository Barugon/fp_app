use std::{time, sync::mpsc, thread, collections};
use eframe::{epaint, egui};
use crate::util;

const LONG_PRESS_SECS: f64 = 1.0;

enum Request {
  Refresh,
  Clear,
  Exit,
}

struct TouchInfo {
  time: time::SystemTime,
  pos: epaint::Pos2,
}

/// Track a long-press touch.
pub struct TouchTracker {
  sender: mpsc::Sender<Request>,
  thread: Option<thread::JoinHandle<()>>,
  ids: collections::HashSet<u64>,
  info: Option<TouchInfo>,
  pub pos: Option<epaint::Pos2>,
}

impl TouchTracker {
  pub fn new(ctx: egui::Context) -> Self {
    let (sender, receiver) = mpsc::channel();
    let thread = Some(
      thread::Builder::new()
        .name("app::TouchTracker thread".to_owned())
        .spawn(move || loop {
          let mut request = receiver.recv().expect(util::FAIL_ERR);
          let mut refresh;
          loop {
            match request {
              Request::Refresh => refresh = true,
              Request::Clear => refresh = false,
              Request::Exit => return,
            }

            // Check for another request.
            match receiver.try_recv() {
              Ok(rqst) => request = rqst,
              Err(_) => break,
            }
          }

          // When Refresh is sent, we wait for the required time and then send a refresh request.
          // This will allow the main thread to wake up and update TrackerTimer.
          if refresh {
            thread::sleep(time::Duration::from_secs_f64(LONG_PRESS_SECS));
            ctx.request_repaint();
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
        if self.ids.is_empty() {
          let time = time::SystemTime::now();
          self.info = Some(TouchInfo { time, pos });
          self.ids.insert(id.0);
          self.sender.send(Request::Refresh).expect(util::FAIL_ERR);
        } else {
          self.info = None;
          self.sender.send(Request::Clear).expect(util::FAIL_ERR);
        }
      }
      _ => {
        self.ids.remove(&id.0);
        self.info = None;
        self.sender.send(Request::Clear).expect(util::FAIL_ERR);
      }
    }
  }

  pub fn update(&mut self) {
    if let Some(info) = self.info.take() {
      if let Ok(dur) = time::SystemTime::now().duration_since(info.time) {
        if dur.as_secs_f64() > LONG_PRESS_SECS {
          self.pos = Some(info.pos);
          return;
        }
        self.info = Some(info);
      }
    }
  }
}

impl Drop for TouchTracker {
  fn drop(&mut self) {
    // Send an exit request.
    self.sender.send(Request::Exit).expect(util::FAIL_ERR);
    if let Some(thread) = self.thread.take() {
      // Wait for the thread to join.
      thread.join().expect(util::FAIL_ERR);
    }
  }
}
