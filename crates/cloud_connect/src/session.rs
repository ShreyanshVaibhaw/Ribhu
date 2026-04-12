//! Session timer, cost estimation, and idle detection for cloud instances.

use std::time::{Duration, Instant};

/// Tracks a single active cloud/SSH session.
pub struct SessionTracker {
    pub instance_name: String,
    pub hourly_cost: f64,
    started_at: Instant,
    last_activity: Instant,
    idle_threshold: Duration,
}

impl SessionTracker {
    pub fn new(instance_name: impl Into<String>, hourly_cost: f64) -> Self {
        let now = Instant::now();
        Self {
            instance_name: instance_name.into(),
            hourly_cost,
            started_at: now,
            last_activity: now,
            idle_threshold: Duration::from_secs(30 * 60),
        }
    }

    pub fn set_idle_threshold(&mut self, threshold: Duration) {
        self.idle_threshold = threshold;
    }

    pub fn record_activity(&mut self) {
        self.last_activity = Instant::now();
    }

    pub fn elapsed(&self) -> Duration {
        self.started_at.elapsed()
    }

    pub fn elapsed_display(&self) -> String {
        let elapsed = self.elapsed();
        let total_seconds = elapsed.as_secs();
        let hours = total_seconds / 3600;
        let minutes = (total_seconds % 3600) / 60;
        if hours > 0 {
            format!("{}h {}m", hours, minutes)
        } else {
            format!("{}m", minutes)
        }
    }

    pub fn estimated_cost(&self) -> f64 {
        let hours = self.elapsed().as_secs_f64() / 3600.0;
        self.hourly_cost * hours
    }

    pub fn cost_display(&self) -> String {
        format!("${:.2}", self.estimated_cost())
    }

    pub fn status_line(&self) -> String {
        format!(
            "{}: {} ({})",
            self.instance_name,
            self.elapsed_display(),
            self.cost_display()
        )
    }

    pub fn idle_duration(&self) -> Duration {
        self.last_activity.elapsed()
    }

    pub fn is_idle(&self) -> bool {
        self.idle_duration() >= self.idle_threshold
    }
}

/// Budget tracking for cloud spend.
pub struct BudgetTracker {
    pub daily_limit: Option<f64>,
    pub monthly_limit: Option<f64>,
    daily_spend: f64,
    monthly_spend: f64,
}

impl BudgetTracker {
    pub fn new() -> Self {
        Self {
            daily_limit: None,
            monthly_limit: None,
            daily_spend: 0.0,
            monthly_spend: 0.0,
        }
    }

    pub fn add_spend(&mut self, amount: f64) {
        self.daily_spend += amount;
        self.monthly_spend += amount;
    }

    pub fn daily_usage_percent(&self) -> Option<f64> {
        self.daily_limit
            .map(|limit| (self.daily_spend / limit) * 100.0)
    }

    pub fn monthly_usage_percent(&self) -> Option<f64> {
        self.monthly_limit
            .map(|limit| (self.monthly_spend / limit) * 100.0)
    }

    pub fn daily_warning(&self) -> Option<BudgetWarning> {
        self.daily_usage_percent().and_then(|percent| {
            if percent >= 100.0 {
                Some(BudgetWarning::Exceeded)
            } else if percent >= 80.0 {
                Some(BudgetWarning::NearLimit)
            } else {
                None
            }
        })
    }

    pub fn monthly_warning(&self) -> Option<BudgetWarning> {
        self.monthly_usage_percent().and_then(|percent| {
            if percent >= 100.0 {
                Some(BudgetWarning::Exceeded)
            } else if percent >= 80.0 {
                Some(BudgetWarning::NearLimit)
            } else {
                None
            }
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetWarning {
    NearLimit,
    Exceeded,
}

impl std::fmt::Display for BudgetWarning {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NearLimit => write!(formatter, "Approaching budget limit (80%)"),
            Self::Exceeded => write!(formatter, "Budget limit exceeded"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_display_formatting() {
        let session = SessionTracker::new("gpu-box-1", 1.10);
        let display = session.elapsed_display();
        assert!(display.contains('m'));
    }

    #[test]
    fn session_cost_starts_near_zero() {
        let session = SessionTracker::new("test", 3.99);
        assert!(session.estimated_cost() < 0.01);
    }

    #[test]
    fn budget_warnings() {
        let mut budget = BudgetTracker::new();
        budget.daily_limit = Some(10.0);
        assert!(budget.daily_warning().is_none());
        budget.add_spend(8.50);
        assert_eq!(budget.daily_warning(), Some(BudgetWarning::NearLimit));
        budget.add_spend(2.00);
        assert_eq!(budget.daily_warning(), Some(BudgetWarning::Exceeded));
    }

    #[test]
    fn idle_detection() {
        let mut session = SessionTracker::new("test", 1.0);
        session.set_idle_threshold(Duration::from_millis(10));
        session.record_activity();
        assert!(!session.is_idle());
        std::thread::sleep(Duration::from_millis(15));
        assert!(session.is_idle());
    }
}
