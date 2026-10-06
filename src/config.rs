use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub video_device: String,
    pub video_input: u32,
    pub standard: String,
    pub audio_device: String,
    pub audio_channel: u32,
    pub stereo: bool,
    pub output_dir: PathBuf,
    pub encoder: String,
    pub quality: u32,
    pub x264_preset: String,
    pub preview: bool,
    pub keep_master: bool,
    pub capture_mp4: bool,
    pub idle_timeout_secs: f64,
    pub write_timeout_secs: f64,
    pub auto_reset: bool,
    pub render_device: String,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            video_device: "/dev/video0".into(),
            video_input: 0,
            standard: "auto".into(),
            audio_device: "auto-scarlett".into(),
            audio_channel: 1,
            stereo: false,
            output_dir: "/srv/vhs".into(),
            encoder: "vaapi".into(),
            quality: 24,
            x264_preset: "veryfast".into(),
            preview: true,
            keep_master: false,
            capture_mp4: true,
            idle_timeout_secs: 15.,
            write_timeout_secs: 3.,
            auto_reset: true,
            render_device: "/dev/dri/renderD128".into(),
        }
    }
}
impl Config {
    pub fn load(path: Option<&Path>) -> Result<Self> {
        let mut config = if let Some(path) = path {
            toml::from_str::<Self>(
                &std::fs::read_to_string(path)
                    .with_context(|| format!("reading {}", path.display()))?,
            )?
        } else if Path::new("/usr/local/etc/vhs-rs.toml").exists() {
            toml::from_str::<Self>(&std::fs::read_to_string("/usr/local/etc/vhs-rs.toml")?)?
        } else {
            Self::default()
        };
        if let Ok(standard) = std::env::var("VHS_STANDARD") {
            config.standard = standard;
        }
        config.validate()?;
        Ok(config)
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.encoder == "vaapi" || self.encoder == "x264",
            "encoder must be vaapi or x264"
        );
        ensure!(
            self.keep_master || self.capture_mp4,
            "at least one output must be enabled"
        );
        ensure!(self.quality <= 51, "quality must be between 0 and 51");
        ensure!(self.audio_channel < 2, "audio_channel must be 0 or 1");
        ensure!(
            self.idle_timeout_secs.is_finite() && (0.1..=120.).contains(&self.idle_timeout_secs),
            "idle timeout must be 0.1–120 seconds"
        );
        ensure!(
            self.write_timeout_secs.is_finite() && (0.1..=30.).contains(&self.write_timeout_secs),
            "write timeout must be 0.1–30 seconds"
        );
        Ok(())
    }
    pub fn audio(&self) -> Result<String> {
        if self.audio_device != "auto-scarlett" {
            return Ok(self.audio_device.clone());
        }
        let cards = std::fs::read_to_string("/proc/asound/cards")?;
        let card = cards
            .lines()
            .find(|line| line.contains("Scarlett"))
            .and_then(|line| line.split_whitespace().next())
            .context("Scarlett Solo not found; connect it or configure audio_device")?;
        ensure!(card.parse::<u32>().is_ok(), "invalid Scarlett card number");
        Ok(format!("hw:{card},0"))
    }
}

pub fn duration(value: &str) -> Result<f64> {
    let parts: Vec<&str> = value.split(':').collect();
    ensure!(
        !parts.is_empty() && parts.len() <= 3,
        "use seconds or HH:MM:SS"
    );
    let mut seconds = 0.;
    for (index, part) in parts.iter().enumerate() {
        let n = part.parse::<f64>().context("invalid duration")?;
        ensure!(
            n.is_finite() && n >= 0. && (index == 0 || n < 60.),
            "invalid duration component"
        );
        if index + 1 < parts.len() {
            ensure!(n.fract() == 0., "only seconds can be fractional");
        }
        seconds = seconds * 60. + n;
    }
    ensure!(
        seconds > 0. && seconds <= 7. * 24. * 3600.,
        "duration must be positive and at most seven days"
    );
    Ok(seconds)
}
pub fn valid_name(name: &str) -> Result<()> {
    ensure!(!name.is_empty() && name.len()<=120 && !name.starts_with('.') && name.bytes().all(|c|c.is_ascii_alphanumeric() || b"-_.".contains(&c)),
        "recording name must use letters, numbers, dots, underscores, or hyphens (no paths or leading dot)");
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn duration_is_validated() {
        assert_eq!(duration("02:05:00").unwrap(), 7500.);
        assert_eq!(duration("1.5").unwrap(), 1.5);
        for bad in ["NaN", "inf", "-1", "00:61:00", "1:2:3:4", "0", "1.5:00"] {
            assert!(duration(bad).is_err(), "{bad}");
        }
    }
    #[test]
    fn output_paths_cannot_escape_directory() {
        for bad in ["", "../x", "/tmp/a", ".hidden", "x/y"] {
            assert!(valid_name(bad).is_err());
        }
        assert!(valid_name("tape-14").is_ok());
    }
    #[test]
    fn unknown_settings_fail_instead_of_silently_defaulting() {
        assert!(toml::from_str::<Config>("keep_mastre = true").is_err());
    }
}
