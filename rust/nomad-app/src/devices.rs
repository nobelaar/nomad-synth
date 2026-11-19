//! Hot switching of the audio device and the MIDI port (`_cycle_device`,
//! `_cycle_midi` of `nomad/gui/app.py`). The engine lives in the `AudioCore`,
//! which a closed output hands back, so instruments, patterns and parameters
//! survive the switch. Opening goes through `AudioOpener`, so tests can use a
//! fake device.

use std::sync::Arc;

use nomad_runtime::AudioCore;
use nomad_runtime::midi::input::MidiInput;
use nomad_runtime::output::AudioOutput;

use crate::ui::NomadApp;

/// An open output that can give its core back.
pub trait OutputDevice {
    fn close(self: Box<Self>) -> AudioCore;
    fn stop(&mut self);
}

impl OutputDevice for AudioOutput {
    fn close(self: Box<Self>) -> AudioCore {
        AudioOutput::close(*self)
    }
    fn stop(&mut self) {
        AudioOutput::stop(self);
    }
}

/// Open `core` on device `index` (`None` = automatic choice); a failure hands it back.
pub type AudioOpener =
    Box<dyn FnMut(AudioCore, Option<usize>) -> Result<Box<dyn OutputDevice>, (String, AudioCore)>>;

/// The real opener, at the engine's sample rate (the engine is not rebuilt).
// The core comes back by value on purpose (rare call: a device change).
#[allow(clippy::result_large_err)]
pub fn real_opener(sample_rate: u32) -> AudioOpener {
    Box::new(move |core, index| {
        AudioOutput::start_or_return(core, index, sample_rate, 512)
            .map(|o| Box::new(o) as Box<dyn OutputDevice>)
    })
}

fn cut(s: String) -> String {
    s.chars().take(80).collect()
}

impl NomadApp {
    /// Move audio to device `index`; if it does not open, back to the previous one.
    pub fn switch_audio(&mut self, index: usize) {
        let core = match (self.output.take(), self.parked_core.take()) {
            (Some(o), _) => o.close(),
            (None, Some(c)) => c,
            (None, None) => {
                self.status = "AUDIO: no hay motor de audio".into();
                return;
            }
        };
        let previous = self.audio_device;
        match (self.opener)(core, Some(index)) {
            Ok(o) => {
                self.output = Some(o);
                self.audio_device = Some(index);
                self.status = String::new();
            }
            Err((e, core)) => match (self.opener)(core, previous) {
                Ok(o) => {
                    self.output = Some(o);
                    // Cut the error, not the suffix: the user must see it went back.
                    const BACK: &str = " (sigue el anterior)";
                    let head: String = format!("AUDIO: {e}")
                        .chars()
                        .take(80 - BACK.chars().count())
                        .collect();
                    self.status = format!("{head}{BACK}");
                }
                Err((_, core)) => {
                    self.parked_core = Some(core);
                    self.status = cut(format!("AUDIO: {e}"));
                }
            },
        }
    }

    /// Release held keys, close the MIDI port and open `port` with the same map.
    pub fn switch_midi(&mut self, port: &str) {
        let open = self.midi.borrow_mut().take();
        let sender = match open {
            Some(m) => m.close(),
            None => self.parked_midi.take(),
        };
        let Some(sender) = sender else {
            self.status = "MIDI: no se puede abrir otro puerto".into();
            return;
        };
        match MidiInput::open_or_return(Some(port), sender, Arc::clone(&self.midi_map)) {
            Ok(m) => {
                *self.midi.borrow_mut() = Some(m);
                self.status = String::new();
            }
            Err((e, sender)) => {
                self.parked_midi = Some(sender);
                self.status = cut(format!("MIDI: {e}"));
            }
        }
    }
}
