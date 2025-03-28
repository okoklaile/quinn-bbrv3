#[doc(hidden)]
#[derive(Debug, Clone)]
pub struct RecoveryConfig {
    /// Enable Datagram Packetization Layer Path MTU Discovery.
    pub enable_dplpmtud: bool,

    /// The maximum size of outgoing UDP payloads.
    pub max_datagram_size: usize,

    /// The maximum amount of time the endpoint intends to delay acknowledgments
    /// for packets in the Application Data packet number space.
    max_ack_delay: Duration,

    /// The maximum number of ack-eliciting packets the endpoint receives before
    /// sending an acknowledgment.
    ack_eliciting_threshold: u64,

    /// The congestion control algorithm used for a path.
    pub congestion_control_algorithm: CongestionControlAlgorithm,

    /// The minimal congestion window in packets.
    /// The RECOMMENDED value is 2 * max_datagram_size.
    /// See RFC 9002 Section 7.2
    pub min_congestion_window: u64,

    /// The initial congestion window in packets.
    /// Endpoints SHOULD use an initial congestion window of ten times the
    /// maximum datagram size (max_datagram_size), while limiting the window to
    /// the larger of 14,720 bytes or twice the maximum datagram size.
    /// See RFC 9002 Section 7.2
    pub initial_congestion_window: u64,

    /// The threshold for slow start in packets.
    pub slow_start_thresh: u64,

    /// The minimum duration for BBR ProbeRTT state
    pub bbr_probe_rtt_duration: Duration,

    /// Enable using a cwnd based on bdp during ProbeRTT state.
    pub bbr_probe_rtt_based_on_bdp: bool,

    /// The cwnd gain for BBR ProbeRTT state
    pub bbr_probe_rtt_cwnd_gain: f64,

    /// The length of the RTProp min filter window
    pub bbr_rtprop_filter_len: Duration,

    /// The cwnd gain for ProbeBW state
    pub bbr_probe_bw_cwnd_gain: f64,

    /// Delta in copa slow start state.
    pub copa_slow_start_delta: f64,

    /// Delta in coap steady state.
    pub copa_steady_delta: f64,

    /// Use rtt standing instead of latest rtt to calculate queueing delay
    pub copa_use_standing_rtt: bool,

    /// The initial rtt, used before real rtt is estimated.
    pub initial_rtt: Duration,

    /// Enable pacing to smooth the flow of packets sent onto the network.
    pub enable_pacing: bool,

    /// Clock granularity used by the pacer.
    pub pacing_granularity: Duration,

    /// Linear factor for calculating the probe timeout.
    pub pto_linear_factor: u64,

    /// Upper limit of probe timeout.
    pub max_pto: Duration,
}

impl Default for RecoveryConfig {
    fn default() -> RecoveryConfig {
        RecoveryConfig {
            enable_dplpmtud: true,
            max_datagram_size: DEFAULT_SEND_UDP_PAYLOAD_SIZE, // The upper limit is determined by DPLPMTUD
            max_ack_delay: time::Duration::from_millis(0),
            ack_eliciting_threshold: 2,
            congestion_control_algorithm: CongestionControlAlgorithm::Bbr,
            min_congestion_window: 2_u64,
            initial_congestion_window: 10_u64,
            slow_start_thresh: u64::MAX,
            bbr_probe_rtt_duration: Duration::from_millis(200),
            bbr_probe_rtt_based_on_bdp: false,
            bbr_probe_rtt_cwnd_gain: 0.75,
            bbr_rtprop_filter_len: Duration::from_secs(10),
            bbr_probe_bw_cwnd_gain: 2.0,
            copa_slow_start_delta: congestion_control::COPA_DELTA,
            copa_steady_delta: congestion_control::COPA_DELTA,
            copa_use_standing_rtt: true,
            initial_rtt: INITIAL_RTT,
            enable_pacing: true,
            pacing_granularity: time::Duration::from_millis(1),
            pto_linear_factor: DEFAULT_PTO_LINEAR_FACTOR,
            max_pto: MAX_PTO,
        }
    }
}