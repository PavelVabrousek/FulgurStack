//! Solving a whole stack: one frame of each camera group is searched blind,
//! the others start from the scale, parity and centre of a solved sibling. A
//! frame no sibling fits gets a blind search of its own, whose solution then
//! seeds the rest of its group. Every solution passes the same refinement and
//! held-out check, however it was found.
use super::SkyFrame;
use super::catalogue;
use super::solve::{self, BlindCache, Prior, Solution};
use rayon::prelude::*;
use serde::Serialize;
use std::collections::BTreeMap;
use std::time::Instant;

#[derive(Clone, Debug)]
pub struct Outcome {
    pub solution: Result<Solution, String>,
    /// 1 blind search of a group, 2 seeded from a sibling, 3 last pass with
    /// the centres of solved fields as hints.
    pub pass: u8,
    pub seconds: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct Round {
    pub blind: usize,
    pub seeded: usize,
    pub solved: usize,
    pub seconds: f64,
}

/// Solves `frames`. `groups[i]` names frame i's camera (instrument, sensor
/// size, mosaic); `header_hints[i]` is its header RA/Dec, if any; `hint` is
/// the target position.
pub fn solve_all(
    frames: &[SkyFrame<'_>],
    groups: &[String],
    header_hints: &[Option<(f64, f64)>],
    hint: (f64, f64),
    cache: &BlindCache,
    max_rms: f64,
) -> (Vec<Outcome>, Vec<Round>) {
    let start = Instant::now();
    let n = frames.len();
    let mut out: Vec<Outcome> = (0..n)
        .map(|_| Outcome {
            solution: Err("not solved".into()),
            pass: 0,
            seconds: 0.,
        })
        .collect();
    let base_hints = |i: usize| {
        let mut h = vec![];
        if let Some(x) = header_hints[i] {
            h.push(x);
        }
        h.push(hint);
        h
    };
    let mut priors: BTreeMap<&str, Vec<Prior>> = BTreeMap::new();
    let mut tried = vec![0usize; n];
    let mut blind_done = vec![false; n];
    let mut rounds = vec![];
    for _ in 0..8 {
        let pending: Vec<usize> = (0..n).filter(|&i| out[i].solution.is_err()).collect();
        if pending.is_empty() {
            break;
        }
        // Blind: in each group the pending frame with most stars that has
        // not had a blind search yet.
        let mut reps: BTreeMap<&str, usize> = BTreeMap::new();
        for &i in &pending {
            if blind_done[i] {
                continue;
            }
            let better = reps
                .get(groups[i].as_str())
                .is_none_or(|&r| frames[i].stars.len() > frames[r].stars.len());
            if better {
                reps.insert(groups[i].as_str(), i);
            }
        }
        let reps: Vec<usize> = reps.into_values().collect();
        let results: Vec<(usize, Result<Solution, String>, f64)> = reps
            .par_iter()
            .map(|&i| {
                let s = Instant::now();
                let r = solve::solve(&frames[i], &base_hints(i), cache, max_rms, None);
                (i, r, s.elapsed().as_secs_f64())
            })
            .collect();
        fulgur_core::telemetry::timeline("sky blind solves");
        for (i, r, secs) in results {
            blind_done[i] = true;
            out[i].seconds += secs;
            if let Ok(s) = &r {
                priors
                    .entry(groups[i].as_str())
                    .or_default()
                    .push(Prior::of(s));
                out[i].pass = 1;
            }
            out[i].solution = r;
        }
        // Seeded: each pending frame tries its group's untried priors.
        let pending: Vec<usize> = (0..n).filter(|&i| out[i].solution.is_err()).collect();
        let seeded: Vec<(usize, Option<Solution>, f64, usize)> = pending
            .par_iter()
            .map(|&i| {
                let list = priors.get(groups[i].as_str()).cloned().unwrap_or_default();
                let s = Instant::now();
                let found = list.iter().skip(tried[i]).find_map(|p| {
                    solve::solve(&frames[i], &[p.centre, hint], cache, max_rms, Some(p)).ok()
                });
                (i, found, s.elapsed().as_secs_f64(), list.len())
            })
            .collect();
        fulgur_core::telemetry::timeline("sky seeded solves");
        let mut gained = 0;
        for (i, sol, secs, count) in seeded {
            tried[i] = count;
            out[i].seconds += secs;
            if let Some(sol) = sol {
                out[i].solution = Ok(sol);
                out[i].pass = 2;
                gained += 1;
            }
        }
        rounds.push(Round {
            blind: reps.len(),
            seeded: gained,
            solved: out.iter().filter(|o| o.solution.is_ok()).count(),
            seconds: start.elapsed().as_secs_f64(),
        });
        if reps.is_empty() {
            break;
        }
    }
    // Last pass for frames nothing placed: hints from the solved fields.
    let mut centres: Vec<(f64, f64)> = vec![];
    for o in &out {
        if let Ok(s) = &o.solution {
            let (ra, dec) = (s.centre_ra, s.centre_dec);
            if centres
                .iter()
                .all(|c| catalogue::separation(c.0, c.1, ra, dec) > 0.3)
                && catalogue::separation(hint.0, hint.1, ra, dec) > 0.3
            {
                centres.push((ra, dec));
            }
        }
    }
    centres.truncate(8);
    if !centres.is_empty() {
        let last: Vec<(usize, Option<Solution>, f64)> = (0..n)
            .into_par_iter()
            .filter(|&i| out[i].solution.is_err())
            .map(|i| {
                let s = Instant::now();
                let r = solve::solve(&frames[i], &centres, cache, max_rms, None).ok();
                (i, r, s.elapsed().as_secs_f64())
            })
            .collect();
        for (i, r, secs) in last {
            out[i].seconds += secs;
            if let Some(r) = r {
                out[i].solution = Ok(r);
                out[i].pass = 3;
            }
        }
    }
    (out, rounds)
}
