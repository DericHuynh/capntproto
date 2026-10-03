#!/usr/bin/env python3
"""Test v1 in both directions against aioquic 1.3.0, including Retry and key updates.

Build first: cargo build --locked --example quic-interop
Run with an isolated Python environment containing aioquic==1.3.0.
"""
import asyncio
import datetime
import importlib.metadata
import pathlib
import sys
import tempfile

from aioquic.asyncio import connect, serve
from aioquic.asyncio.protocol import QuicConnectionProtocol
from aioquic.quic.configuration import QuicConfiguration
from aioquic.quic.events import HandshakeCompleted
from aioquic.quic.packet import QuicProtocolVersion
from cryptography import x509
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ec
from cryptography.x509.oid import NameOID

SIZE = 131_072
ALPN = "capntproto-rpc/1"


class Peer(QuicConnectionProtocol):
    def quic_event_received(self, event):
        if isinstance(event, HandshakeCompleted):
            assert event.alpn_protocol == ALPN
            assert not event.early_data_accepted
            assert not event.session_resumed
            assert self._quic._version == self._quic.configuration.supported_versions[0]
        super().quic_event_received(event)


async def exchange(reader, writer, client, update):
    for value in (1, 2):
        expected = bytes([value]) * SIZE
        if client:
            writer.write(expected)
            await writer.drain()
        assert await reader.readexactly(SIZE) == expected
        if not client:
            writer.write(expected)
            await writer.drain()
        update()
    if client:
        writer.write(b"done")
        await writer.drain()
    else:
        assert await reader.readexactly(4) == b"done"
    writer.write_eof()


async def main(binary):
    assert importlib.metadata.version("aioquic") == "1.3.0", "use aioquic==1.3.0"
    key = ec.generate_private_key(ec.SECP256R1())
    name = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, "localhost")])
    now = datetime.datetime.now(datetime.timezone.utc)
    cert = (x509.CertificateBuilder().subject_name(name).issuer_name(name)
            .public_key(key.public_key()).serial_number(x509.random_serial_number())
            .not_valid_before(now - datetime.timedelta(minutes=1))
            .not_valid_after(now + datetime.timedelta(days=1))
            .add_extension(x509.SubjectAlternativeName([x509.DNSName("localhost")]), False)
            .sign(key, hashes.SHA256()))
    with tempfile.TemporaryDirectory(prefix="quic-interop-") as directory:
        root = pathlib.Path(directory)
        (root / "cert.der").write_bytes(cert.public_bytes(serialization.Encoding.DER))
        (root / "key.der").write_bytes(key.private_bytes(serialization.Encoding.DER, serialization.PrivateFormat.PKCS8, serialization.NoEncryption()))
        for version, wire in [("1", QuicProtocolVersion.VERSION_1)]:
            for retry in (False, True):
                for rust_server in (False, True):
                    config = QuicConfiguration(is_client=rust_server, alpn_protocols=[ALPN], supported_versions=[wire], server_name="localhost")
                    config.cadata = cert.public_bytes(serialization.Encoding.PEM)
                    config.certificate = cert
                    config.private_key = key
                    server = None
                    tasks = []
                    protocols = []
                    if rust_server:
                        address = "127.0.0.1:0"
                    else:
                        def create_protocol(*args, **kwargs):
                            protocol = Peer(*args, **kwargs)
                            protocols.append(protocol)
                            return protocol

                        def handle_stream(reader, writer):
                            tasks.append(asyncio.create_task(exchange(reader, writer, False, protocols[-1].request_key_update)))

                        server = await serve("127.0.0.1", 0, configuration=config, create_protocol=create_protocol, stream_handler=handle_stream, retry=retry)
                        address = f"127.0.0.1:{server._transport.get_extra_info('sockname')[1]}"
                    process = await asyncio.create_subprocess_exec(
                        binary, "server" if rust_server else "client", address,
                        str(root / "cert.der"), str(root / "key.der"), version,
                        "retry" if retry else "direct", stdout=asyncio.subprocess.PIPE,
                    )
                    try:
                        if rust_server:
                            port = int(await asyncio.wait_for(process.stdout.readline(), 10))
                            async with connect("127.0.0.1", port, configuration=config, create_protocol=Peer) as protocol:
                                reader, writer = await protocol.create_stream()
                                await asyncio.wait_for(exchange(reader, writer, True, protocol.request_key_update), 15)
                                await asyncio.wait_for(protocol.wait_closed(), 10)
                        assert await asyncio.wait_for(process.wait(), 15) == 0
                        assert rust_server or tasks, "Rust client never opened a stream"
                        for task in tasks:
                            await task
                        print(f"PASS v{version} Rust {'server' if rust_server else 'client'} retry={retry}", flush=True)
                    finally:
                        if process.returncode is None:
                            process.kill()
                            await process.wait()
                        if server:
                            server.close()


if __name__ == "__main__":
    binary = str(pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else "target/debug/examples/quic-interop").resolve())
    asyncio.run(main(binary))
