use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event as CrosstermEvent, KeyEvent};

pub enum Event {
    Key(KeyEvent),
    Resize,
    Tick,
}

pub struct EventHandler {
    tick_rate: Duration,
}

impl EventHandler {
    pub fn new(tick_rate_ms: u64) -> Self {
        Self {
            tick_rate: Duration::from_millis(tick_rate_ms),
        }
    }

    pub fn next(&self) -> Result<Event> {
        if event::poll(self.tick_rate)? {
            match event::read()? {
                CrosstermEvent::Key(key) => Ok(Event::Key(key)),
                CrosstermEvent::Resize(..) => Ok(Event::Resize),
                _ => Ok(Event::Tick),
            }
        } else {
            Ok(Event::Tick)
        }
    }
}
