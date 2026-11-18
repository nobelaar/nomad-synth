//! MIDI input, ported from `nomad/midi/` (the specification): message parsing,
//! the router (held keys, sustain pedal, wheel, panic) and the CC map.

pub mod map;
pub mod parse;
pub mod router;
