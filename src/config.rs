use std::path::PathBuf;

use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};

use crate::error::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub force_region: bool,
    pub spoof_open_achievements: bool,
    pub stop_on_spoof_fail: bool,
    pub use_randomized_delays: bool,
    pub randomize_delays: DelayRandomize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DelayRandomize {
    pub over_1_min: DelayBand,
    pub over_10_min: DelayBand,
    pub over_1_hour: DelayBand,
    pub over_5_hours: DelayBand,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DelayBand {
    pub enabled: bool,
    pub min: u64,
    pub max: u64,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            force_region: true,
            spoof_open_achievements: false,
            stop_on_spoof_fail: false,
            use_randomized_delays: false,
            randomize_delays: DelayRandomize::default(),
        }
    }
}

impl Default for DelayRandomize {
    fn default() -> Self {
        Self {
            over_1_min: DelayBand {
                enabled: false,
                min: 10,
                max: 30,
            },
            over_10_min: DelayBand {
                enabled: false,
                min: 30,
                max: 120,
            },
            over_1_hour: DelayBand {
                enabled: false,
                min: 5,
                max: 15,
            },
            over_5_hours: DelayBand {
                enabled: false,
                min: 6,
                max: 9,
            },
        }
    }
}

impl Default for DelayBand {
    fn default() -> Self {
        Self {
            enabled: false,
            min: 0,
            max: 0,
        }
    }
}

const MINUTE: u64 = 60;
const HOUR: u64 = 60 * MINUTE;

pub fn randomize_delay(seconds: u64, settings: &DelayRandomize) -> u64 {
    randomize_delay_with(seconds, settings, || OsRng.next_u64())
}

fn randomize_delay_with(
    seconds: u64,
    settings: &DelayRandomize,
    mut next: impl FnMut() -> u64,
) -> u64 {
    if settings.over_5_hours.enabled && seconds > 5 * HOUR {
        let low = settings.over_5_hours.min.saturating_mul(HOUR);
        let high = settings.over_5_hours.max.saturating_mul(HOUR);
        return pick(low, high, &mut next);
    }
    if let Some(jitter) = jitter_band(seconds, settings) {
        let low = jitter.band.min.saturating_mul(jitter.unit);
        let high = jitter.band.max.saturating_mul(jitter.unit);
        let delta = pick(low, high, &mut next);
        if next() % 2 == 0 {
            seconds.saturating_sub(delta)
        } else {
            seconds.saturating_add(delta)
        }
    } else {
        seconds
    }
}

struct Jitter<'a> {
    band: &'a DelayBand,
    unit: u64,
}

fn jitter_band(seconds: u64, settings: &DelayRandomize) -> Option<Jitter<'_>> {
    if settings.over_1_hour.enabled && seconds > HOUR {
        return Some(Jitter {
            band: &settings.over_1_hour,
            unit: MINUTE,
        });
    }
    if settings.over_10_min.enabled && seconds > 10 * MINUTE {
        return Some(Jitter {
            band: &settings.over_10_min,
            unit: 1,
        });
    }
    if settings.over_1_min.enabled && seconds > MINUTE {
        return Some(Jitter {
            band: &settings.over_1_min,
            unit: 1,
        });
    }
    None
}

fn pick(min: u64, max: u64, next: &mut impl FnMut() -> u64) -> u64 {
    let (low, high) = if min <= max { (min, max) } else { (max, min) };
    if low == high {
        return low;
    }
    let span = high - low + 1;
    low + next() % span
}

impl AppConfig {
    pub fn load() -> Result<Self, Error> {
        let path = config_path()?;
        if !path.exists() {
            let config = Self::default();
            config.save()?;
            return Ok(config);
        }

        let text = std::fs::read_to_string(&path)?;
        Ok(serde_json::from_str(&text)?)
    }

    pub fn save(&self) -> Result<(), Error> {
        let path = config_path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }
}

pub fn kilog_dir() -> Result<PathBuf, Error> {
    let dirs = directories::UserDirs::new().ok_or(Error::NoDocuments)?;
    let docs = dirs.document_dir().ok_or(Error::NoDocuments)?;
    Ok(docs.join("kilog"))
}

pub fn config_path() -> Result<PathBuf, Error> {
    Ok(kilog_dir()?.join("config.json"))
}

pub fn events_dir() -> Result<PathBuf, Error> {
    let dir = kilog_dir()?.join("events");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::{AppConfig, DelayRandomize, randomize_delay_with};

    #[test]
    fn missing_settings_stay_off() {
        let config: AppConfig = serde_json::from_str(r#"{"force_region":false}"#).unwrap();
        assert!(!config.spoof_open_achievements);
        assert!(!config.stop_on_spoof_fail);
        assert!(!config.use_randomized_delays);
        assert!(!config.randomize_delays.over_1_min.enabled);
        assert!(!config.randomize_delays.over_5_hours.enabled);
        assert_eq!(
            randomize_delay_with(9_000, &config.randomize_delays, || panic!("no roll")),
            9_000
        );
    }

    #[test]
    fn five_hour_tier_replaces_longer_delays() {
        let mut settings = DelayRandomize::default();
        settings.over_1_min.enabled = true;
        settings.over_5_hours.enabled = true;
        settings.over_5_hours.min = 6;
        settings.over_5_hours.max = 6;
        assert_eq!(
            randomize_delay_with(6 * 3_600, &settings, || panic!("fixed ends")),
            6 * 3_600
        );
    }

    #[test]
    fn jitter_adds_or_subtracts_inside_the_range() {
        let mut settings = DelayRandomize::default();
        settings.over_1_min.enabled = true;
        settings.over_1_min.min = 10;
        settings.over_1_min.max = 10;
        let mut subtract = std::iter::once(0);
        assert_eq!(
            randomize_delay_with(90, &settings, || subtract.next().unwrap()),
            80
        );
        let mut add = std::iter::once(1);
        assert_eq!(
            randomize_delay_with(90, &settings, || add.next().unwrap()),
            100
        );
    }

    #[test]
    fn subtract_never_goes_below_zero() {
        let mut settings = DelayRandomize::default();
        settings.over_1_min.enabled = true;
        settings.over_1_min.min = 500;
        settings.over_1_min.max = 500;
        let mut subtract = std::iter::once(0);
        assert_eq!(
            randomize_delay_with(90, &settings, || subtract.next().unwrap()),
            0
        );
    }

    #[test]
    fn lower_tier_is_used_when_the_longer_one_is_off() {
        let mut settings = DelayRandomize::default();
        settings.over_1_hour.enabled = true;
        settings.over_1_hour.min = 5;
        settings.over_1_hour.max = 5;
        settings.over_10_min.enabled = true;
        let mut add = std::iter::once(1);
        assert_eq!(
            randomize_delay_with(2 * 3_600, &settings, || add.next().unwrap()),
            2 * 3_600 + 5 * 60
        );
    }
}
