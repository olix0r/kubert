// Based on https://github.com/prometheus/procfs/blob/775997f46ff61807cd9980078b8fdfee847d0c2d/proc_netstat.go#L28.
//
// Copyright 2022 The Prometheus Authors
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::fs;
use std::io::{self, BufRead, BufReader};

#[derive(Debug, Default)]
pub struct TcpExt {
    pub syncookies_sent: Option<f64>,
    pub syncookies_recv: Option<f64>,
    pub syncookies_failed: Option<f64>,
    pub embryonic_rsts: Option<f64>,
    pub prune_called: Option<f64>,
    pub rcv_pruned: Option<f64>,
    pub ofo_pruned: Option<f64>,
    pub out_of_window_icmps: Option<f64>,
    pub lock_dropped_icmps: Option<f64>,
    pub arp_filter: Option<f64>,
    pub tw: Option<f64>,
    pub tw_recycled: Option<f64>,
    pub tw_killed: Option<f64>,
    pub paws_active: Option<f64>,
    pub paws_estab: Option<f64>,
    pub delayed_acks: Option<f64>,
    pub delayed_ack_locked: Option<f64>,
    pub delayed_ack_lost: Option<f64>,
    pub listen_overflows: Option<f64>,
    pub listen_drops: Option<f64>,
    pub tcphp_hits: Option<f64>,
    pub tcppure_acks: Option<f64>,
    pub tcphp_acks: Option<f64>,
    pub tcp_reno_recovery: Option<f64>,
    pub tcp_sack_recovery: Option<f64>,
    pub tcpsack_reneging: Option<f64>,
    pub tcpsack_reorder: Option<f64>,
    pub tcp_reno_reorder: Option<f64>,
    pub tcp_ts_reorder: Option<f64>,
    pub tcp_full_undo: Option<f64>,
    pub tcp_partial_undo: Option<f64>,
    pub tcp_dsack_undo: Option<f64>,
    pub tcp_loss_undo: Option<f64>,
    pub tcp_lost_retransmit: Option<f64>,
    pub tcp_reno_failures: Option<f64>,
    pub tcp_sack_failures: Option<f64>,
    pub tcp_loss_failures: Option<f64>,
    pub tcp_fast_retrans: Option<f64>,
    pub tcp_slow_start_retrans: Option<f64>,
    pub tcp_timeouts: Option<f64>,
    pub tcp_loss_probes: Option<f64>,
    pub tcp_loss_probe_recovery: Option<f64>,
    pub tcp_reno_recovery_fail: Option<f64>,
    pub tcp_sack_recovery_fail: Option<f64>,
    pub tcp_rcv_collapsed: Option<f64>,
    pub tcp_dsack_old_sent: Option<f64>,
    pub tcp_dsack_ofo_sent: Option<f64>,
    pub tcp_dsack_recv: Option<f64>,
    pub tcp_dsack_ofo_recv: Option<f64>,
    pub tcp_abort_on_data: Option<f64>,
    pub tcp_abort_on_close: Option<f64>,
    pub tcp_abort_on_memory: Option<f64>,
    pub tcp_abort_on_timeout: Option<f64>,
    pub tcp_abort_on_linger: Option<f64>,
    pub tcp_abort_failed: Option<f64>,
    pub tcp_memory_pressures: Option<f64>,
    pub tcp_memory_pressures_chrono: Option<f64>,
    pub tcpsack_discard: Option<f64>,
    pub tcp_dsack_ignored_old: Option<f64>,
    pub tcp_dsack_ignored_no_undo: Option<f64>,
    pub tcp_spurious_rtos: Option<f64>,
    pub tcp_md5_not_found: Option<f64>,
    pub tcp_md5_unexpected: Option<f64>,
    pub tcp_md5_failure: Option<f64>,
    pub tcp_sack_shifted: Option<f64>,
    pub tcp_sack_merged: Option<f64>,
    pub tcp_sack_shift_fallback: Option<f64>,
    pub tcp_backlog_drop: Option<f64>,
    pub pf_memalloc_drop: Option<f64>,
    pub tcp_min_ttl_drop: Option<f64>,
    pub tcp_defer_accept_drop: Option<f64>,
    pub ip_reverse_path_filter: Option<f64>,
    pub tcp_time_wait_overflow: Option<f64>,
    pub tcp_req_q_full_do_cookies: Option<f64>,
    pub tcp_req_q_full_drop: Option<f64>,
    pub tcp_retrans_fail: Option<f64>,
    pub tcp_rcv_coalesce: Option<f64>,
    pub tcp_rcv_q_drop: Option<f64>,
    pub tcp_ofo_queue: Option<f64>,
    pub tcp_ofo_drop: Option<f64>,
    pub tcp_ofo_merge: Option<f64>,
    pub tcp_challenge_ack: Option<f64>,
    pub tcp_syn_challenge: Option<f64>,
    pub tcp_fast_open_active: Option<f64>,
    pub tcp_fast_open_active_fail: Option<f64>,
    pub tcp_fast_open_passive: Option<f64>,
    pub tcp_fast_open_passive_fail: Option<f64>,
    pub tcp_fast_open_listen_overflow: Option<f64>,
    pub tcp_fast_open_cookie_reqd: Option<f64>,
    pub tcp_fast_open_blackhole: Option<f64>,
    pub tcp_spurious_rtx_host_queues: Option<f64>,
    pub busy_poll_rx_packets: Option<f64>,
    pub tcp_auto_corking: Option<f64>,
    pub tcp_from_zero_window_adv: Option<f64>,
    pub tcp_to_zero_window_adv: Option<f64>,
    pub tcp_want_zero_window_adv: Option<f64>,
    pub tcp_syn_retrans: Option<f64>,
    pub tcp_orig_data_sent: Option<f64>,
    pub tcp_hystart_train_detect: Option<f64>,
    pub tcp_hystart_train_cwnd: Option<f64>,
    pub tcp_hystart_delay_detect: Option<f64>,
    pub tcp_hystart_delay_cwnd: Option<f64>,
    pub tcp_ack_skipped_syn_recv: Option<f64>,
    pub tcp_ack_skipped_paws: Option<f64>,
    pub tcp_ack_skipped_seq: Option<f64>,
    pub tcp_ack_skipped_fin_wait2: Option<f64>,
    pub tcp_ack_skipped_time_wait: Option<f64>,
    pub tcp_ack_skipped_challenge: Option<f64>,
    pub tcp_win_probe: Option<f64>,
    pub tcp_keep_alive: Option<f64>,
    pub tcp_mtup_fail: Option<f64>,
    pub tcp_mtup_success: Option<f64>,
    pub tcp_wqueue_too_big: Option<f64>,
}

