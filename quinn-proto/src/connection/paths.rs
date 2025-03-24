use std::{cmp, net::SocketAddr};

use tracing::trace;

use super::{
    mtud::MtuDiscovery,
    pacing::Pacer,
    spaces::{PacketSpace, SentPacket},
};
use crate::{Duration, Instant, TIMER_GRANULARITY, TransportConfig, congestion, packet::SpaceId};

/// Description of a particular network path
pub(super) struct PathData {
    pub(super) remote: SocketAddr,
    pub(super) rtt: RttEstimator,
    /// Whether we're enabling ECN on outgoing packets
    pub(super) sending_ecn: bool,
    /// Congestion controller state
    pub(super) congestion: Box<dyn congestion::Controller>,
    /// Pacing state
    pub(super) pacing: Pacer,
    pub(super) challenge: Option<u64>,
    pub(super) challenge_pending: bool,
    /// Whether we're certain the peer can both send and receive on this address
    ///
    /// Initially equal to `use_stateless_retry` for servers, and becomes false again on every
    /// migration. Always true for clients.
    pub(super) validated: bool,
    /// Total size of all UDP datagrams sent on this path
    pub(super) total_sent: u64,
    /// Total size of all UDP datagrams received on this path
    pub(super) total_recvd: u64,
    /// The state of the MTU discovery process
    pub(super) mtud: MtuDiscovery,
    /// Packet number of the first packet sent after an RTT sample was collected on this path
    ///
    /// Used in persistent congestion determination.
    pub(super) first_packet_after_rtt_sample: Option<(SpaceId, u64)>,
    pub(super) in_flight: InFlight,
    /// Number of the first packet sent on this path
    ///
    /// Used to determine whether a packet was sent on an earlier path. Insufficient to determine if
    /// a packet was sent on a later path.
    first_packet: Option<u64>,
}

impl PathData {
    pub(super) fn new(
        remote: SocketAddr,
        allow_mtud: bool,
        peer_max_udp_payload_size: Option<u16>,
        now: Instant,
        config: &TransportConfig,
    ) -> Self {
        let congestion = config
            .congestion_controller_factory
            .clone()
            .build(now, config.get_initial_mtu());
        Self {
            remote,
            rtt: RttEstimator::new(),
            sending_ecn: true,
            pacing: Pacer::new(
                config.initial_rtt,
                congestion.initial_window(),
                config.get_initial_mtu(),
                now,
            ),
            congestion,
            challenge: None,
            challenge_pending: false,
            validated: false,
            total_sent: 0,
            total_recvd: 0,
            mtud: config
                .mtu_discovery_config
                .as_ref()
                .filter(|_| allow_mtud)
                .map_or(
                    MtuDiscovery::disabled(config.get_initial_mtu(), config.min_mtu),
                    |mtud_config| {
                        MtuDiscovery::new(
                            config.get_initial_mtu(),
                            config.min_mtu,
                            peer_max_udp_payload_size,
                            mtud_config.clone(),
                        )
                    },
                ),
            first_packet_after_rtt_sample: None,
            in_flight: InFlight::new(),
            first_packet: None,
        }
    }

    pub(super) fn from_previous(remote: SocketAddr, prev: &Self, now: Instant) -> Self {
        let congestion = prev.congestion.clone_box();
        let smoothed_rtt = prev.rtt.get();
        Self {
            remote,
            rtt: prev.rtt.clone(),
            pacing: Pacer::new(smoothed_rtt, congestion.window(), prev.current_mtu(), now),
            sending_ecn: true,
            congestion,
            challenge: None,
            challenge_pending: false,
            validated: false,
            total_sent: 0,
            total_recvd: 0,
            mtud: prev.mtud.clone(),
            first_packet_after_rtt_sample: prev.first_packet_after_rtt_sample,
            in_flight: InFlight::new(),
            first_packet: None,
        }
    }

