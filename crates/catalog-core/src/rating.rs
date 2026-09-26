//! Rating adjusted for the number of reviews (spec §5.3):
//!
//! ```text
//! R_b = (n × R + C × m) / (n + C)
//! ```
//!
//! `m` is the unweighted mean rating over the category's products with a
//! known rating, `C` the median review count over the same products.

/// An observed rating. Only built from a known average and at least one
/// review; a missing rating is `None` upstream, never zero.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RatingObs {
    avg: f32,
    count: u32,
}

impl RatingObs {
    pub fn new(avg: f32, count: u32) -> Option<RatingObs> {
        if count == 0 || !avg.is_finite() || !(0.0..=5.0).contains(&avg) {
            return None;
        }
        Some(RatingObs { avg, count })
    }

    pub fn avg(self) -> f32 {
        self.avg
    }

    pub fn count(self) -> u32 {
        self.count
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RatingPrior {
    /// Mean rating `m`.
    pub mean: f64,
    /// Median review count `C`.
    pub median_count: f64,
}

/// `None` when no product in the sample has a rating.
pub fn prior(sample: &[RatingObs]) -> Option<RatingPrior> {
    if sample.is_empty() {
        return None;
    }
    let len = u32::try_from(sample.len()).ok()?;
    let mean = sample.iter().map(|r| f64::from(r.avg)).sum::<f64>() / f64::from(len);
    let mut counts: Vec<u32> = sample.iter().map(|r| r.count).collect();
    counts.sort_unstable();
    let mid = counts.len() / 2;
    let median_count = if counts.len() % 2 == 1 {
        f64::from(counts[mid])
    } else {
        (f64::from(counts[mid - 1]) + f64::from(counts[mid])) / 2.0
    };
    Some(RatingPrior { mean, median_count })
}

pub fn adjusted(obs: RatingObs, prior: RatingPrior) -> f64 {
    let n = f64::from(obs.count);
    (n * f64::from(obs.avg) + prior.median_count * prior.mean) / (n + prior.median_count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn few_perfect_reviews_do_not_beat_many_good_ones() {
        let few = RatingObs::new(5.0, 8).unwrap();
        let many = RatingObs::new(4.8, 40_000).unwrap();
        let mid = RatingObs::new(4.5, 1_200).unwrap();
        let p = prior(&[few, many, mid]).unwrap();
        assert!((p.median_count - 1_200.0).abs() < f64::EPSILON);
        assert!(adjusted(few, p) < adjusted(many, p));
    }

    #[test]
    fn rejects_unknown() {
        assert!(RatingObs::new(4.5, 0).is_none());
        assert!(RatingObs::new(f32::NAN, 10).is_none());
        assert!(RatingObs::new(5.5, 10).is_none());
        assert!(prior(&[]).is_none());
    }
}
