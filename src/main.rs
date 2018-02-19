#[allow(dead_code)]

extern crate jack;
extern crate miosc;
extern crate rosc;

use std::io;
use std::net::UdpSocket;
use std::str::FromStr;
use std::sync::mpsc::channel;
use std::f64::consts::PI;

use jack::prelude::*;

trait Dsp {
    fn make_noise(&mut self, buf: &mut [f32], sample_rate: f64);
}

#[derive(Debug)]
struct Oscillator {
    freq: f64,
    phase: f64,
    acc: f64,
    fbuf: f64,
    fbuf2: f64,
}

impl Dsp for Oscillator {
    fn make_noise(&mut self, buf: &mut [f32], sample_rate: f64) {
        let period = sample_rate / self.freq;
        let phase_inc = PI / period;
        let m = 2.0 * (period * 0.5).floor() - 1.0;

        let c = 1.0 / period;
        let leak = 0.995;

        for v in buf.iter_mut() {
            let fraq =
                if self.phase.sin() > std::f64::EPSILON {
                    (m * self.phase).sin()
                    / (m * self.phase.sin())
                }
                else { 1.0 };

            let y = (m / period) * fraq;

            let saw = y + self.acc - c;
            self.acc = saw * leak;

            self.fbuf += 0.5 * (saw - self.fbuf);
            self.fbuf2 += 0.6 * (self.fbuf - self.fbuf2);
            *v = self.fbuf2 as f32;

            self.phase += phase_inc;
            if self.phase >= PI { self.phase -= PI }
        }
    }
}

enum AdsrStatus {
    Idle,
    Pressed(f64),
    Released(f64),
}

struct Adsr {
    attack: f32,
    decay: f32,
    sustain: f32,
    release: f32,
}

fn main() {
    let (client, _status) = Client::new("rust_jack_sine", client_options::NO_START_SERVER).unwrap();
    let mut out_port = client.register_port("sine_out", AudioOutSpec::default()).unwrap();

    let sample_rate = client.sample_rate();
    let mut maybe_osc: Option<Oscillator> = None;
    Oscillator {
        freq: 440.0,
        phase: 0.0,
        acc: 0.0,
        fbuf: 0.0,
        fbuf2: 0.0,
    };

    let (tx, rx) = channel();

    let process = ClosureProcessHandler::new(move |_: &Client, ps: &ProcessScope| -> JackControl {
        use miosc::MioscMessage as MM;
        let mut out_p = AudioOutPort::new(&mut out_port, ps);
        let out: &mut [f32] = &mut out_p;

        match rx.try_recv() {
            Ok(MM::NoteOn(_, pitch, _)) => {
                maybe_osc = Some(Oscillator {
                    freq: 440.0 * (pitch / 12.0).exp2() as f64,
                    phase: 0.0,
                    acc: 0.0,
                    fbuf: 0.0,
                    fbuf2: 0.0,
                })
            },
            Ok(MM::NoteOff(_)) => {
                maybe_osc = None;
                for v in out.iter_mut() {
                    *v = 0.0;
                }
            },
            _ => (),
        }

        if let Some(ref mut osc) = maybe_osc {
            osc.make_noise(out, sample_rate as f64)
        };
        
        JackControl::Continue
    });

    let _active_client = AsyncClient::new(client, (), process).unwrap();

    let socket = UdpSocket::bind("127.0.0.1:3579").unwrap();
    let mut buf = [0u8; 1024];
    loop {
        use miosc::MioscMessage as MM;
        if let Ok((n, _)) = socket.recv_from(&mut buf) {
            let pkg = rosc::decoder::decode(&buf[..n]);
            match pkg {
                Ok(rosc::OscPacket::Message(msg)) => {
                    if let Ok(msg) = miosc::into_miosc(msg) {
                        drop(tx.send(msg));
                    }
                }
                _ => (),
            }
            
        }

        let dt = ::std::time::Duration::from_millis(8);
        ::std::thread::sleep(dt);
    }
}
