use std::f32::consts::TAU;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const SAMPLE_RATE: u32 = 44_100;

pub fn play(setting: &str) {
    if setting == "bell" {
        bell();
        return;
    }
    let path = if setting == "metal-gear" {
        match built_in_sound() {
            Ok(path) => path,
            Err(_) => {
                bell();
                return;
            }
        }
    } else {
        PathBuf::from(crate::expand::variables(setting))
    };
    if !path.is_file() || !play_file(&path) {
        bell();
    }
}

fn play_file(path: &Path) -> bool {
    for player in ["pw-play", "paplay", "aplay"] {
        let mut command = Command::new(player);
        if player == "aplay" {
            command.arg("-q");
        }
        match command
            .arg(path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(mut child) => {
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
                return true;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(_) => return false,
        }
    }
    false
}

fn bell() {
    eprint!("\x07");
    let _ = io::stderr().flush();
}

fn built_in_sound() -> io::Result<PathBuf> {
    let path = crate::state::ensure_directory()?.join("metal-gear-alert.wav");
    if !path.is_file() {
        fs::write(&path, synthesized_alert())?;
    }
    Ok(path)
}

fn synthesized_alert() -> Vec<u8> {
    let sample_count = SAMPLE_RATE * 2 / 5;
    let data_length = sample_count * 2;
    let mut wav = Vec::with_capacity(44 + data_length as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_length).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16_u32.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes());
    wav.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    wav.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes());
    wav.extend_from_slice(&2_u16.to_le_bytes());
    wav.extend_from_slice(&16_u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_length.to_le_bytes());

    for index in 0..sample_count {
        let time = index as f32 / SAMPLE_RATE as f32;
        let progress = index as f32 / sample_count as f32;
        let frequency = if time < 0.09 {
            760.0 + time / 0.09 * 980.0
        } else {
            1_280.0 + (time * 42.0).sin() * 55.0
        };
        let attack = (time / 0.006).min(1.0);
        let envelope = attack * (1.0 - progress).powf(1.8);
        let tone = (TAU * frequency * time).sin()
            + 0.38 * (TAU * frequency * 2.01 * time).sin()
            + 0.16 * (TAU * frequency * 3.97 * time).sin();
        let sample = (tone * envelope * 0.48).clamp(-1.0, 1.0);
        wav.extend_from_slice(&((sample * i16::MAX as f32) as i16).to_le_bytes());
    }
    wav
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synthesized_alert_is_a_complete_pcm_wav() {
        let wav = synthesized_alert();
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[36..40], b"data");
        assert_eq!(wav.len(), 44 + (SAMPLE_RATE * 2 / 5 * 2) as usize);
    }
}
