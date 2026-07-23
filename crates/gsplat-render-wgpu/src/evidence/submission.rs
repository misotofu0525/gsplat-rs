use crate::api::{SurfaceGpuOrderProducer, SurfaceOrderBackendUsed, SurfaceProjectedDrawExecution};

/// Why a requested order measurement did not reserve a ticket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceOrderMeasurementUnsampledReason {
    /// Every non-blocking telemetry slot was still owned by earlier work.
    RingBusy,
    /// The platform Surface did not provide a drawable for this frame.
    SurfaceUnavailable,
}

/// Exact submission identity for the optional order measurement on a frame.
/// Only `Issued` creates a ticket that must later receive one terminal success
/// or failure receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SurfaceOrderMeasurementSubmission {
    #[default]
    NotRequested,
    Issued {
        backend: SurfaceOrderBackendUsed,
        ticket: u64,
    },
    Unsampled {
        backend: SurfaceOrderBackendUsed,
        reason: SurfaceOrderMeasurementUnsampledReason,
    },
}

impl SurfaceOrderMeasurementSubmission {
    pub const fn backend(self) -> Option<SurfaceOrderBackendUsed> {
        match self {
            Self::NotRequested => None,
            Self::Issued { backend, .. } | Self::Unsampled { backend, .. } => Some(backend),
        }
    }

    pub const fn ticket(self) -> Option<u64> {
        match self {
            Self::Issued { ticket, .. } => Some(ticket),
            Self::NotRequested | Self::Unsampled { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceProjectedDrawMeasurementUnsampledReason {
    RingBusy,
    SurfaceUnavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SurfaceProjectedDrawMeasurementSubmission {
    #[default]
    NotRequested,
    Issued {
        execution: SurfaceProjectedDrawExecution,
        ticket: u64,
    },
    Unsampled {
        execution: SurfaceProjectedDrawExecution,
        reason: SurfaceProjectedDrawMeasurementUnsampledReason,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceGpuProducerMeasurementUnsampledReason {
    RingBusy,
    SurfaceUnavailable,
}

/// Ticket identity for the independent Packed GPU-producer A/B receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SurfaceGpuProducerMeasurementSubmission {
    #[default]
    NotRequested,
    Issued {
        producer: SurfaceGpuOrderProducer,
        ticket: u64,
    },
    Unsampled {
        producer: SurfaceGpuOrderProducer,
        reason: SurfaceGpuProducerMeasurementUnsampledReason,
    },
}

impl SurfaceGpuProducerMeasurementSubmission {
    pub const fn ticket(self) -> Option<u64> {
        match self {
            Self::Issued { ticket, .. } => Some(ticket),
            Self::NotRequested | Self::Unsampled { .. } => None,
        }
    }
}

impl SurfaceProjectedDrawMeasurementSubmission {
    pub const fn ticket(self) -> Option<u64> {
        match self {
            Self::Issued { ticket, .. } => Some(ticket),
            Self::NotRequested | Self::Unsampled { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        SurfaceGpuProducerMeasurementSubmission, SurfaceOrderMeasurementSubmission,
        SurfaceProjectedDrawMeasurementSubmission,
    };

    #[test]
    fn submission_identities_remain_available_at_compatibility_paths() {
        let _: crate::SurfaceOrderMeasurementSubmission =
            SurfaceOrderMeasurementSubmission::NotRequested;
        let _: crate::surface_session::SurfaceOrderMeasurementSubmission =
            SurfaceOrderMeasurementSubmission::NotRequested;
        let _: crate::SurfaceProjectedDrawMeasurementSubmission =
            SurfaceProjectedDrawMeasurementSubmission::NotRequested;
        let _: crate::surface_session::SurfaceProjectedDrawMeasurementSubmission =
            SurfaceProjectedDrawMeasurementSubmission::NotRequested;
        let _: crate::SurfaceGpuProducerMeasurementSubmission =
            SurfaceGpuProducerMeasurementSubmission::NotRequested;
        let _: crate::surface_session::SurfaceGpuProducerMeasurementSubmission =
            SurfaceGpuProducerMeasurementSubmission::NotRequested;
    }
}
