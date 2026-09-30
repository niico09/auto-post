//! Bookkeeping for `on_status` recovery attempts of a single step.

use std::collections::HashMap;

use super::error::RunError;
use crate::manifest::RecoveryPolicy;

/// Counts recovery attempts per HTTP status for one step execution, so each
/// status honours its own `max`.
#[derive(Debug, Default)]
pub(super) struct RecoveryTracker {
    attempts: HashMap<u16, u32>,
}

impl RecoveryTracker {
    /// Registers one more recovery for `status`. Fails once `policy.max`
    /// attempts were already used.
    pub(super) fn register(
        &mut self,
        request: &str,
        status: u16,
        policy: &RecoveryPolicy,
    ) -> Result<(), RunError> {
        let used = self.attempts.entry(status).or_insert(0);
        if *used >= policy.max {
            return Err(RunError::RecoveryExhausted {
                request: request.to_owned(),
                status,
                run: policy.run.clone(),
                max: policy.max,
            });
        }
        *used += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::RecoveryAction;

    fn policy(max: u32) -> RecoveryPolicy {
        RecoveryPolicy {
            run: "login".into(),
            then: RecoveryAction::Retry,
            max,
        }
    }

    #[test]
    fn allows_up_to_max_attempts_per_status() {
        let mut tracker = RecoveryTracker::default();
        assert!(tracker.register("r", 401, &policy(2)).is_ok());
        assert!(tracker.register("r", 401, &policy(2)).is_ok());
        assert!(matches!(
            tracker.register("r", 401, &policy(2)),
            Err(RunError::RecoveryExhausted {
                max: 2,
                status: 401,
                ..
            })
        ));
        // Another status has its own budget.
        assert!(tracker.register("r", 403, &policy(1)).is_ok());
    }

    #[test]
    fn zero_max_never_recovers() {
        let mut tracker = RecoveryTracker::default();
        assert!(tracker.register("r", 401, &policy(0)).is_err());
    }
}
