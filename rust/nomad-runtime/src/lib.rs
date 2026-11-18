//! Audio runtime around `nomad-core`: the command queue towards the audio
//! thread, the garbage queue back from it, telemetry, and (next) the device
//! output. The audio thread owns the `Engine` inside an [`AudioCore`]; the GUI or
//! console side talks to it through a [`ControlHandle`].

mod command;
mod core;
mod telemetry;

pub use command::{Command, Garbage};
pub use core::{AudioCore, ControlHandle, create};
pub use telemetry::{ScopeRing, Telemetry};
pub mod output;
