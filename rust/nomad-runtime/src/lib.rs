//! Audio runtime around `nomad-core`: the command queue towards the audio
//! thread, the garbage queue back from it, telemetry, and (next) the device
//! output. The audio thread owns the `Engine` inside an [`AudioCore`]; the GUI or
//! console side talks to it through a [`ControlHandle`].

mod command;
mod core;
mod telemetry;

/// Audio-to-UI state mirrors (defined in `nomad-core`, where instances carry them).
pub mod mirror {
    pub use nomad_core::mirror::*;
}

pub use command::{Command, Garbage};
pub use core::{AudioCore, ControlHandle, MidiSender, create};
pub use telemetry::{ScopeRing, Telemetry};
pub mod midi;
pub mod output;