    /// Resets RTT, congestion control and MTU states.
    ///
    /// This is useful when it is known the underlying path has changed.
    pub(super) fn reset(&mut self, now: Instant, config: &TransportConfig) {
        self.rtt = RttEstimator::new();
        self.congestion = config
            .congestion_controller_factory
            .clone()
            .build(now, config.get_initial_mtu());
        self.mtud.reset(config.get_initial_mtu(), config.min_mtu);
    }

    /// Indicates whether we're a server that hasn't validated the peer's address and hasn't
    /// received enough data from the peer to permit sending `bytes_to_send` additional bytes
    pub(super) fn anti_amplification_blocked(&self, bytes_to_send: u64) -> bool {
        !self.validated && self.total_recvd * 3 < self.total_sent + bytes_to_send
    }

    /// Returns the path's current MTU
    pub(super) fn current_mtu(&self) -> u16 {
        self.mtud.current_mtu()
    }

    /// Account for transmission of `packet` with number `pn` in `space`
    pub(super) fn sent(&mut self, pn: u64, packet: SentPacket, space: &mut PacketSpace) {
        self.in_flight.insert(&packet);
        if self.first_packet.is_none() {
            self.first_packet = Some(pn);
        }
        self.in_flight.bytes -= space.sent(pn, packet);
    }

    /// Remove `packet` with number `pn` from this path's congestion control counters, or return
    /// `false` if `pn` was sent before this path was established.
    pub(super) fn remove_in_flight(&mut self, pn: u64, packet: &SentPacket) -> bool {
        if self.first_packet.map_or(true, |first| first > pn) {
            return false;
        }
        self.in_flight.remove(packet);
        true
    }
}

/// RTT estimation for a particular network path
#[derive(Clone)]
pub struct RttEstimator {
    min_rtt: Duration,
    latest_rtt: Duration,
    smoothed_rtt: Duration,
    rtt_var: Duration,
    delivered: u64,      
    delivered_ce: u64,   
    is_ce: bool,        
}

impl RttEstimator {
    pub fn new() -> Self {
        Self {
            min_rtt: Duration::from_secs(0),
            latest_rtt: Duration::from_secs(0),
            smoothed_rtt: Duration::from_secs(0),
            rtt_var: Duration::from_secs(0),
            delivered: 0,
            delivered_ce: 0,
            is_ce: false,
        }
    }

    // 添加获取ECN状态的方法
    pub fn is_ce(&self) -> Option<bool> {
        if self.delivered == 0 {
            None
        } else {
            Some(self.is_ce)
        }
    }

    // 更新ECN状态
    pub fn update_ecn(&mut self, bytes: u64, is_ce: bool) {
        self.delivered += bytes;
        if is_ce {
            self.delivered_ce += bytes;
            self.is_ce = true;
        }
    }

    // 获取ECN标记率
    pub fn ce_ratio(&self) -> Option<f32> {
        if self.delivered == 0 {
            None
        } else {
            Some(self.delivered_ce as f32 / self.delivered as f32)
        }
    }

    // 现有方法保持不变
    pub fn min(&self) -> Duration {
        self.min_rtt
    }

    pub fn latest(&self) -> Duration {
        self.latest_rtt
    }

    pub fn smoothed(&self) -> Duration {
        self.smoothed_rtt  
    }

    pub fn var(&self) -> Duration {
        self.rtt_var
    }

    // 为 cubic.rs 添加 get() 方法
    pub fn get(&self) -> Duration {
        self.smoothed_rtt
    }

    // 恢复 conservative 方法
    pub fn conservative(&self) -> Duration {
        self.smoothed_rtt.max(self.latest_rtt)
    }

    // 恢复 pto_base 方法
    pub fn pto_base(&self) -> Duration {
        self.smoothed_rtt + cmp::max(4 * self.rtt_var, TIMER_GRANULARITY)
    }

