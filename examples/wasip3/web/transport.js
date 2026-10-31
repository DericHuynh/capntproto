// Browser TLS verification remains enabled. Each WIT resource owns its session.
export class Connection {
  constructor(session, stream) {
    this.session = session; this.reader = stream.readable.getReader(); this.writer = stream.writable.getWriter();
    this.pending = new Uint8Array(); this.offset = 0;
  }
  async read(maxLength) {
    if (!Number.isInteger(maxLength) || maxLength < 1 || maxLength > 16384) throw new Error('invalid read size');
    while (this.offset === this.pending.length) {
      const { value, done } = await this.reader.read();
      if (done) return new Uint8Array();
      this.pending = value; this.offset = 0;
    }
    const result = this.pending.slice(this.offset, this.offset + maxLength); this.offset += result.length; return result;
  }
  async write(bytes) { await this.writer.write(new Uint8Array(bytes)); }
  async close() { await (this.closing ??= this.writer.close()); }
  [Symbol.dispose]() { this.reader.cancel().catch(() => {}); this.writer.abort().catch(() => {}); this.session.close(); }
}
export async function connect(url, certificateSha256) {
  if (new URL(url).protocol !== 'https:') throw new Error('WebTransport requires HTTPS');
  if (certificateSha256.length !== 0 && certificateSha256.length !== 32) throw new Error('invalid SHA-256 pin');
  const options = certificateSha256.length ? { allowPooling: false, requireUnreliable: true, serverCertificateHashes: [{ algorithm: 'sha-256', value: new Uint8Array(certificateSha256) }] } : {};
  const session = new WebTransport(url, options); session.closed.catch(() => {});
  try { await session.ready; return new Connection(session, await session.createBidirectionalStream()); }
  catch (error) { session.close(); throw error; }
}
export const transport = { Connection, connect };
