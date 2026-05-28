// IPC Trend — utility trend computation
//
// Computes whether utility scores are improving, stable, or declining
// based on a 5-point moving average comparison between recent and older values.

use std::collections::VecDeque;

/// Compute the trend direction from a history of utility scores.
///
/// Compares the average of the 5 most recent values against the average
/// of the 5 values before that. Returns:
/// - "improving" if recent average is >0.02 higher
/// - "declining" if recent average is >0.02 lower
/// - "stable" otherwise (or if insufficient history)
pub fn compute_trend(history: &VecDeque<f64>) -> &'static str {
    if history.len() < 3 {
        return "stable";
    }

    let recent: Vec<f64> = history.iter().rev().take(5).copied().collect();
    let avg_recent = recent.iter().sum::<f64>() / recent.len() as f64;

    let older: Vec<f64> = history.iter().rev().skip(5).take(5).copied().collect();
    if older.is_empty() {
        return "stable";
    }
    let avg_older = older.iter().sum::<f64>() / older.len() as f64;

    let diff = avg_recent - avg_older;
    if diff > 0.02 {
        "improving"
    } else if diff < -0.02 {
        "declining"
    } else {
        "stable"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_history_is_stable() {
        let history = VecDeque::new();
        assert_eq!(compute_trend(&history), "stable");
    }

    #[test]
    fn test_short_history_is_stable() {
        let history: VecDeque<f64> = vec![0.5, 0.6].into();
        assert_eq!(compute_trend(&history), "stable");
    }

    #[test]
    fn test_three_values_no_older_is_stable() {
        let history: VecDeque<f64> = vec![0.5, 0.6, 0.7].into();
        assert_eq!(compute_trend(&history), "stable");
    }

    #[test]
    fn test_improving_trend() {
        // Older values: 0.5, 0.5, 0.5, 0.5, 0.5
        // Recent values: 0.8, 0.8, 0.8, 0.8, 0.8
        let history: VecDeque<f64> = vec![
            0.5, 0.5, 0.5, 0.5, 0.5, 0.8, 0.8, 0.8, 0.8, 0.8,
        ]
        .into();
        assert_eq!(compute_trend(&history), "improving");
    }

    #[test]
    fn test_declining_trend() {
        // Older values: 0.8, 0.8, 0.8, 0.8, 0.8
        // Recent values: 0.5, 0.5, 0.5, 0.5, 0.5
        let history: VecDeque<f64> = vec![
            0.8, 0.8, 0.8, 0.8, 0.8, 0.5, 0.5, 0.5, 0.5, 0.5,
        ]
        .into();
        assert_eq!(compute_trend(&history), "declining");
    }

    #[test]
    fn test_stable_within_threshold() {
        // Difference is within 0.02 threshold
        let history: VecDeque<f64> = vec![
            0.50, 0.50, 0.50, 0.50, 0.50, 0.51, 0.51, 0.51, 0.51, 0.51,
        ]
        .into();
        assert_eq!(compute_trend(&history), "stable");
    }

    #[test]
    fn test_exactly_at_threshold_is_stable() {
        // avg_recent - avg_older = exactly 0.02 → not > 0.02, so stable
        let history: VecDeque<f64> = vec![
            0.50, 0.50, 0.50, 0.50, 0.50, 0.52, 0.52, 0.52, 0.52, 0.52,
        ]
        .into();
        assert_eq!(compute_trend(&history), "stable");
    }

    #[test]
    fn test_just_above_threshold_is_improving() {
        let history: VecDeque<f64> = vec![
            0.50, 0.50, 0.50, 0.50, 0.50, 0.53, 0.53, 0.53, 0.53, 0.53,
        ]
        .into();
        assert_eq!(compute_trend(&history), "improving");
    }

    #[test]
    fn test_partial_older_window() {
        // Only 3 older values available (history has 8 total, recent takes 5, older gets 3)
        let history: VecDeque<f64> = vec![0.3, 0.3, 0.3, 0.8, 0.8, 0.8, 0.8, 0.8].into();
        assert_eq!(compute_trend(&history), "improving");
    }
}