    // 修改 update 方法签名以匹配调用
    pub fn update(&mut self, ack_delay: Duration, rtt: Duration) {
        self.latest_rtt = rtt;
        
        // 更新最小RTT (忽略 ack_delay)
        if self.min_rtt.is_zero() || rtt < self.min_rtt {
            self.min_rtt = rtt;
        }

        // 更新平滑RTT和RTT变化
        let adjusted_rtt = if self.min_rtt + ack_delay <= rtt {
            rtt - ack_delay
        } else {
            rtt
        };

        if self.smoothed_rtt.is_zero() {
            self.smoothed_rtt = adjusted_rtt;
            self.rtt_var = adjusted_rtt / 2;
        } else {
            let delta = if self.smoothed_rtt > adjusted_rtt {
                self.smoothed_rtt - adjusted_rtt
            } else {
                adjusted_rtt - self.smoothed_rtt
            };
            self.rtt_var = (3 * self.rtt_var + delta) / 4;
            self.smoothed_rtt = (7 * self.smoothed_rtt + adjusted_rtt) / 8;
        }

        // ECN 状态更新移到 update_ecn 方法中
    }
}

#[derive(Default)]
pub(crate) struct PathResponses {
    pending: Vec<PathResponse>,
}

impl PathResponses {
    pub(crate) fn push(&mut self, packet: u64, token: u64, remote: SocketAddr) {
        /// Arbitrary permissive limit to prevent abuse
        const MAX_PATH_RESPONSES: usize = 16;
        let response = PathResponse {
            packet,
            token,
            remote,
        };
        let existing = self.pending.iter_mut().find(|x| x.remote == remote);
        if let Some(existing) = existing {
            // Update a queued response
            if existing.packet <= packet {
                *existing = response;
            }
            return;
        }
        if self.pending.len() < MAX_PATH_RESPONSES {
            self.pending.push(response);
        } else {
            // We don't expect to ever hit this with well-behaved peers, so we don't bother dropping
            // older challenges.
            trace!("ignoring excessive PATH_CHALLENGE");
        }
    }

    pub(crate) fn pop_off_path(&mut self, remote: SocketAddr) -> Option<(u64, SocketAddr)> {
        let response = *self.pending.last()?;
        if response.remote == remote {
            // We don't bother searching further because we expect that the on-path response will
            // get drained in the immediate future by a call to `pop_on_path`
            return None;
        }
        self.pending.pop();
        Some((response.token, response.remote))
    }

    pub(crate) fn pop_on_path(&mut self, remote: SocketAddr) -> Option<u64> {
        let response = *self.pending.last()?;
        if response.remote != remote {
            // We don't bother searching further because we expect that the off-path response will
            // get drained in the immediate future by a call to `pop_off_path`
            return None;
        }
        self.pending.pop();
        Some(response.token)
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}

#[derive(Copy, Clone)]
struct PathResponse {
    /// The packet number the corresponding PATH_CHALLENGE was received in
    packet: u64,
    token: u64,
    /// The address the corresponding PATH_CHALLENGE was received from
    remote: SocketAddr,
}

/// Summary statistics of packets that have been sent on a particular path, but which have not yet
/// been acked or deemed lost
pub(super) struct InFlight {
    /// Sum of the sizes of all sent packets considered "in flight" by congestion control
    ///
    /// The size does not include IP or UDP overhead. Packets only containing ACK frames do not
    /// count towards this to ensure congestion control does not impede congestion feedback.
    pub(super) bytes: u64,
    /// Number of packets in flight containing frames other than ACK and PADDING
    ///
    /// This can be 0 even when bytes is not 0 because PADDING frames cause a packet to be
    /// considered "in flight" by congestion control. However, if this is nonzero, bytes will always
    /// also be nonzero.
    pub(super) ack_eliciting: u64,
}

impl InFlight {
    fn new() -> Self {
        Self {
            bytes: 0,
            ack_eliciting: 0,
        }
    }

    fn insert(&mut self, packet: &SentPacket) {
        self.bytes += u64::from(packet.size);
        self.ack_eliciting += u64::from(packet.ack_eliciting);
    }

    /// Update counters to account for a packet becoming acknowledged, lost, or abandoned
    fn remove(&mut self, packet: &SentPacket) {
        self.bytes -= u64::from(packet.size);
        self.ack_eliciting -= u64::from(packet.ack_eliciting);
    }
}
