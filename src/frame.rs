use anyhow::{bail, ensure, Result};
use serde::Serialize;

#[derive(Debug, Clone, Copy)]
pub struct Rate {
    pub numerator: u64,
    pub denominator: u64,
}
impl Rate {
    pub fn pal() -> Self {
        Self {
            numerator: 25,
            denominator: 1,
        }
    }
    pub fn ntsc() -> Self {
        Self {
            numerator: 30000,
            denominator: 1001,
        }
    }
    pub fn ffmpeg(self) -> String {
        format!("{}/{}", self.numerator, self.denominator)
    }
    pub fn seconds(self, frames: u64) -> f64 {
        frames as f64 * self.denominator as f64 / self.numerator as f64
    }
}

/// Copy from original valid lines, with a stable preference for the preceding line.
/// All-zero frames have no recoverable picture and are rejected by the caller.
pub fn repair(data: &mut [u8], line: usize) -> Result<usize> {
    ensure!(
        line > 0 && data.len().is_multiple_of(line),
        "invalid frame dimensions"
    );
    let valid: Vec<usize> = data
        .chunks_exact(line)
        .enumerate()
        .filter_map(|(i, row)| row.iter().any(|v| *v != 0).then_some(i))
        .collect();
    ensure!(!valid.is_empty(), "frame contains no valid lines");
    let missing = data.len() / line - valid.len();
    let mut nearest = 0;
    for row in 0..data.len() / line {
        if data[row * line..(row + 1) * line].iter().any(|v| *v != 0) {
            continue;
        }
        while nearest + 1 < valid.len()
            && valid[nearest + 1].abs_diff(row) < valid[nearest].abs_diff(row)
        {
            nearest += 1;
        }
        data.copy_within(
            valid[nearest] * line..(valid[nearest] + 1) * line,
            row * line,
        );
    }
    Ok(missing)
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct Stats {
    pub received: u64,
    pub repaired: u64,
    pub repeated: u64,
    pub duplicates: u64,
    pub invalid: u64,
    pub output_frames: u64,
}

/// Frame numbers stay tied to the driver's timestamps, including missing frames.
pub struct Grid {
    start: Option<u64>,
    previous: Option<u64>,
    pub next: u64,
    rate: Rate,
}
impl Grid {
    pub fn new(rate: Rate) -> Self {
        Self {
            start: None,
            previous: None,
            next: 0,
            rate,
        }
    }
    pub fn place(&mut self, timestamp_us: u64) -> Result<Option<u64>> {
        if let Some(previous) = self.previous {
            ensure!(timestamp_us >= previous, "video clock moved backwards");
        }
        self.previous = Some(timestamp_us);
        let start = *self.start.get_or_insert(timestamp_us);
        let scaled = (timestamp_us - start) as u128 * self.rate.numerator as u128;
        let divisor = 1_000_000u128 * self.rate.denominator as u128;
        let slot = ((scaled + divisor / 2) / divisor) as u64;
        if slot < self.next {
            return Ok(None);
        }
        let missing = slot - self.next;
        if self.rate.seconds(missing) > 15.0 {
            bail!("video timestamp jumped more than 15 seconds");
        }
        self.next = slot + 1;
        Ok(Some(missing))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repairs_runs_and_edges() {
        for original in [
            vec![1, 2, 0, 0],
            vec![0, 0, 3, 0, 0],
            vec![1, 0, 3, 0],
            vec![1, 0, 0, 4],
        ] {
            let mut data = original.clone();
            assert_eq!(
                repair(&mut data, 1).unwrap(),
                original.iter().filter(|x| **x == 0).count()
            );
            assert!(data.iter().all(|x| *x > 0));
            for (before, after) in original.iter().zip(data) {
                if *before > 0 {
                    assert_eq!(*before, after);
                }
            }
        }
        let mut tie = [1, 0, 3];
        repair(&mut tie, 1).unwrap();
        assert_eq!(tie, [1, 1, 3]);
    }
    #[test]
    fn empty_picture_is_not_repaired() {
        assert!(repair(&mut [0; 8], 2).is_err());
    }
    #[test]
    fn preserves_time_across_gaps() {
        let mut grid = Grid::new(Rate::pal());
        assert_eq!(grid.place(1_000_000).unwrap(), Some(0));
        assert_eq!(grid.place(1_040_000).unwrap(), Some(0));
        assert_eq!(grid.place(1_160_000).unwrap(), Some(2));
        assert_eq!(grid.place(1_161_000).unwrap(), None);
        assert_eq!(grid.next, 5);
        assert!(grid.place(999).is_err());
    }
    #[test]
    fn ntsc_has_no_accumulated_rounding_drift() {
        let mut grid = Grid::new(Rate::ntsc());
        for frame in 0..(30000 * 4) {
            // More than an hour of timestamps.
            assert_eq!(
                grid.place(frame * 1001 * 1_000_000 / 30000).unwrap(),
                Some(0)
            );
        }
        assert_eq!(grid.next, 120000);
    }
}
