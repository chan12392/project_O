//! What one Codex `exec` turn used. `codex exec --json` ends a turn with its thread's running
//! token total, and a resumed thread (`exec resume`) carries the total of its earlier turns
//! (openai/codex#49574), so counting each report whole counts the earlier turns again. A turn's
//! own use is the growth since the total O last saw for that session, kept in o.db:
//!
//! ```text
//! codex_totals   session PK · input · cached · output · at
//! ```
//!
//! A report that is not at least the kept total in every count was not carried over from the
//! earlier turns: it is taken as that turn's own. A session O has no total for (its first turn,
//! or one last run through app-server, which reports each turn by itself) is counted as reported.

use std::path::Path;

use chrono::{DateTime, Duration, Utc};
use rusqlite::{params, OptionalExtension, TransactionBehavior};

use crate::run::index::{self, sql};
use crate::sys::js::iso;

/// A session not resumed for this long loses its total (its next turn is counted as reported).
const KEEP_DAYS: i64 = 90;
const TABLE: &str = "
CREATE TABLE IF NOT EXISTS codex_totals (
  session TEXT PRIMARY KEY, input REAL NOT NULL, cached REAL NOT NULL, output REAL NOT NULL, at TEXT NOT NULL
) WITHOUT ROWID;
";

/// This turn's `[input, cached, output]` out of the thread total a turn reported. When the total
/// cannot be kept or read, the report is returned as it is.
pub fn turn(home: &Path, session: &str, total: [f64; 3], now: DateTime<Utc>) -> [f64; 3] {
    since_last(home, session, total, now).unwrap_or(total)
}

fn since_last(home: &Path, session: &str, total: [f64; 3], now: DateTime<Utc>) -> std::io::Result<[f64; 3]> {
    let mut conn = index::open(home)?;
    conn.execute_batch(TABLE).map_err(sql)?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sql)?;
    let kept: Option<[f64; 3]> = tx
        .query_row("SELECT input, cached, output FROM codex_totals WHERE session = ?1", [session], |row| Ok([row.get(0)?, row.get(1)?, row.get(2)?]))
        .optional()
        .map_err(sql)?;
    let (used, next) = match kept {
        Some(kept) if (0..3).all(|i| total[i] >= kept[i]) => ([total[0] - kept[0], total[1] - kept[1], total[2] - kept[2]], total),
        // Not carried over: the thread's total so far is what was kept plus this turn.
        Some(kept) => (total, [kept[0] + total[0], kept[1] + total[1], kept[2] + total[2]]),
        None => (total, total),
    };
    tx.execute(
        "INSERT INTO codex_totals (session, input, cached, output, at) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(session) DO UPDATE SET input = excluded.input, cached = excluded.cached, output = excluded.output, at = excluded.at",
        params![session, next[0], next[1], next[2], iso(now)],
    ).map_err(sql)?;
    tx.execute("DELETE FROM codex_totals WHERE at < ?1", [iso(now - Duration::days(KEEP_DAYS))]).map_err(sql)?;
    tx.commit().map_err(sql)?;
    Ok(used)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_resumed_threads_total_counts_each_turn_once() {
        let home = std::env::temp_dir().join(format!("o-thread-total-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let now = Utc::now();
        // The first turn is its own total; a resumed one reports the thread's.
        assert_eq!(turn(&home, "s", [100.0, 40.0, 10.0], now), [100.0, 40.0, 10.0]);
        assert_eq!(turn(&home, "s", [260.0, 150.0, 25.0], now), [160.0, 110.0, 15.0]);
        // A turn whose usage never arrived is counted with the next one.
        assert_eq!(turn(&home, "s", [900.0, 700.0, 60.0], now), [640.0, 550.0, 35.0]);
        // A report below the kept total is that turn's own, and the total goes on from both.
        assert_eq!(turn(&home, "s", [50.0, 20.0, 5.0], now), [50.0, 20.0, 5.0]);
        assert_eq!(turn(&home, "s", [1000.0, 760.0, 70.0], now), [50.0, 40.0, 5.0]);
        // Another session has its own total; one unused for long starts again.
        assert_eq!(turn(&home, "other", [30.0, 0.0, 3.0], now), [30.0, 0.0, 3.0]);
        assert_eq!(turn(&home, "s", [1100.0, 800.0, 80.0], now + Duration::days(KEEP_DAYS + 1)), [100.0, 40.0, 10.0]);
        assert_eq!(turn(&home, "other", [45.0, 10.0, 4.0], now + Duration::days(KEEP_DAYS + 1)), [45.0, 10.0, 4.0]);
        std::fs::remove_dir_all(home).unwrap();
    }
}