#[derive(Debug, Default)]
pub struct IpExt {
    pub in_no_routes: Option<f64>,
    pub in_truncated_pkts: Option<f64>,
    pub in_mcast_pkts: Option<f64>,
    pub out_mcast_pkts: Option<f64>,
    pub in_bcast_pkts: Option<f64>,
    pub out_bcast_pkts: Option<f64>,
    pub in_octets: Option<f64>,
    pub out_octets: Option<f64>,
    pub in_mcast_octets: Option<f64>,
    pub out_mcast_octets: Option<f64>,
    pub in_bcast_octets: Option<f64>,
    pub out_bcast_octets: Option<f64>,
    pub in_csum_errors: Option<f64>,
    pub in_no_ect_pkts: Option<f64>,
    pub in_ect1_pkts: Option<f64>,
    pub in_ect0_pkts: Option<f64>,
    pub in_ce_pkts: Option<f64>,
    pub reasm_overlaps: Option<f64>,
}

#[derive(Debug, Default)]
pub struct ProcNetstat {
    pub pid: i32,
    pub tcp_ext: TcpExt,
    pub ip_ext: IpExt,
}

impl ProcNetstat {
    /// Reads the /proc/<pid>/net/netstat file and returns a ProcNetstat structure.
    pub fn read(pid: i32) -> io::Result<ProcNetstat> {
        let filename = format!("/proc/{pid}/net/netstat");
        let mut proc_netstat = read_from_file(&filename)?;
        proc_netstat.pid = pid;
        Ok(proc_netstat)
    }
}

/// Reads a netstat file from the given path and parses it.
fn read_from_file(path: &str) -> io::Result<ProcNetstat> {
    let data = fs::read(path)?;
    parse_proc_netstat(&data[..], path)
}

