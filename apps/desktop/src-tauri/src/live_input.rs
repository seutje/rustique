use std::{
    collections::BTreeMap,
    net::UdpSocket,
    sync::{Arc, Mutex},
    thread,
};

use midir::{Ignore, MidiInput, MidiInputConnection};
use rosc::{OscPacket, OscType};
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveInputValue {
    pub source: String,
    pub value: f32,
}

pub struct LiveInputController {
    values: Arc<Mutex<BTreeMap<String, f32>>>,
    midi: Mutex<Option<MidiInputConnection<()>>>,
    osc_thread: Mutex<Option<thread::JoinHandle<()>>>,
}

impl LiveInputController {
    pub fn new() -> Self {
        Self {
            values: Arc::new(Mutex::new(BTreeMap::new())),
            midi: Mutex::new(None),
            osc_thread: Mutex::new(None),
        }
    }

    pub fn midi_ports() -> Result<Vec<String>, String> {
        let input = MidiInput::new("rustique-enumerate")
            .map_err(|error| format!("failed to initialize MIDI: {error}"))?;
        Ok(input
            .ports()
            .iter()
            .map(|port| {
                input
                    .port_name(port)
                    .unwrap_or_else(|_| "unknown MIDI port".into())
            })
            .collect())
    }

    pub fn connect_midi(&self, index: usize) -> Result<(), String> {
        let mut input = MidiInput::new("rustique-midi")
            .map_err(|error| format!("failed to initialize MIDI: {error}"))?;
        input.ignore(Ignore::None);
        let ports = input.ports();
        let port = ports
            .get(index)
            .ok_or_else(|| format!("MIDI port index {index} does not exist"))?;
        let values = Arc::clone(&self.values);
        let connection = input
            .connect(
                port,
                "rustique-input",
                move |_, message, ()| {
                    if message.len() >= 3 && message[0] & 0xf0 == 0xb0 {
                        if let Ok(mut values) = values.lock() {
                            values.insert(
                                format!("midi.cc.{}", message[1]),
                                f32::from(message[2]) / 127.0,
                            );
                        }
                    }
                },
                (),
            )
            .map_err(|error| format!("failed to connect MIDI port: {error}"))?;
        *self
            .midi
            .lock()
            .map_err(|_| "MIDI state lock was poisoned")? = Some(connection);
        Ok(())
    }

    pub fn listen_osc(&self, address: &str) -> Result<(), String> {
        let socket = UdpSocket::bind(address)
            .map_err(|error| format!("failed to bind OSC UDP socket at {address}: {error}"))?;
        let values = Arc::clone(&self.values);
        let handle = thread::Builder::new()
            .name("rustique-osc".into())
            .spawn(move || {
                let mut buffer = vec![0_u8; 65_535].into_boxed_slice();
                while let Ok((length, _)) = socket.recv_from(&mut buffer) {
                    if let Ok((_, packet)) = rosc::decoder::decode_udp(&buffer[..length]) {
                        record_packet(&values, packet);
                    }
                }
            })
            .map_err(|error| format!("failed to start OSC listener: {error}"))?;
        *self
            .osc_thread
            .lock()
            .map_err(|_| "OSC state lock was poisoned")? = Some(handle);
        Ok(())
    }

    pub fn snapshot(&self) -> Result<Vec<LiveInputValue>, String> {
        Ok(self
            .values
            .lock()
            .map_err(|_| "live input state lock was poisoned")?
            .iter()
            .map(|(source, value)| LiveInputValue {
                source: source.clone(),
                value: *value,
            })
            .collect())
    }
}

fn record_packet(values: &Mutex<BTreeMap<String, f32>>, packet: OscPacket) {
    match packet {
        OscPacket::Message(message) => {
            if let Some(value) = message.args.first().and_then(osc_number) {
                if let Ok(mut values) = values.lock() {
                    values.insert(format!("osc{}", message.addr), value);
                }
            }
        }
        OscPacket::Bundle(bundle) => {
            for packet in bundle.content {
                record_packet(values, packet);
            }
        }
    }
}
#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
fn osc_number(value: &OscType) -> Option<f32> {
    match value {
        OscType::Float(value) => Some(*value),
        OscType::Double(value) => Some(*value as f32),
        OscType::Int(value) => Some(*value as f32),
        OscType::Long(value) => Some(*value as f32),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn osc_numeric_messages_are_recorded() {
        let values = Mutex::new(BTreeMap::new());
        record_packet(
            &values,
            OscPacket::Message(rosc::OscMessage {
                addr: "/energy".into(),
                args: vec![OscType::Float(0.75)],
            }),
        );
        assert!((values.lock().unwrap()["osc/energy"] - 0.75).abs() < f32::EPSILON);
    }
}
