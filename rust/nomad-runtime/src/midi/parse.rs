//! Raw MIDI bytes to events: `parse_message(mido.Message.from_bytes(bytes))` of
//! `nomad/midi/midi_input.py`. Channels are omni (reported, not filtered).

#[derive(Clone, Debug, PartialEq)]
pub enum MidiKind {
    NoteOn,
    NoteOff,
    Cc,
    Bend,
}

/// Python's `MidiEvent` without the port name (the input adds it).
#[derive(Clone, Debug, PartialEq)]
pub struct MidiEvent {
    pub kind: MidiKind,
    pub note: u8,
    pub velocity: f64,
    pub cc: u8,
    pub value: u8,
    pub channel: u8,
    pub bend: f64,
}

impl MidiEvent {
    fn new(kind: MidiKind, channel: u8) -> MidiEvent {
        MidiEvent {
            kind,
            note: 0,
            velocity: 0.0,
            cc: 0,
            value: 0,
            channel,
            bend: 0.0,
        }
    }
}

/// `None` for anything unsupported or malformed (system messages, program
/// change, aftertouch, wrong length, data bytes >= 0x80), like mido + Python.
pub fn parse_message(bytes: &[u8]) -> Option<MidiEvent> {
    let (&status, data) = bytes.split_first()?;
    if !(0x80..0xF0).contains(&status) || data.iter().any(|&b| b >= 0x80) {
        return None;
    }
    let channel = status & 0x0F;
    let len = match status >> 4 {
        0xC | 0xD => 1,
        _ => 2,
    };
    if data.len() != len {
        return None; // mido: "wrong number of bytes"
    }
    match status >> 4 {
        0x8 | 0x9 => {
            let (note, vel) = (data[0], data[1]);
            let mut e = MidiEvent::new(MidiKind::NoteOff, channel);
            e.note = note;
            if status >> 4 == 0x9 && vel > 0 {
                e.kind = MidiKind::NoteOn;
                e.velocity = f64::from(vel) / 127.0;
            }
            Some(e)
        }
        0xB => {
            let mut e = MidiEvent::new(MidiKind::Cc, channel);
            e.cc = data[0];
            e.value = data[1];
            Some(e)
        }
        0xE => {
            let pitch = ((i32::from(data[1]) << 7) | i32::from(data[0])) - 8192;
            let mut e = MidiEvent::new(MidiKind::Bend, channel);
            e.bend = (f64::from(pitch) / 8191.0).clamp(-1.0, 1.0);
            Some(e)
        }
        _ => None, // aftertouch, program change, channel pressure
    }
}
