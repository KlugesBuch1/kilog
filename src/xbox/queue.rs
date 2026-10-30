use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::config::kilog_dir;
use crate::error::Error;
use crate::xbox::achievements::TitleAchievements;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnlockQueue {
    pub title_id: u64,
    pub gamertag: String,
    pub entries: Vec<QueueEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueEntry {
    pub id: String,
    pub name: String,
    pub reference_unlocked_at: String,
    pub delay_seconds: u64,
    pub done: bool,
    #[serde(default)]
    pub skip: bool,
    #[serde(default)]
    pub remaining_seconds: Option<u64>,
    #[serde(skip)]
    pub rolled_delay: Option<u64>,
}

impl UnlockQueue {
    pub fn pending(&self) -> bool {
        self.entries.iter().any(|entry| !entry.done && !entry.skip)
    }
}

pub fn queue_path() -> Result<std::path::PathBuf, Error> {
    Ok(kilog_dir()?.join("queue.json"))
}

pub fn load_queue() -> Result<Option<UnlockQueue>, Error> {
    let path = queue_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(path)?;
    Ok(Some(serde_json::from_str(&text)?))
}

pub fn save_queue(queue: &UnlockQueue) -> Result<(), Error> {
    let path = queue_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(queue)?)?;
    Ok(())
}

pub fn build_queue(
    title_id: u64,
    gamertag: &str,
    local: &TitleAchievements,
    reference: &TitleAchievements,
    supported: Option<&HashSet<String>>,
) -> UnlockQueue {
    let locked: HashSet<&str> = local
        .achievements
        .iter()
        .filter(|achievement| !achievement.is_unlocked())
        .filter(|achievement| supported.is_none_or(|ids| ids.contains(achievement.id.as_str())))
        .map(|achievement| achievement.id.as_str())
        .collect();
    let mut theirs: Vec<(&str, &str, &str)> = reference
        .achievements
        .iter()
        .filter(|achievement| achievement.is_unlocked() && locked.contains(achievement.id.as_str()))
        .filter_map(|achievement| {
            let at = achievement.unlocked_at.as_deref()?;
            Some((achievement.id.as_str(), achievement.name.as_str(), at))
        })
        .collect();
    theirs.sort_by(|left, right| left.2.cmp(right.2));
    let mut entries = Vec::new();
    let mut previous: Option<i64> = None;
    for (id, name, at) in theirs {
        let delay_seconds = match (previous, unix_seconds(at)) {
            (Some(earlier), Some(now)) => now.saturating_sub(earlier).max(0) as u64,
            _ => 0,
        };
        if let Some(now) = unix_seconds(at) {
            previous = Some(now);
        }
        entries.push(QueueEntry {
            id: id.to_owned(),
            name: name.to_owned(),
            reference_unlocked_at: at.to_owned(),
            delay_seconds,
            done: false,
            skip: false,
            remaining_seconds: None,
            rolled_delay: None,
        });
    }
    UnlockQueue {
        title_id,
        gamertag: gamertag.to_owned(),
        entries,
    }
}

fn unix_seconds(raw: &str) -> Option<i64> {
    let raw = raw.trim();
    if raw.len() < 19 {
        return None;
    }
    let year: i32 = raw.get(0..4)?.parse().ok()?;
    let month: u32 = raw.get(5..7)?.parse().ok()?;
    let day: u32 = raw.get(8..10)?.parse().ok()?;
    let hour: u32 = raw.get(11..13)?.parse().ok()?;
    let minute: u32 = raw.get(14..16)?.parse().ok()?;
    let second: u32 = raw.get(17..19)?.parse().ok()?;
    let days = days_from_civil(year, month, day)?;
    Some(days * 86_400 + i64::from(hour) * 3_600 + i64::from(minute) * 60 + i64::from(second))
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::build_queue;
    use crate::xbox::achievements::{Achievement, ProgressState, TitleAchievements};

    fn achievement(id: &str, unlocked_at: Option<&str>) -> Achievement {
        Achievement {
            id: id.to_owned(),
            name: id.to_owned(),
            description: String::new(),
            locked_description: String::new(),
            gamerscore: 0,
            progress_state: if unlocked_at.is_some() {
                ProgressState::Unlocked
            } else {
                ProgressState::Locked
            },
            in_progress: false,
            is_secret: false,
            rarity: None,
            unlocked_at: unlocked_at.map(str::to_owned),
            icon_url: None,
        }
    }

    fn board(achievements: Vec<Achievement>) -> TitleAchievements {
        TitleAchievements {
            title_name: None,
            service_config_id: None,
            achievements,
        }
    }

    #[test]
    fn partial_support_keeps_supported_achievements_and_their_gaps() {
        let local = board(vec![
            achievement("1", None),
            achievement("2", None),
            achievement("3", None),
        ]);
        let reference = board(vec![
            achievement("1", Some("2020-01-01T00:00:00Z")),
            achievement("2", Some("2020-01-01T00:01:00Z")),
            achievement("3", Some("2020-01-01T00:03:00Z")),
        ]);
        let supported = HashSet::from(["1".to_owned(), "3".to_owned()]);
        let queue = build_queue(1, "ref", &local, &reference, Some(&supported));
        assert_eq!(queue.entries.len(), 2);
        assert_eq!(queue.entries[0].id, "1");
        assert_eq!(queue.entries[0].delay_seconds, 0);
        assert_eq!(queue.entries[1].id, "3");
        assert_eq!(queue.entries[1].delay_seconds, 180);
    }
}

fn days_from_civil(year: i32, month: u32, day: u32) -> Option<i64> {
    if !(1..=12).contains(&month) || day == 0 || day > 31 {
        return None;
    }
    let mut year = i64::from(year);
    let month = i64::from(month);
    let day = i64::from(day);
    if month <= 2 {
        year -= 1;
    }
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month_prime = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * month_prime + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    Some(era * 146_097 + day_of_era - 719_468)
}
