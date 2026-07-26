//! Unpublished Direct/Paged Surface attempt state.
//!
//! A Surface attempt may prepare order and statistics before a drawable is
//! available. This owner keeps both values private until the Renderer facade
//! publishes them from the successful-present branch or discards them when a
//! scene/path transaction invalidates the attempt.

use gsplat_core::FrameStats;

#[derive(Debug, Default)]
pub(crate) struct SurfaceAttempt {
    order: Option<Vec<u32>>,
    stats: Option<FrameStats>,
}

impl SurfaceAttempt {
    pub(crate) const fn has_staged_order(&self) -> bool {
        self.order.is_some()
    }

    pub(crate) fn take_order(&mut self) -> Vec<u32> {
        self.order.take().unwrap_or_default()
    }

    pub(crate) fn stage_order(&mut self, order: Vec<u32>) {
        self.order = Some(order);
    }

    pub(crate) fn stage_order_recycling(&mut self, order: &mut Vec<u32>) {
        let mut staged = self.take_order();
        std::mem::swap(&mut staged, order);
        self.stage_order(staged);
    }

    pub(crate) fn order_or<'a>(&'a self, published: &'a [u32]) -> &'a [u32] {
        self.order.as_deref().unwrap_or(published)
    }

    pub(crate) fn stage_stats(&mut self, stats: FrameStats) {
        self.stats = Some(stats);
    }

    pub(crate) fn publish(
        &mut self,
        published_order: &mut Vec<u32>,
        published_stats: &mut FrameStats,
    ) {
        if let Some(mut order) = self.order.take() {
            std::mem::swap(published_order, &mut order);
        }
        if let Some(stats) = self.stats.take() {
            *published_stats = stats;
        }
    }

    pub(crate) fn discard(&mut self) {
        self.order = None;
        self.stats = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats(frame_ms: f32, visible_count: u32) -> FrameStats {
        FrameStats {
            frame_ms,
            preprocess_ms: frame_ms + 1.0,
            sort_ms: frame_ms + 2.0,
            raster_ms: frame_ms + 3.0,
            visible_count,
            drawn_count: visible_count,
        }
    }

    #[test]
    fn staged_order_and_stats_publish_together() {
        let mut attempt = SurfaceAttempt::default();
        let mut published_order = vec![7, 8];
        let mut published_stats = stats(1.0, 2);
        let staged_stats = stats(10.0, 3);

        attempt.stage_order(vec![2, 1, 0]);
        attempt.stage_stats(staged_stats);

        assert_eq!(attempt.order_or(&published_order), [2, 1, 0]);
        assert_eq!(published_order, [7, 8]);
        assert_eq!(published_stats, stats(1.0, 2));

        attempt.publish(&mut published_order, &mut published_stats);

        assert_eq!(published_order, [2, 1, 0]);
        assert_eq!(published_stats, staged_stats);
        assert!(!attempt.has_staged_order());
        assert_eq!(attempt.order_or(&published_order), published_order);
    }

    #[test]
    fn discard_preserves_published_snapshot() {
        let mut attempt = SurfaceAttempt::default();
        let mut published_order = vec![4, 5];
        let mut published_stats = stats(2.0, 2);

        attempt.stage_order(vec![1, 0]);
        attempt.stage_stats(stats(20.0, 2));
        attempt.discard();
        attempt.publish(&mut published_order, &mut published_stats);

        assert!(!attempt.has_staged_order());
        assert_eq!(attempt.order_or(&published_order), published_order);
        assert_eq!(published_stats, stats(2.0, 2));
    }

    #[test]
    fn recycling_returns_previous_staging_allocation() {
        let mut attempt = SurfaceAttempt::default();
        attempt.stage_order(vec![9, 8, 7]);
        let mut worker_order = vec![2, 0, 1];

        attempt.stage_order_recycling(&mut worker_order);

        assert_eq!(attempt.order_or(&[]), [2, 0, 1]);
        assert_eq!(worker_order, [9, 8, 7]);
    }
}