/// Parses the metrics from a /proc/<pid>/net/netstat file and returns a ProcNetstat structure.
/// The file is expected to consist of pairs of lines, one header and one value line.
fn parse_proc_netstat<R: io::Read>(reader: R, file_name: &str) -> io::Result<ProcNetstat> {
    let mut proc_netstat = ProcNetstat::default();
    let reader = BufReader::new(reader);
    let mut lines = reader.lines();

    while let Some(header_line) = lines.next() {
        let header = header_line?;
        let name_parts: Vec<&str> = header.split_whitespace().collect();

        let value_line = match lines.next() {
            Some(l) => l?,
            None => break,
        };
        let value_parts: Vec<&str> = value_line.split_whitespace().collect();

        if name_parts.len() != value_parts.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "mismatch field count in {}: {}",
                    file_name,
                    name_parts[0].trim_end_matches(':')
                ),
            ));
        }

        let protocol = name_parts[0].trim_end_matches(':');
        for i in 1..name_parts.len() {
            let value: f64 = value_parts[i].parse().map_err(|e| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("invalid value in {file_name}: {e}"),
                )
            })?;
            let key = name_parts[i];
            match protocol {
                "TcpExt" => match key {
                    "SyncookiesSent" => proc_netstat.tcp_ext.syncookies_sent = Some(value),
                    "SyncookiesRecv" => proc_netstat.tcp_ext.syncookies_recv = Some(value),
                    "SyncookiesFailed" => proc_netstat.tcp_ext.syncookies_failed = Some(value),
                    "EmbryonicRsts" => proc_netstat.tcp_ext.embryonic_rsts = Some(value),
                    "PruneCalled" => proc_netstat.tcp_ext.prune_called = Some(value),
                    "RcvPruned" => proc_netstat.tcp_ext.rcv_pruned = Some(value),
                    "OfoPruned" => proc_netstat.tcp_ext.ofo_pruned = Some(value),
                    "OutOfWindowIcmps" => proc_netstat.tcp_ext.out_of_window_icmps = Some(value),
                    "LockDroppedIcmps" => proc_netstat.tcp_ext.lock_dropped_icmps = Some(value),
                    "ArpFilter" => proc_netstat.tcp_ext.arp_filter = Some(value),
                    "TW" => proc_netstat.tcp_ext.tw = Some(value),
                    "TWRecycled" => proc_netstat.tcp_ext.tw_recycled = Some(value),
                    "TWKilled" => proc_netstat.tcp_ext.tw_killed = Some(value),
                    "PAWSActive" => proc_netstat.tcp_ext.paws_active = Some(value),
                    "PAWSEstab" => proc_netstat.tcp_ext.paws_estab = Some(value),
                    "DelayedACKs" => proc_netstat.tcp_ext.delayed_acks = Some(value),
                    "DelayedACKLocked" => proc_netstat.tcp_ext.delayed_ack_locked = Some(value),
                    "DelayedACKLost" => proc_netstat.tcp_ext.delayed_ack_lost = Some(value),
                    "ListenOverflows" => proc_netstat.tcp_ext.listen_overflows = Some(value),
                    "ListenDrops" => proc_netstat.tcp_ext.listen_drops = Some(value),
                    "TCPHPHits" => proc_netstat.tcp_ext.tcphp_hits = Some(value),
                    "TCPPureAcks" => proc_netstat.tcp_ext.tcppure_acks = Some(value),
                    "TCPHPAcks" => proc_netstat.tcp_ext.tcphp_acks = Some(value),
                    "TCPRenoRecovery" => proc_netstat.tcp_ext.tcp_reno_recovery = Some(value),
                    "TCPSackRecovery" => proc_netstat.tcp_ext.tcp_sack_recovery = Some(value),
                    "TCPSACKReneging" => proc_netstat.tcp_ext.tcpsack_reneging = Some(value),
                    "TCPSACKReorder" => proc_netstat.tcp_ext.tcpsack_reorder = Some(value),
                    "TCPRenoReorder" => proc_netstat.tcp_ext.tcp_reno_reorder = Some(value),
                    "TCPTSReorder" => proc_netstat.tcp_ext.tcp_ts_reorder = Some(value),
                    "TCPFullUndo" => proc_netstat.tcp_ext.tcp_full_undo = Some(value),
                    "TCPPartialUndo" => proc_netstat.tcp_ext.tcp_partial_undo = Some(value),
                    "TCPDSACKUndo" => proc_netstat.tcp_ext.tcp_dsack_undo = Some(value),
                    "TCPLossUndo" => proc_netstat.tcp_ext.tcp_loss_undo = Some(value),
                    "TCPLostRetransmit" => proc_netstat.tcp_ext.tcp_lost_retransmit = Some(value),
                    "TCPRenoFailures" => proc_netstat.tcp_ext.tcp_reno_failures = Some(value),
                    "TCPSackFailures" => proc_netstat.tcp_ext.tcp_sack_failures = Some(value),
                    "TCPLossFailures" => proc_netstat.tcp_ext.tcp_loss_failures = Some(value),
                    "TCPFastRetrans" => proc_netstat.tcp_ext.tcp_fast_retrans = Some(value),
                    "TCPSlowStartRetrans" => {
                        proc_netstat.tcp_ext.tcp_slow_start_retrans = Some(value)
                    }
                    "TCPTimeouts" => proc_netstat.tcp_ext.tcp_timeouts = Some(value),
                    "TCPLossProbes" => proc_netstat.tcp_ext.tcp_loss_probes = Some(value),
                    "TCPLossProbeRecovery" => {
                        proc_netstat.tcp_ext.tcp_loss_probe_recovery = Some(value)
                    }
                    "TCPRenoRecoveryFail" => {
                        proc_netstat.tcp_ext.tcp_reno_recovery_fail = Some(value)
                    }
                    "TCPSackRecoveryFail" => {
                        proc_netstat.tcp_ext.tcp_sack_recovery_fail = Some(value)
                    }
                    "TCPRcvCollapsed" => proc_netstat.tcp_ext.tcp_rcv_collapsed = Some(value),
                    "TCPDSACKOldSent" => proc_netstat.tcp_ext.tcp_dsack_old_sent = Some(value),
                    "TCPDSACKOfoSent" => proc_netstat.tcp_ext.tcp_dsack_ofo_sent = Some(value),
                    "TCPDSACKRecv" => proc_netstat.tcp_ext.tcp_dsack_recv = Some(value),
                    "TCPDSACKOfoRecv" => proc_netstat.tcp_ext.tcp_dsack_ofo_recv = Some(value),
                    "TCPAbortOnData" => proc_netstat.tcp_ext.tcp_abort_on_data = Some(value),
                    "TCPAbortOnClose" => proc_netstat.tcp_ext.tcp_abort_on_close = Some(value),
                    "TCPAbortOnMemory" => proc_netstat.tcp_ext.tcp_abort_on_memory = Some(value),
                    "TCPAbortOnTimeout" => proc_netstat.tcp_ext.tcp_abort_on_timeout = Some(value),
                    "TCPAbortOnLinger" => proc_netstat.tcp_ext.tcp_abort_on_linger = Some(value),
                    "TCPAbortFailed" => proc_netstat.tcp_ext.tcp_abort_failed = Some(value),
                    "TCPMemoryPressures" => proc_netstat.tcp_ext.tcp_memory_pressures = Some(value),
                    "TCPMemoryPressuresChrono" => {
                        proc_netstat.tcp_ext.tcp_memory_pressures_chrono = Some(value)
                    }
                    "TCPSackDiscard" => proc_netstat.tcp_ext.tcpsack_discard = Some(value),
                    "TCPDSACKIgnoredOld" => {
                        proc_netstat.tcp_ext.tcp_dsack_ignored_old = Some(value)
                    }
                    "TCPDSACKIgnoredNoUndo" => {
                        proc_netstat.tcp_ext.tcp_dsack_ignored_no_undo = Some(value)
                    }
                    "TCPSpuriousRTOs" => proc_netstat.tcp_ext.tcp_spurious_rtos = Some(value),
                    "TCPMD5NotFound" => proc_netstat.tcp_ext.tcp_md5_not_found = Some(value),
                    "TCPMD5Unexpected" => proc_netstat.tcp_ext.tcp_md5_unexpected = Some(value),
                    "TCPMD5Failure" => proc_netstat.tcp_ext.tcp_md5_failure = Some(value),
                    "TCPSackShifted" => proc_netstat.tcp_ext.tcp_sack_shifted = Some(value),
                    "TCPSackMerged" => proc_netstat.tcp_ext.tcp_sack_merged = Some(value),
                    "TCPSackShiftFallback" => {
                        proc_netstat.tcp_ext.tcp_sack_shift_fallback = Some(value)
                    }
                    "TCPBacklogDrop" => proc_netstat.tcp_ext.tcp_backlog_drop = Some(value),
                    "PFMemallocDrop" => proc_netstat.tcp_ext.pf_memalloc_drop = Some(value),
                    "TCPMinTTLDrop" => proc_netstat.tcp_ext.tcp_min_ttl_drop = Some(value),
                    "TCPDeferAcceptDrop" => {
                        proc_netstat.tcp_ext.tcp_defer_accept_drop = Some(value)
                    }
                    "IPReversePathFilter" => {
                        proc_netstat.tcp_ext.ip_reverse_path_filter = Some(value)
                    }
                    "TCPTimeWaitOverflow" => {
                        proc_netstat.tcp_ext.tcp_time_wait_overflow = Some(value)
                    }
                    "TCPReqQFullDoCookies" => {
                        proc_netstat.tcp_ext.tcp_req_q_full_do_cookies = Some(value)
                    }
                    "TCPReqQFullDrop" => proc_netstat.tcp_ext.tcp_req_q_full_drop = Some(value),
                    "TCPRetransFail" => proc_netstat.tcp_ext.tcp_retrans_fail = Some(value),
                    "TCPRcvCoalesce" => proc_netstat.tcp_ext.tcp_rcv_coalesce = Some(value),
                    "TCPRcvQDrop" => proc_netstat.tcp_ext.tcp_rcv_q_drop = Some(value),
                    "TCPOFOQueue" => proc_netstat.tcp_ext.tcp_ofo_queue = Some(value),
                    "TCPOFODrop" => proc_netstat.tcp_ext.tcp_ofo_drop = Some(value),
                    "TCPOFOMerge" => proc_netstat.tcp_ext.tcp_ofo_merge = Some(value),
                    "TCPChallengeACK" => proc_netstat.tcp_ext.tcp_challenge_ack = Some(value),
                    "TCPSYNChallenge" => proc_netstat.tcp_ext.tcp_syn_challenge = Some(value),
                    "TCPFastOpenActive" => proc_netstat.tcp_ext.tcp_fast_open_active = Some(value),
                    "TCPFastOpenActiveFail" => {
                        proc_netstat.tcp_ext.tcp_fast_open_active_fail = Some(value)
                    }
                    "TCPFastOpenPassive" => {
                        proc_netstat.tcp_ext.tcp_fast_open_passive = Some(value)
                    }
                    "TCPFastOpenPassiveFail" => {
                        proc_netstat.tcp_ext.tcp_fast_open_passive_fail = Some(value)
                    }
                    "TCPFastOpenListenOverflow" => {
                        proc_netstat.tcp_ext.tcp_fast_open_listen_overflow = Some(value)
                    }
                    "TCPFastOpenCookieReqd" => {
                        proc_netstat.tcp_ext.tcp_fast_open_cookie_reqd = Some(value)
                    }
                    "TCPFastOpenBlackhole" => {
                        proc_netstat.tcp_ext.tcp_fast_open_blackhole = Some(value)
                    }
                    "TCPSpuriousRtxHostQueues" => {
                        proc_netstat.tcp_ext.tcp_spurious_rtx_host_queues = Some(value)
                    }
                    "BusyPollRxPackets" => proc_netstat.tcp_ext.busy_poll_rx_packets = Some(value),
                    "TCPAutoCorking" => proc_netstat.tcp_ext.tcp_auto_corking = Some(value),
                    "TCPFromZeroWindowAdv" => {
                        proc_netstat.tcp_ext.tcp_from_zero_window_adv = Some(value)
                    }
                    "TCPToZeroWindowAdv" => {
                        proc_netstat.tcp_ext.tcp_to_zero_window_adv = Some(value)
                    }
                    "TCPWantZeroWindowAdv" => {
                        proc_netstat.tcp_ext.tcp_want_zero_window_adv = Some(value)
                    }
                    "TCPSynRetrans" => proc_netstat.tcp_ext.tcp_syn_retrans = Some(value),
                    "TCPOrigDataSent" => proc_netstat.tcp_ext.tcp_orig_data_sent = Some(value),
                    "TCPHystartTrainDetect" => {
                        proc_netstat.tcp_ext.tcp_hystart_train_detect = Some(value)
                    }
                    "TCPHystartTrainCwnd" => {
                        proc_netstat.tcp_ext.tcp_hystart_train_cwnd = Some(value)
                    }
                    "TCPHystartDelayDetect" => {
                        proc_netstat.tcp_ext.tcp_hystart_delay_detect = Some(value)
                    }
                    "TCPHystartDelayCwnd" => {
                        proc_netstat.tcp_ext.tcp_hystart_delay_cwnd = Some(value)
                    }
                    "TCPACKSkippedSynRecv" => {
                        proc_netstat.tcp_ext.tcp_ack_skipped_syn_recv = Some(value)
                    }
                    "TCPACKSkippedPAWS" => proc_netstat.tcp_ext.tcp_ack_skipped_paws = Some(value),
                    "TCPACKSkippedSeq" => proc_netstat.tcp_ext.tcp_ack_skipped_seq = Some(value),
                    "TCPACKSkippedFinWait2" => {
                        proc_netstat.tcp_ext.tcp_ack_skipped_fin_wait2 = Some(value)
                    }
                    "TCPACKSkippedTimeWait" => {
                        proc_netstat.tcp_ext.tcp_ack_skipped_time_wait = Some(value)
                    }
                    "TCPACKSkippedChallenge" => {
                        proc_netstat.tcp_ext.tcp_ack_skipped_challenge = Some(value)
                    }
                    "TCPWinProbe" => proc_netstat.tcp_ext.tcp_win_probe = Some(value),
                    "TCPKeepAlive" => proc_netstat.tcp_ext.tcp_keep_alive = Some(value),
                    "TCPMTUPFail" => proc_netstat.tcp_ext.tcp_mtup_fail = Some(value),
                    "TCPMTUPSuccess" => proc_netstat.tcp_ext.tcp_mtup_success = Some(value),
                    "TCPWqueueTooBig" => proc_netstat.tcp_ext.tcp_wqueue_too_big = Some(value),
                    _ => {}
                },
                "IpExt" => match key {
                    "InNoRoutes" => proc_netstat.ip_ext.in_no_routes = Some(value),
                    "InTruncatedPkts" => proc_netstat.ip_ext.in_truncated_pkts = Some(value),
                    "InMcastPkts" => proc_netstat.ip_ext.in_mcast_pkts = Some(value),
                    "OutMcastPkts" => proc_netstat.ip_ext.out_mcast_pkts = Some(value),
                    "InBcastPkts" => proc_netstat.ip_ext.in_bcast_pkts = Some(value),
                    "OutBcastPkts" => proc_netstat.ip_ext.out_bcast_pkts = Some(value),
                    "InOctets" => proc_netstat.ip_ext.in_octets = Some(value),
                    "OutOctets" => proc_netstat.ip_ext.out_octets = Some(value),
                    "InMcastOctets" => proc_netstat.ip_ext.in_mcast_octets = Some(value),
                    "OutMcastOctets" => proc_netstat.ip_ext.out_mcast_octets = Some(value),
                    "InBcastOctets" => proc_netstat.ip_ext.in_bcast_octets = Some(value),
                    "OutBcastOctets" => proc_netstat.ip_ext.out_bcast_octets = Some(value),
                    "InCsumErrors" => proc_netstat.ip_ext.in_csum_errors = Some(value),
                    "InNoECTPkts" => proc_netstat.ip_ext.in_no_ect_pkts = Some(value),
                    "InECT1Pkts" => proc_netstat.ip_ext.in_ect1_pkts = Some(value),
                    "InECT0Pkts" => proc_netstat.ip_ext.in_ect0_pkts = Some(value),
                    "InCEPkts" => proc_netstat.ip_ext.in_ce_pkts = Some(value),
                    "ReasmOverlaps" => proc_netstat.ip_ext.reasm_overlaps = Some(value),
                    _ => {}
                },
                _ => {}
            }
        }
    }
    Ok(proc_netstat)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_proc_netstat() {
        let input = b"TcpExt SyncookiesSent SyncookiesRecv\nTcpExt 1 2\nIpExt InOctets OutOctets\nIpExt 3 4\n";
        let ps = parse_proc_netstat(&input[..], "dummy").unwrap();
        assert_eq!(ps.tcp_ext.syncookies_sent, Some(1.0));
        assert_eq!(ps.tcp_ext.syncookies_recv, Some(2.0));
        assert_eq!(ps.ip_ext.in_octets, Some(3.0));
        assert_eq!(ps.ip_ext.out_octets, Some(4.0));
    }

    #[test]
    fn test_parse_proc_netstat_with_multiple_metrics() {
        let input = b"TcpExt SyncookiesSent SyncookiesRecv TCPSackRecovery TCPRenoRecovery\n\
                      TcpExt 10 20 30 40\n\
                      IpExt InNoRoutes InTruncatedPkts InMcastPkts\n\
                      IpExt 50 60 70\n";

        let ps = parse_proc_netstat(&input[..], "test_file").unwrap();

        // Check TcpExt metrics
        assert_eq!(ps.tcp_ext.syncookies_sent, Some(10.0));
        assert_eq!(ps.tcp_ext.syncookies_recv, Some(20.0));
        assert_eq!(ps.tcp_ext.tcp_sack_recovery, Some(30.0));
        assert_eq!(ps.tcp_ext.tcp_reno_recovery, Some(40.0));

        // Check IpExt metrics
        assert_eq!(ps.ip_ext.in_no_routes, Some(50.0));
        assert_eq!(ps.ip_ext.in_truncated_pkts, Some(60.0));
        assert_eq!(ps.ip_ext.in_mcast_pkts, Some(70.0));
    }

    #[test]
    fn test_parse_proc_netstat_network_bytes() {
        // Test specifically for network traffic byte metrics (InOctets and OutOctets)
        let input =
            b"IpExt InOctets OutOctets InMcastOctets OutMcastOctets InBcastOctets OutBcastOctets\n\
                      IpExt 1000000 2000000 3000 4000 5000 6000\n";

        let ps = parse_proc_netstat(&input[..], "network_bytes_file").unwrap();

        // Verify that octets metrics are correctly parsed
        assert_eq!(ps.ip_ext.in_octets, Some(1000000.0));
        assert_eq!(ps.ip_ext.out_octets, Some(2000000.0));
        assert_eq!(ps.ip_ext.in_mcast_octets, Some(3000.0));
        assert_eq!(ps.ip_ext.out_mcast_octets, Some(4000.0));
        assert_eq!(ps.ip_ext.in_bcast_octets, Some(5000.0));
        assert_eq!(ps.ip_ext.out_bcast_octets, Some(6000.0));
    }

    #[test]
    fn test_parse_proc_netstat_full_ip_metrics() {
        // Test with a more comprehensive set of IP metrics including octets
        let input = b"IpExt InNoRoutes InTruncatedPkts InOctets OutOctets InCsumErrors\n\
                      IpExt 10 20 12345678 87654321 30\n";

        let ps = parse_proc_netstat(&input[..], "ip_metrics_file").unwrap();

        // Verify all metrics parsed correctly
        assert_eq!(ps.ip_ext.in_no_routes, Some(10.0));
        assert_eq!(ps.ip_ext.in_truncated_pkts, Some(20.0));
        assert_eq!(ps.ip_ext.in_octets, Some(12345678.0));
        assert_eq!(ps.ip_ext.out_octets, Some(87654321.0));
        assert_eq!(ps.ip_ext.in_csum_errors, Some(30.0));

        // These should be None since they weren't in the input
        assert_eq!(ps.ip_ext.in_mcast_octets, None);
        assert_eq!(ps.ip_ext.out_mcast_octets, None);
    }

    #[test]
    fn test_parse_proc_netstat_mismatch_fields() {
        let input = b"TcpExt SyncookiesSent SyncookiesRecv\nTcpExt 1\n";
        let result = parse_proc_netstat(&input[..], "mismatch_file");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_proc_netstat_empty() {
        let input = b"";
        let ps = parse_proc_netstat(&input[..], "empty_file").unwrap();
        assert_eq!(ps.pid, 0); // Default value
        assert_eq!(ps.tcp_ext.syncookies_sent, None);
    }

    #[test]
    fn test_parse_proc_netstat_invalid_value() {
        let input = b"TcpExt SyncookiesSent\nTcpExt invalid\n";
        let result = parse_proc_netstat(&input[..], "invalid_value_file");
        assert!(result.is_err());
    }

    // Replicates https://github.com/prometheus/procfs/blob/775997f46ff61807cd9980078b8fdfee847d0c2d/proc_netstat_test.go
    #[test]
    fn test_upstream() {
        // Create test input with specific values that match the Go test
        let input = b"TcpExt: SyncookiesSent EmbryonicRsts TW PAWSEstab\n\
                      TcpExt: 0 1 83 3640\n\
                      IpExt: InNoRoutes InMcastPkts OutMcastPkts InOctets OutOctets\n\
                      IpExt: 0 208 214 123456 654321\n";

        let mut ps = parse_proc_netstat(&input[..], "test_specific_values").unwrap();
        ps.pid = 26231; // Set PID explicitly to match the Go test

        // Create a vector of test cases similar to the Go test
        struct TestCase {
            name: &'static str,
            want: f64,
            have: f64,
        }

        let test_cases = [
            TestCase {
                name: "pid",
                want: 26231.0,
                have: ps.pid as f64,
            },
            TestCase {
                name: "TcpExt:SyncookiesSent",
                want: 0.0,
                have: ps.tcp_ext.syncookies_sent.unwrap(),
            },
            TestCase {
                name: "TcpExt:EmbryonicRsts",
                want: 1.0,
                have: ps.tcp_ext.embryonic_rsts.unwrap(),
            },
            TestCase {
                name: "TcpExt:TW",
                want: 83.0,
                have: ps.tcp_ext.tw.unwrap(),
            },
            TestCase {
                name: "TcpExt:PAWSEstab",
                want: 3640.0,
                have: ps.tcp_ext.paws_estab.unwrap(),
            },
            TestCase {
                name: "IpExt:InNoRoutes",
                want: 0.0,
                have: ps.ip_ext.in_no_routes.unwrap(),
            },
            TestCase {
                name: "IpExt:InMcastPkts",
                want: 208.0,
                have: ps.ip_ext.in_mcast_pkts.unwrap(),
            },
            TestCase {
                name: "IpExt:OutMcastPkts",
                want: 214.0,
                have: ps.ip_ext.out_mcast_pkts.unwrap(),
            },
            // Also test the network bytes metrics which we're particularly interested in
            TestCase {
                name: "IpExt:InOctets",
                want: 123456.0,
                have: ps.ip_ext.in_octets.unwrap(),
            },
            TestCase {
                name: "IpExt:OutOctets",
                want: 654321.0,
                have: ps.ip_ext.out_octets.unwrap(),
            },
        ];

        // Check each test case
        for test in &test_cases {
            assert_eq!(
                test.want, test.have,
                "For {}: expected {}, got {}",
                test.name, test.want, test.have
            );
        }
    }

    #[test]
    fn test_queue_values() {
        // Create test input that specifically tests queue-related metrics
        let input = b"TcpExt: TCPOFOQueue TCPOFODrop TCPOFOMerge TCPRcvQDrop TCPWqueueTooBig\n\
                      TcpExt: 100 20 30 15 5\n";

        let ps = parse_proc_netstat(&input[..], "queue_metrics").unwrap();

        // Test the queue-related metrics specifically
        assert_eq!(ps.tcp_ext.tcp_ofo_queue, Some(100.0));
        assert_eq!(ps.tcp_ext.tcp_ofo_drop, Some(20.0));
        assert_eq!(ps.tcp_ext.tcp_ofo_merge, Some(30.0));
        assert_eq!(ps.tcp_ext.tcp_rcv_q_drop, Some(15.0));
        assert_eq!(ps.tcp_ext.tcp_wqueue_too_big, Some(5.0));
    }
}
