//! Common timing, full response validation and cleanup for both Rust runtimes.
use crate::{echo_capnp, Result, MEASUREMENT_VERSION};
use serde_json::json;
use std::time::{Duration, Instant};

pub(crate) trait Roundtrip {
    async fn roundtrip(&mut self, sequence: u64, payload: &[u8]) -> Result<()>;
}
impl Roundtrip for echo_capnp::echo::Client {
    async fn roundtrip(&mut self, sequence: u64, payload: &[u8]) -> Result<()> {
        let mut request = self.echo_request();
        request.get().set_sequence(sequence);
        request.get().set_payload(payload);
        let response = request.send().promise.await?;
        let response = response.get()?;
        if response.get_sequence() != sequence || response.get_payload()? != payload {
            return Err("Native response mismatch".into());
        }
        Ok(())
    }
}
pub(crate) async fn measure_calls(
    client: &mut impl Roundtrip,
    protocol: &str,
    payload: &[u8],
    warmup: u64,
    iterations: u64,
) -> Result<()> {
    for sequence in 0..warmup {
        tokio::time::timeout(Duration::from_secs(10), client.roundtrip(sequence, payload))
            .await??;
    }
    let mut samples = Vec::with_capacity(iterations as usize);
    let start = Instant::now();
    for sequence in warmup..warmup + iterations {
        let roundtrip = Instant::now();
        tokio::time::timeout(Duration::from_secs(10), client.roundtrip(sequence, payload))
            .await??;
        samples.push(u64::try_from(roundtrip.elapsed().as_nanos())?);
    }
    println!(
        "{}",
        json!({"measurement_version":MEASUREMENT_VERSION,"protocol":protocol,"security":crate::transport_metadata(protocol)?,"payload_bytes":payload.len(),"warmup":warmup,"iterations":iterations,"elapsed_ns":u64::try_from(start.elapsed().as_nanos())?,"latency_ns":samples})
    );
    Ok(())
}
