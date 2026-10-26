//! Record-layer correctness: these are bounded data-integrity tests, not benchmarks.
use super::common::{Pki, TIMEOUT};
use capntproto::rpc::tls::TlsStream;
use std::{io, sync::Arc};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufStream};

async fn send(mut stream: impl AsyncWrite + Unpin, value: u8, size: usize) {
    stream.write_all(&vec![value; size]).await.unwrap();
    stream.shutdown().await.unwrap();
    // Repeated shutdown must not emit another close_notify or lose bytes.
    stream.shutdown().await.unwrap();
    assert_eq!(
        stream.write_u8(1).await.unwrap_err().kind(),
        io::ErrorKind::BrokenPipe
    );
}
async fn receive(mut stream: impl AsyncRead + Unpin, value: u8, size: usize) {
    // Cross record boundaries using both tiny and large application reads.
    let mut received = 0;
    let mut buffer = [0; 19_001];
    loop {
        let limit = if received % 2 == 0 { 7 } else { buffer.len() };
        let count = stream.read(&mut buffer[..limit]).await.unwrap();
        if count == 0 {
            break;
        }
        assert!(buffer[..count].iter().all(|byte| *byte == value));
        received += count;
    }
    assert_eq!(received, size);
    assert_eq!(stream.read(&mut buffer).await.unwrap(), 0);
}

#[tokio::test]
async fn simultaneous_fragmented_records_survive_backpressure_and_partial_reads() {
    tokio::time::timeout(TIMEOUT, async {
        let pki = Pki::new();
        let (client, server) = tokio::io::duplex(257);
        // These wrappers require poll_flush to publish even a complete handshake
        // flight. Their small buffers force short writes and Pending resumes.
        let client = BufStream::with_capacity(113, 127, client);
        let server = BufStream::with_capacity(79, 97, server);
        let (client, server) = tokio::join!(
            TlsStream::connect(client, pki.client(false), "localhost".try_into().unwrap()),
            TlsStream::accept(server, pki.server(false)),
        );
        let (cr, cw) = tokio::io::split(client.unwrap());
        let (sr, sw) = tokio::io::split(server.unwrap());
        // Separate tasks exercise read/write waker replacement by split halves.
        let cr = tokio::spawn(receive(cr, 0xa7, 300_001));
        let sr = tokio::spawn(receive(sr, 0x73, 400_007));
        let cw = tokio::spawn(send(cw, 0x73, 400_007));
        let sw = tokio::spawn(send(sw, 0xa7, 300_001));
        cr.await.unwrap();
        sr.await.unwrap();
        cw.await.unwrap();
        sw.await.unwrap();
    })
    .await
    .expect("fragmented full-duplex TLS stalled");
}

#[tokio::test]
async fn tiny_record_configuration_and_half_close_preserve_reverse_traffic() {
    tokio::time::timeout(TIMEOUT, async {
        let pki = Pki::new();
        let mut config = (*pki.client(false)).clone();
        config.max_fragment_size = Some(32);
        let (client, server) = tokio::io::duplex(8192);
        let (client, server) = tokio::join!(
            TlsStream::connect(client, Arc::new(config), "localhost".try_into().unwrap()),
            TlsStream::accept(server, pki.server(false)),
        );
        let (mut client, mut server) = (client.unwrap(), server.unwrap());
        tokio::join!(
            async {
                client.write_all(&vec![0x73; 150_003]).await.unwrap();
                client.shutdown().await.unwrap();
                receive(client, 0xa7, 50_007).await;
            },
            async {
                receive(&mut server, 0x73, 150_003).await;
                send(server, 0xa7, 50_007).await;
            },
        );
    })
    .await
    .expect("tiny-record TLS stalled");
}

#[tokio::test]
async fn unbuffered_client_interoperates_with_buffered_server_and_key_updates() {
    tokio::time::timeout(TIMEOUT, async {
        let pki = Pki::new();
        let (client, server) = tokio::io::duplex(37);
        let acceptor = tokio_rustls::TlsAcceptor::from(pki.reference_server());
        // Start application reads as soon as that side's handshake finishes.
        // A joint handshake barrier would prevent the client from draining the
        // server's post-handshake tickets through this 37-byte transport.
        let client = TlsStream::connect(client, pki.client(false), "localhost".try_into().unwrap());
        let server = acceptor.accept(server);
        tokio::join!(
            async {
                let mut client = client.await.unwrap();
                for _ in 0..4 {
                    assert_eq!(client.read_u32().await.unwrap(), 0x12345678);
                    client.write_u32(0x87654321).await.unwrap();
                    client.flush().await.unwrap();
                }
                client.shutdown().await.unwrap();
                assert_eq!(client.read(&mut [0; 1]).await.unwrap(), 0);
            },
            async {
                let mut server = server.await.unwrap();
                for _ in 0..4 {
                    server.get_mut().1.refresh_traffic_keys().unwrap();
                    server.write_u32(0x12345678).await.unwrap();
                    server.flush().await.unwrap();
                    assert_eq!(server.read_u32().await.unwrap(), 0x87654321);
                }
                assert_eq!(server.read(&mut [0; 1]).await.unwrap(), 0);
                server.shutdown().await.unwrap();
            },
        );
    })
    .await
    .expect("TLS key update stalled");
}

