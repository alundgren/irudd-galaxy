//! F8 notes: what the owner thought, tagged with the combination, the place and the time,
//! with a shot of the moment. The note is typed in the window; Enter saves it to
//! `notes/notes.md` in the lab's folder and the shot beside it.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use render::RgbaImage;

pub struct Draft {
    pub text: String,
    pub image: Option<RgbaImage>,
    pub combo: String,
    pub place: String,
    /// Seconds played when F8 was pressed.
    pub play_s: f32,
    pub at: SystemTime,
}

/// `notes/` in the lab's folder (kept out of git).
pub fn folder() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("notes")
}

/// Year, month, day, hour, minute, second in UTC.
fn utc(t: SystemTime) -> (i64, u32, u32, u32, u32, u32) {
    let s = t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64;
    let (days, rem) = (s.div_euclid(86_400), s.rem_euclid(86_400));
    // Days to a civil date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (
        y,
        m,
        d,
        (rem / 3600) as u32,
        (rem % 3600 / 60) as u32,
        (rem % 60) as u32,
    )
}

/// Saves the note and its shot; returns the notes file.
pub fn save(dir: &Path, d: &Draft) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let (y, mo, da, h, mi, s) = utc(d.at);
    let stamp = format!("{y:04}{mo:02}{da:02}-{h:02}{mi:02}{s:02}");
    let shot = d.image.as_ref().and_then(|img| {
        let name = format!("{stamp}-{}.png", d.place);
        img.save_png(dir.join(&name)).ok().map(|_| name)
    });
    let file = dir.join("notes.md");
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file)
        .map_err(|e| format!("{}: {e}", file.display()))?;
    let played = d.play_s as u32;
    let text = if d.text.trim().is_empty() {
        "(no words)"
    } else {
        d.text.trim()
    };
    writeln!(
        f,
        "## {y:04}-{mo:02}-{da:02} {h:02}:{mi:02}:{s:02} UTC · {} · {}\n\nPlayed {} min {} s.{}\n\n{text}\n",
        d.combo,
        d.place,
        played / 60,
        played % 60,
        shot.map(|s| format!(" Shot: [{s}]({s})")).unwrap_or_default(),
    )
    .map_err(|e| e.to_string())?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn dates_come_out_in_utc() {
        let t = UNIX_EPOCH + Duration::from_secs(1_793_543_696); // 2026-11-01 14:34:56
        assert_eq!(utc(t), (2026, 11, 1, 14, 34, 56));
    }

    #[test]
    fn a_note_is_appended_with_its_tags() {
        let dir = std::env::temp_dir().join(format!("places-lab-notes-{}", std::process::id()));
        let d = Draft {
            text: "The office feels close".into(),
            image: None,
            combo: "cut,staged,bubble,still".into(),
            place: "office".into(),
            play_s: 75.0,
            at: SystemTime::now(),
        };
        let f = save(&dir, &d).unwrap();
        let s = std::fs::read_to_string(f).unwrap();
        assert!(s.contains("cut,staged,bubble,still · office"), "{s}");
        assert!(s.contains("Played 1 min 15 s."), "{s}");
        assert!(s.contains("The office feels close"), "{s}");
        std::fs::remove_dir_all(&dir).ok();
    }
}
