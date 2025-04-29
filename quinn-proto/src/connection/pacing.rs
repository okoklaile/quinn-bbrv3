//! Pacing of packet transmissions.

use std::time::{Duration, Instant};

use tracing::warn;

/// A simple token-bucket pacer
///
/// The pacer's capacity is derived on a fraction of the congestion window
/// which can be sent in regular intervals
/// Once the bucket is empty, further transmission is blocked.
/// The bucket refills at a rate slightly faster
/// than one congestion window per RTT, as recommended in
/// <https://tools.ietf.org/html/draft-ietf-quic-recovery-34#section-7.7>
pub(super) struct Pacer {
    capacity: u64,
    last_window: u64,
    last_mtu: u16,
    tokens: u64,
    prev: Instant,
}

impl Pacer {
    /// Obtains a new [`Pacer`].
    pub(super) fn new(smoothed_rtt: Duration, window: u64, mtu: u16, now: Instant) -> Self {
        let capacity = optimal_capacity(smoothed_rtt, window, mtu);
        Self {
            capacity,
            last_window: window,
            last_mtu: mtu,
            tokens: capacity,
            prev: now,
        }
    }

    /// Record that a packet has been transmitted.
    pub(super) fn on_transmit(&mut self, packet_length: u16) {
        self.tokens = self.tokens.saturating_sub(packet_length.into())
    }

    /// Return how long we need to wait before sending `bytes_to_send`
    ///
    /// If we can send a packet right away, this returns `None`. Otherwise, returns `Some(d)`,
    /// where `d` is the time before this function should be called again.
    ///
    /// The 5/4 ratio used here comes from the suggestion that N = 1.25 in the draft IETF RFC for
    /// QUIC.
    pub(super) fn delay(
        &mut self,
        smoothed_rtt: Duration,
        bytes_to_send: u64,
        mtu: u16,
        window: u64,
        now: Instant,
        pacing_rate: u64,
    ) -> Option<Instant> {
        if smoothed_rtt.is_zero() || window == 0 || pacing_rate == 0 {
            return None;
        }

        // Update capacity and tokens if necessary
        if window != self.last_window {
            optimal_capacity(smoothed_rtt,window,  mtu);
            self.tokens = self.capacity.min(self.tokens);   
            self.last_window = window;
        }

        // If tokens are enough, no need to wait and update
        if self.tokens >= bytes_to_send {
            return None;
        }

        // Tokens are refilled at the rate of pacing_rate
        let elapsed = now.saturating_duration_since(self.prev);
        self.tokens = self
            .tokens
            .saturating_add((pacing_rate as u128 * elapsed.as_nanos() / 1_000_000_000) as u64)
            .min(self.capacity);
        self.prev = now;

        if bytes_to_send <= self.tokens {
            return None;
        }

        // If tokens are not enough, calculate the time to wait for enough tokens.
        let time_to_wait =
            bytes_to_send.saturating_sub(self.tokens) * 1_000_000_000 / pacing_rate.max(1);
        Some(self.prev + Duration::from_nanos(time_to_wait))
    }
}
/// Calculates a pacer capacity for a certain window and RTT
///
/// The goal is to emit a burst (of size `capacity`) in timer intervals
/// which compromise between
/// - ideally distributing datagrams over time
/// - constantly waking up the connection to produce additional datagrams
///
/// Too short burst intervals means we will never meet them since the timer
/// accuracy in user-space is not high enough. If we miss the interval by more
/// than 25%, we will lose that part of the congestion window since no additional
/// tokens for the extra-elapsed time can be stored.
///
/// Too long burst intervals make pacing less effective.
    fn optimal_capacity(smoothed_rtt: Duration, window: u64, mtu: u16) -> u64 {
        let rtt = smoothed_rtt.as_nanos().max(1);

        let capacity = ((window as u128 * BURST_INTERVAL_NANOS) / rtt) as u64;

    // Small bursts are less efficient (no GSO), could increase latency and don't effectively
    // use the channel's buffer capacity. Large bursts might block the connection on sending.
        capacity.clamp(MIN_BURST_SIZE * mtu as u64, MAX_BURST_SIZE * mtu as u64)
    }

/// The burst interval
///
/// The capacity will we refilled in 4/5 of that time.
/// 2ms is chosen here since framework timers might have 1ms precision.
/// If kernel-level pacing is supported later a higher time here might be
/// more applicable.
const BURST_INTERVAL_NANOS: u128 = 2_000_000; // 2ms

/// Allows some usage of GSO, and doesn't slow down the handshake.
const MIN_BURST_SIZE: u64 = 10;

/// Creating 256 packets took 1ms in a benchmark, so larger bursts don't make sense.
const MAX_BURST_SIZE: u64 = 256;