#[tokio::test]
async fn unbuffered_server_interoperates_with_buffered_client_and_rejects_truncation() {
    tokio::time::timeout(TIMEOUT, async {
        let pki = Pki::new();
        let (client, server) = tokio::io::duplex(8192);
        let connector = tokio_rustls::TlsConnector::from(pki.reference_client());
        let (client, server) = tokio::join!(
            connector.connect("localhost".try_into().unwrap(), client),
            TlsStream::accept(server, pki.server(false)),
        );
        let (mut client, mut server) = (client.unwrap(), server.unwrap());
        client.get_mut().1.refresh_traffic_keys().unwrap();
        client.write_all(b"authenticated").await.unwrap();
        client.flush().await.unwrap();
        let mut received = [0; 13];
        server.read_exact(&mut received).await.unwrap();
        assert_eq!(&received, b"authenticated");
        // A TCP/IO EOF without TLS close_notify must never appear as clean EOF.
        client.get_mut().0.shutdown().await.unwrap();
        assert_eq!(
            server.read_u8().await.unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
    })
    .await
    .expect("TLS truncation test stalled");
}

#[tokio::test]
async fn corrupted_application_record_is_rejected() {
    tokio::time::timeout(TIMEOUT, async {
        let pki = Pki::new();
        let (client, server) = tokio::io::duplex(8192);
        let connector = tokio_rustls::TlsConnector::from(pki.reference_client());
        let (client, server) = tokio::join!(
            connector.connect("localhost".try_into().unwrap(), client),
            TlsStream::accept(server, pki.server(false)),
        );
        let (mut client, mut server) = (client.unwrap(), server.unwrap());
        let mut record = [0; 37];
        record[..5].copy_from_slice(&[23, 3, 3, 0, 32]);
        client.get_mut().0.write_all(&record).await.unwrap();
        let error = server.read_u8().await.unwrap_err();
        assert!(error.to_string().contains("decrypt"), "{error}");
    })
    .await
    .expect("corrupt TLS record test stalled");
}

// Exercise the public vectored contract across empty slices, record boundaries,
// partial acceptance, socket backpressure and independent reader/writer wakers.
#[tokio::test]
async fn vectored_records_preserve_order_under_bidirectional_backpressure() {
    async fn write(mut stream: impl AsyncWrite + Unpin, bytes: Vec<u8>) {
        assert!(stream.is_write_vectored());
        assert_eq!(stream.write_vectored(&[]).await.unwrap(), 0);
        assert_eq!(
            stream
                .write_vectored(&[io::IoSlice::new(&[])])
                .await
                .unwrap(),
            0
        );
        let cuts = [
            0,
            0,
            1,
            16_383,
            16_384,
            16_385,
            131_073,
            131_073,
            200_003,
            bytes.len(),
        ];
        let mut buffers: Vec<_> = cuts
            .windows(2)
            .map(|w| io::IoSlice::new(&bytes[w[0]..w[1]]))
            .collect();
        let mut remaining = &mut buffers[..];
        while !remaining.is_empty() {
            let size = stream.write_vectored(remaining).await.unwrap();
            assert!(size > 0);
            io::IoSlice::advance_slices(&mut remaining, size);
        }
        stream.shutdown().await.unwrap();
    }
    async fn read(mut stream: impl AsyncRead + Unpin, bytes: Vec<u8>) {
        let mut received = Vec::new();
        stream.read_to_end(&mut received).await.unwrap();
        assert_eq!(received, bytes);
    }
    tokio::time::timeout(TIMEOUT, async {
        for fragment in [None, Some(32)] {
            let pki = Pki::new();
            let mut client_config = (*pki.client(false)).clone();
            let mut server_config = (*pki.server(false)).clone();
            client_config.max_fragment_size = fragment;
            server_config.max_fragment_size = fragment;
            let (client, server) = tokio::io::duplex(257);
            let client = TlsStream::connect(
                BufStream::with_capacity(113, 127, client),
                Arc::new(client_config),
                "localhost".try_into().unwrap(),
            );
            let server = TlsStream::accept(
                BufStream::with_capacity(79, 97, server),
                Arc::new(server_config),
            );
            let request: Vec<_> = (0..300_001).map(|i| (i % 251) as u8).collect();
            let response: Vec<_> = (0..400_007).map(|i| (i % 241) as u8).collect();
            // With tiny records, post-handshake tickets can exceed the duplex
            // capacity. Each peer must start reading after its own handshake.
            let (client, server) = tokio::join!(
                async {
                    let (reader, writer) = tokio::io::split(client.await.unwrap());
                    let writer = tokio::spawn(write(writer, request.clone()));
                    let reader = tokio::spawn(read(reader, response.clone()));
                    tokio::try_join!(writer, reader)
                },
                async {
                    let (reader, writer) = tokio::io::split(server.await.unwrap());
                    let writer = tokio::spawn(write(writer, response.clone()));
                    let reader = tokio::spawn(read(reader, request.clone()));
                    tokio::try_join!(writer, reader)
                },
            );
            client.unwrap();
            server.unwrap();
        }
    })
    .await
    .expect("vectored TLS traffic stalled");
}
