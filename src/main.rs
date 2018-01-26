#[allow(dead_code)]

extern crate jack;
extern crate miosc;
extern crate rosc;

use std::io;
use std::net::UdpSocket;
use std::str::FromStr;
use std::sync::mpsc::channel;

use jack::prelude::*;

trait Dsp {
    fn make_noise(&mut self, buf: &mut [f32], sample_rate: f64);
}

#[derive(Debug)]
struct Oscillator {
    freq: f64,
    time: f64,
}

fn poly_blep(t: f64, dt: f64) -> f64 {
    // 0 <= t < 1
    if t < dt {
        let t = t / dt;

        t*t + 2.0 * t - 1.0
    }
    // -1 < t < 0
    else if t > 1.0 - dt {
        let t = (t - 1.0) / dt;

        t*t + 2.0 * t - 1.0
    }
    else {
        0.0
    }
}

fn poly_saw(t: f64, dt: f64) -> f64 {
    let mut t = t + 0.5;
    if t >= 1.0 { t -= 1.0 }

    let naive_saw = 2.0 * t - 1.0;
    naive_saw - poly_blep(t, dt)
}


impl Dsp for Oscillator {
    fn make_noise(&mut self, buf: &mut [f32], sample_rate: f64) {
        let dt = 1.0 / sample_rate;
        let period = 1.0 / self.freq;

        for v in buf.iter_mut() {
            if self.time >= period {
                self.time -= period
            }

            let naive = self.time * self.freq - 0.5;
            let less_naive = naive + 0.5 * poly_blep(self.time * self.freq + 0.5, self.freq / sample_rate);
            *v = less_naive as f32;

            self.time += dt;
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
        time: 0.0,
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
                    time: 0.0,
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
