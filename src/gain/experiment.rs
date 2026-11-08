//! Holdout experiments: measure what styles really do.
//!
//! `lagom experiment start --holdout 0.2` makes about 20% of new sessions run without
//! styles (compression stays on). The arm comes from a hash of the session id, so a
//! session and its subagents always share an arm. `lagom stats --experiment` compares
//! output tokens per turn between the arms with a bootstrap confidence interval.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use serde_json::{Value, json};

use super::SessionUsage;
use crate::store;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Experiment {
    /// Share of sessions without styles, 0.0-0.9.
    pub holdout: f64,
    /// Unix seconds.
    pub started: u64,
    pub ends: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Arm {
    Styled,
    Holdout,
}

fn path() -> PathBuf {
    store::lagom_dir().join("experiment.json")
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl Experiment {
    pub fn start(holdout: f64, days: u64) -> std::io::Result<Experiment> {
        let started = now();
        let e = Experiment {
            holdout,
            started,
            ends: started + days * 86_400,
        };
        fs::create_dir_all(store::lagom_dir())?;
        let v = json!({"v": 1, "holdout": holdout, "started": e.started, "ends": e.ends});
        fs::write(path(), v.to_string() + "\n")?;
        Ok(e)
    }

    /// The saved experiment, running or finished.
    pub fn load() -> Option<Experiment> {
        let v: Value = serde_json::from_str(&fs::read_to_string(path()).ok()?).ok()?;
        Some(Experiment {
            holdout: v["holdout"].as_f64()?,
            started: v["started"].as_u64()?,
            ends: v["ends"].as_u64()?,
        })
    }

    /// The experiment, if it is running now.
    pub fn active() -> Option<Experiment> {
        Experiment::load().filter(|e| now() < e.ends)
    }

    pub fn stop() -> std::io::Result<()> {
        match fs::remove_file(path()) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }

    /// Deterministic arm for a session id.
    pub fn arm(&self, session: &str) -> Arm {
        let h = u64::from_str_radix(&store::content_id(session), 16).unwrap_or(0);
        if (h % 10_000) as f64 / 10_000.0 < self.holdout {
            Arm::Holdout
        } else {
            Arm::Styled
        }
    }
}

/// Small deterministic PRNG (xorshift64*), enough for resampling.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545F4914F6CDD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn mean(v: &[f64]) -> f64 {
    v.iter().sum::<f64>() / v.len().max(1) as f64
}

/// Result of comparing the arms on output tokens per turn.
#[derive(Debug, Clone, PartialEq)]
pub struct Comparison {
    pub styled_sessions: usize,
    pub holdout_sessions: usize,
    pub styled_mean: f64,
    pub holdout_mean: f64,
    /// styled − holdout, with a 95% bootstrap interval.
    pub diff: f64,
    pub ci_low: f64,
    pub ci_high: f64,
}

/// Bootstrap the difference of means (`styled − holdout`), resampling sessions.
pub fn compare(styled: &[f64], holdout: &[f64], resamples: usize, seed: u64) -> Option<Comparison> {
    if styled.len() < 2 || holdout.len() < 2 {
        return None;
    }
    let mut rng = Rng(seed | 1);
    let mut diffs: Vec<f64> = (0..resamples)
        .map(|_| {
            let s: f64 = (0..styled.len())
                .map(|_| styled[rng.below(styled.len())])
                .sum::<f64>()
                / styled.len() as f64;
            let h: f64 = (0..holdout.len())
                .map(|_| holdout[rng.below(holdout.len())])
                .sum::<f64>()
                / holdout.len() as f64;
            s - h
        })
        .collect();
    diffs.sort_by(f64::total_cmp);
    let at = |q: f64| diffs[((resamples as f64 * q) as usize).min(resamples - 1)];
    Some(Comparison {
        styled_sessions: styled.len(),
        holdout_sessions: holdout.len(),
        styled_mean: mean(styled),
        holdout_mean: mean(holdout),
        diff: mean(styled) - mean(holdout),
        ci_low: at(0.025),
        ci_high: at(0.975),
    })
}

/// Per-session mean output tokens per main turn, split by arm.
pub fn per_arm(sessions: &[SessionUsage], arms: &HashMap<String, Arm>) -> (Vec<f64>, Vec<f64>) {
    let (mut styled, mut holdout) = (Vec::new(), Vec::new());
    for s in sessions.iter().filter(|s| !s.turns.is_empty()) {
        let v = s.turns.iter().map(|t| t.1 as f64).sum::<f64>() / s.turns.len() as f64;
        match arms.get(&s.session_id) {
            Some(Arm::Styled) => styled.push(v),
            Some(Arm::Holdout) => holdout.push(v),
            None => {}
        }
    }
    (styled, holdout)
}

/// Session arms recorded since the experiment started.
pub fn recorded_arms(since: u64) -> HashMap<String, Arm> {
    let mut out = HashMap::new();
    let Ok(text) = fs::read_to_string(store::lagom_dir().join("stats.jsonl")) else {
        return out;
    };
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if v["ts"].as_u64().is_none_or(|t| t < since) {
            continue;
        }
        let arm = match v["arm"].as_str() {
            Some("styled") => Arm::Styled,
            Some("holdout") => Arm::Holdout,
            _ => continue,
        };
        if let Some(s) = v["session"].as_str() {
            out.insert(s.to_string(), arm);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arms_are_deterministic_and_near_the_share() {
        let e = Experiment {
            holdout: 0.2,
            started: 0,
            ends: u64::MAX,
        };
        assert_eq!(e.arm("session-x"), e.arm("session-x"));
        let held = (0..10_000)
            .filter(|i| e.arm(&format!("s-{i}")) == Arm::Holdout)
            .count();
        assert!((1_800..2_200).contains(&held), "{held}");
        let none = Experiment { holdout: 0.0, ..e };
        assert!((0..1000).all(|i| none.arm(&format!("s-{i}")) == Arm::Styled));
    }

    #[test]
    fn bootstrap_recovers_a_known_effect() {
        // Holdout sessions average 500 output tokens per turn, styled ones 400 (−100).
        let mut rng = Rng(42);
        let noisy = |rng: &mut Rng, base: f64| base + (rng.below(201) as f64 - 100.0);
        let styled: Vec<f64> = (0..80).map(|_| noisy(&mut rng, 400.0)).collect();
        let holdout: Vec<f64> = (0..30).map(|_| noisy(&mut rng, 500.0)).collect();
        let c = compare(&styled, &holdout, 10_000, 7).unwrap();
        assert!(c.ci_low <= -100.0 && -100.0 <= c.ci_high, "{c:?}");
        assert!(c.ci_high < 0.0, "effect is clearly below zero: {c:?}");
        // No effect: the interval contains zero.
        let a: Vec<f64> = (0..60).map(|_| noisy(&mut rng, 450.0)).collect();
        let b: Vec<f64> = (0..60).map(|_| noisy(&mut rng, 450.0)).collect();
        let c = compare(&a, &b, 10_000, 9).unwrap();
        assert!(c.ci_low < 0.0 && 0.0 < c.ci_high, "{c:?}");
        assert!(compare(&[1.0], &[2.0, 3.0], 100, 1).is_none());
    }
}
