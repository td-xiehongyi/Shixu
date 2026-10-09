/// Zero-based attempt, bounded jitter and total delay. No shift overflow.
pub fn next_retry(attempt: u32, jitter_ms: u32) -> u64 {
    let base = [1000_u64, 2000, 4000, 8000, 16000, 32000, 60000][attempt.min(6) as usize];
    (base + u64::from(jitter_ms.min(1000))).min(60000)
}
