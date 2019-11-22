#![recursion_limit = "256"]
use std::net::{SocketAddr, SocketAddrV4};
use std::process::Command;
use std::time::{Duration, Instant};

use bytes::Bytes;
use failure::Error;
use futures::{join, stream, SinkExt, Stream, StreamExt};

use tokio::net::UdpSocket;
use tokio::time::interval;
use tokio_util::codec::BytesCodec;
use tokio_util::udp::UdpFramed;

use srt::{ConnInitMethod, SrtSocketBuilder};

fn counting_stream(packets: u32, delay: Duration) -> impl Stream<Item = Bytes> {
    stream::iter(0..packets)
        .map(|i| Bytes::from(i.to_string()))
        .zip(interval(delay))
        .map(|(b, _)| b)
}

async fn udp_recvr(packets: u32, port: u16) -> Result<(), Error> {
    // start udp listener
    let mut socket = UdpFramed::new(
        UdpSocket::bind(&SocketAddr::V4(SocketAddrV4::new(
            "127.0.0.1".parse().unwrap(),
            port,
        )))
        .await
        .unwrap(),
        BytesCodec::new(),
    );

    let mut i = 0;
    while let Some(a) = socket.next().await {
        let (a, _) = a?;

        assert_eq!(&a, &i.to_string());

        i += 1;

        // stransmit does not totally care if it sends 100% of it's packets
        // (which is prob fair), just make sure that we got at least 2/3s of it
        if i >= packets * 2 / 3 {
            break;
        }
    }
    assert!(i >= packets * 2 / 3);

    Ok(())
}

#[tokio::test]
async fn stransmit_client() -> Result<(), Error> {
    let _ = env_logger::try_init();

    const PACKETS: u32 = 1_000;

    // stranmsit process
    let mut stransmit = Command::new("srt-live-transmit")
        .arg("udp://:2002")
        .arg("srt://127.0.0.1:2003?latency=182")
        .arg("-a:no") // don't reconnect
        .spawn()?;

    let listener = async {
        // connect to process
        let mut conn = SrtSocketBuilder::new(ConnInitMethod::Listen)
            .local_port(2003)
            .latency(Duration::from_millis(827))
            .connect_receiver()
            .await
            .unwrap();
        assert_eq!(conn.settings().tsbpd_latency, Duration::from_millis(827));

        let mut i = 0;

        // start the data generation process
        join!(
            async {
                let mut sock = UdpFramed::new(
                    UdpSocket::bind("127.0.0.1:0").await.unwrap(),
                    BytesCodec::new(),
                );
                let mut stream = counting_stream(PACKETS, Duration::from_millis(1))
                    .map(|b| Ok((b, "127.0.0.1:2002".parse().unwrap())));

                sock.send_all(&mut stream).await.unwrap();
                sock.close().await.unwrap();
            },
            async {
                // receive
                while let Some(p) = conn.next().await {
                    let (_, b) = p.unwrap();

                    assert_eq!(&i.to_string(), &b);

                    i += 1;

                    if i > PACKETS * 2 / 3 {
                        break;
                    }
                }
                assert!(i > PACKETS * 2 / 3);
            }
        );
    };

    listener.await;
    stransmit.wait()?;

    Ok(())
}

// srt-rs connects to stransmit and sends to stransmit
#[tokio::test]
async fn stransmit_server() -> Result<(), Error> {
    let _ = env_logger::try_init();

    const PACKETS: u32 = 1_000;

    // start SRT connector
    let serv = async {
        let mut sender =
            SrtSocketBuilder::new(ConnInitMethod::Connect("127.0.0.1:2000".parse().unwrap()))
                .latency(Duration::from_millis(99))
                .connect_sender()
                .await
                .unwrap();

        assert_eq!(sender.settings().tsbpd_latency, Duration::from_millis(123));

        let mut stream =
            counting_stream(PACKETS, Duration::from_millis(1)).map(|b| Ok((Instant::now(), b)));
        sender.send_all(&mut stream).await.unwrap();
        sender.close().await.unwrap();
    };

    let udp_recv = async { udp_recvr(PACKETS * 2 / 3, 2001).await.unwrap() };

    // start stransmit process
    let mut child = Command::new("srt-live-transmit")
        .arg("srt://:2000?latency=123?blocking=true")
        .arg("udp://127.0.0.1:2001")
        .arg("-a:no") // don't auto-reconnect
        .spawn()?;

    join!(serv, udp_recv);

    child.wait()?;

    Ok(())
}

// #[test]
// fn stransmit_rendezvous() {
//     let _ = env_logger::try_init();

//     const PACKETS: u32 = 1_000;

//     let sender_thread = thread::spawn(move || {
//         let sock = SrtSocketBuilder::new(ConnInitMethod::Rendezvous(
//             "127.0.0.1:2004".parse().unwrap(),
//         ))
//         .local_port(2005)
//         .build()
//         .unwrap();

//         let conn = sock.wait().unwrap().sender();

//         conn.send_all(
//             counting_stream(PACKETS, Duration::from_millis(1)).map(|b| (Instant::now(), b)),
//         )
//         .wait()
//         .unwrap();
//     });

//     let udp_recvr = udp_recvr(PACKETS * 2 / 3, 2006).unwrap();

//     let stransmit = Command::new("srt-live-transmit")
//         .arg("srt://127.0.0.1:1234?mode=rendezvous")
//         .arg();
// }
