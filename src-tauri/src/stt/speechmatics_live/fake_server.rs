//! A scripted local stand-in for the Speechmatics realtime gateway. Every
//! connection lifecycle case runs against it: no credentials, no network, no
//! microphone.
use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};

const RATE: u32 = 16_000;
const BYTES_PER_SEC: usize = RATE as usize * 2;

#[derive(Clone, Debug)]
enum Step {
    /// RecognitionStarted, then normal service: on EndOfStream, one final
    /// covering everything received, then EndOfTranscript.
    Ready,
    /// Ready, then after `after_bytes` of audio: confirm `confirm_secs` with a
    /// final `text`, and drop the socket without a close frame.
    DropAfter { after_bytes: usize, confirm_secs: f64, text: &'static str },
    /// Answers StartRecognition with this provider error type.
    Reject(&'static str),
    /// Accepts the socket but never confirms recognition.
    Hang,
    /// Refuses the websocket upgrade with this HTTP status.
    Http(u16),
}

#[derive(Default)]
struct Record {
    connections: AtomicUsize,
    open: AtomicUsize,
    max_open: AtomicUsize,
    end_of_stream: AtomicUsize,
    audio: Mutex<Vec<usize>>,
    auth: Mutex<Vec<String>>,
    connected_at: Mutex<Vec<Instant>>,
}

struct Fake {
    url: String,
    record: Arc<Record>,
}

fn add_transcript(text: &str, end: f64) -> String {
    serde_json::json!({"message":"AddTranscript","metadata":{"transcript":text,"start_time":0.0,"end_time":end},"results":[]}).to_string()
}

/// Steps are used one per connection; the last one repeats.
async fn fake(steps: Vec<Step>, final_text: &'static str) -> Fake {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}/v2", listener.local_addr().unwrap());
    let record = Arc::new(Record::default());
    let rec = record.clone();
    tokio::spawn(async move {
        loop {
            let Ok((tcp, _)) = listener.accept().await else { return };
            let index = rec.connections.fetch_add(1, Ordering::SeqCst);
            rec.connected_at.lock().unwrap().push(Instant::now());
            let step = steps.get(index).or(steps.last()).cloned().unwrap();
            let rec = rec.clone();
            tokio::spawn(async move {
                let auth = Arc::new(Mutex::new(String::new()));
                let seen = auth.clone();
                let refuse = if let Step::Http(code) = step { Some(code) } else { None };
                let callback = move |req: &Request, resp: Response| -> Result<Response, ErrorResponse> {
                    *seen.lock().unwrap() = req.headers().get("Authorization")
                        .and_then(|v| v.to_str().ok()).unwrap_or_default().to_owned();
                    match refuse {
                        Some(code) => {
                            let mut err = ErrorResponse::new(None);
                            *err.status_mut() = code.try_into().unwrap();
                            Err(err)
                        }
                        None => Ok(resp),
                    }
                };
                let Ok(mut ws) = tokio_tungstenite::accept_hdr_async(tcp, callback).await else { return };
                rec.auth.lock().unwrap().push(auth.lock().unwrap().clone());
                let open = rec.open.fetch_add(1, Ordering::SeqCst) + 1;
                rec.max_open.fetch_max(open, Ordering::SeqCst);
                rec.audio.lock().unwrap().push(0);
                let slot = rec.audio.lock().unwrap().len() - 1;
                serve(&mut ws, step, final_text, &rec, slot).await;
                rec.open.fetch_sub(1, Ordering::SeqCst);
            });
        }
    });
    Fake { url, record }
}

async fn serve<S>(ws: &mut tokio_tungstenite::WebSocketStream<S>, step: Step, final_text: &str, rec: &Record, slot: usize)
where S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin {
    // StartRecognition first.
    let Some(Ok(Message::Text(_start))) = ws.next().await else { return };
    match step {
        Step::Reject(kind) => {
            let _ = ws.send(Message::Text(serde_json::json!({"message":"Error","type":kind,"reason":"scripted"}).to_string().into())).await;
            let _ = ws.close(None).await;
            return;
        }
        Step::Hang => {
            while let Some(Ok(_)) = ws.next().await {}
            return;
        }
        _ => {}
    }
    let _ = ws.send(Message::Text(r#"{"message":"RecognitionStarted","id":"fake"}"#.into())).await;
    let mut received = 0usize;
    while let Some(Ok(message)) = ws.next().await {
        match message {
            Message::Binary(bytes) => {
                received += bytes.len();
                rec.audio.lock().unwrap()[slot] = received;
                if let Step::DropAfter { after_bytes, confirm_secs, text } = step {
                    if received >= after_bytes {
                        let _ = ws.send(Message::Text(add_transcript(text, confirm_secs).into())).await;
                        let _ = ws.flush().await;
                        return; // abrupt: no close frame
                    }
                }
            }
            Message::Text(text) if text.contains("EndOfStream") => {
                rec.end_of_stream.fetch_add(1, Ordering::SeqCst);
                let end = received as f64 / BYTES_PER_SEC as f64;
                let _ = ws.send(Message::Text(add_transcript(final_text, end).into())).await;
                let _ = ws.send(Message::Text(r#"{"message":"EndOfTranscript"}"#.into())).await;
                let _ = ws.close(None).await;
                return;
            }
            _ => {}
        }
    }
}

fn engine(fake: &Fake, handshake_ms: u64, retry_scale: f64) -> SpeechmaticsLiveEngine {
    SpeechmaticsLiveEngine::start_with_options(
        "test-key".into(),
        SpeechmaticsConfig { sample_rate: RATE, ..SpeechmaticsConfig::for_notebook() },
        TransportOptions {
            endpoint: Some(fake.url.clone()),
            handshake_timeout: Duration::from_millis(handshake_ms),
            retry_scale,
            diagnostics: None,
        },
    )
}

/// A 440 Hz tone, loud enough to count as sound for loss reporting.
fn tone(seconds: f64) -> Vec<f32> {
    (0..(seconds * RATE as f64) as usize)
        .map(|i| 0.3 * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / RATE as f32).sin())
        .collect()
}

async fn wait_for(status: &mut watch::Receiver<SpeechConnection>, want: impl Fn(&SpeechConnection) -> bool) -> SpeechConnection {
    tokio::time::timeout(Duration::from_secs(5), status.wait_for(|s| want(s))).await
        .expect("state never reached").expect("engine gone").clone()
}

fn finals(transcripts: &[Transcript]) -> Vec<String> {
    transcripts.iter().filter(|t| t.is_final).map(|t| t.text.clone()).collect()
}

#[tokio::test]
async fn ready_means_recognition_started_and_queued_first_words_are_sent() {
    let fake = fake(vec![Step::Ready], "hello world").await;
    let mut engine = engine(&fake, 2_000, 0.01);
    // Spoken before the service has confirmed: must be queued, not lost.
    engine.push_audio(&tone(0.5), RATE);
    assert_eq!(*engine.status.borrow(), SpeechConnection::Connecting);
    crate::stt::connection::wait_ready(engine.status.clone()).await.unwrap();
    let words = Box::new(engine).shutdown().await;
    assert_eq!(finals(&words), vec!["hello world"], "final words on stop");
    let rec = &fake.record;
    assert_eq!(rec.audio.lock().unwrap().as_slice(), &[BYTES_PER_SEC / 2]);
    assert_eq!(rec.auth.lock().unwrap().as_slice(), &["Bearer test-key".to_owned()]);
    assert_eq!(rec.end_of_stream.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn an_accepted_socket_without_confirmation_is_not_ready() {
    let fake = fake(vec![Step::Hang], "").await;
    let engine = engine(&fake, 2_000, 0.01);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(fake.record.open.load(Ordering::SeqCst), 1, "socket is open");
    assert_eq!(*engine.status.borrow(), SpeechConnection::Connecting);
}

#[tokio::test]
async fn no_free_slot_on_the_very_first_attempt_waits_the_slow_floor_then_recovers() {
    let fake = fake(vec![Step::Reject("quota_exceeded"), Step::Ready], "").await;
    let engine = engine(&fake, 2_000, 0.01);
    let mut status = engine.status.clone();
    let recovering = wait_for(&mut status, |s| matches!(s, SpeechConnection::Recovering { .. })).await;
    let SpeechConnection::Recovering { message } = recovering else { unreachable!() };
    assert!(message.contains("no free session slot"), "{message}");
    wait_for(&mut status, |s| *s == SpeechConnection::Ready).await;
    let times = fake.record.connected_at.lock().unwrap().clone();
    assert_eq!(times.len(), 2);
    // Slow floor is 8s; scaled by 0.01 → 80ms. A fast retry would be ~10ms.
    assert!(times[1] - times[0] >= Duration::from_millis(75), "{:?}", times[1] - times[0]);
}

#[tokio::test]
async fn a_handshake_timeout_is_retried() {
    let fake = fake(vec![Step::Hang, Step::Ready], "").await;
    let engine = engine(&fake, 200, 0.01);
    crate::stt::connection::wait_ready(engine.status.clone()).await.unwrap();
    assert_eq!(fake.record.connections.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn invalid_credentials_fail_once_without_retry_loops() {
    for step in [Step::Reject("not_authorised"), Step::Http(401)] {
        let fake = fake(vec![step.clone()], "").await;
        let engine = engine(&fake, 2_000, 0.01);
        let err = crate::stt::connection::wait_ready(engine.status.clone()).await.unwrap_err();
        assert!(err.contains("authorize"), "{step:?}: {err}");
        assert!(!err.contains("test-key"), "the key never appears in messages");
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(fake.record.connections.load(Ordering::SeqCst), 1, "{step:?}");
    }
}

#[tokio::test]
async fn an_invalid_config_from_the_server_is_terminal() {
    let fake = fake(vec![Step::Reject("invalid_config")], "").await;
    let engine = engine(&fake, 2_000, 0.01);
    assert!(crate::stt::connection::wait_ready(engine.status.clone()).await.is_err());
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(fake.record.connections.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_dropped_connection_replays_only_unconfirmed_audio_and_text_is_not_duplicated() {
    let fake = fake(vec![
        Step::DropAfter { after_bytes: BYTES_PER_SEC / 2, confirm_secs: 0.25, text: "first part" },
        Step::Ready,
    ], "second part").await;
    let mut engine = engine(&fake, 2_000, 0.01);
    engine.push_audio(&tone(1.0), RATE);
    let mut status = engine.status.clone();
    wait_for(&mut status, |s| *s == SpeechConnection::Ready).await;
    tokio::time::timeout(Duration::from_secs(5), async {
        while fake.record.connections.load(Ordering::SeqCst) < 2 || *status.borrow() != SpeechConnection::Ready {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.unwrap();
    let words = Box::new(engine).shutdown().await;
    let audio = fake.record.audio.lock().unwrap().clone();
    assert_eq!(audio.len(), 2);
    // Exactly the 0.75s Speechmatics never confirmed is sent again.
    assert_eq!(audio[1], BYTES_PER_SEC - BYTES_PER_SEC / 4, "{audio:?}");
    assert_eq!(finals(&words), vec!["first part second part"]);
}

#[tokio::test]
async fn stopping_during_a_backoff_is_immediate_and_leaves_nothing_running() {
    let fake = fake(vec![Step::Reject("quota_exceeded")], "").await;
    let engine = engine(&fake, 2_000, 1.0); // real 8s slow floor
    let mut status = engine.status.clone();
    wait_for(&mut status, |s| matches!(s, SpeechConnection::Recovering { .. })).await;
    let began = Instant::now();
    let words = Box::new(engine).shutdown().await;
    assert!(began.elapsed() < Duration::from_millis(500), "{:?}", began.elapsed());
    assert!(words.is_empty());
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(*status.borrow(), SpeechConnection::Stopped);
    assert_eq!(fake.record.connections.load(Ordering::SeqCst), 1, "no orphan retry after stop");
}

#[tokio::test]
async fn stopping_while_connecting_closes_the_socket() {
    let fake = fake(vec![Step::Hang], "").await;
    let engine = engine(&fake, 10_000, 0.01);
    tokio::time::timeout(Duration::from_secs(2), async {
        while fake.record.open.load(Ordering::SeqCst) == 0 { tokio::time::sleep(Duration::from_millis(5)).await; }
    }).await.unwrap();
    let began = Instant::now();
    Box::new(engine).shutdown().await;
    assert!(began.elapsed() < Duration::from_millis(500));
    tokio::time::timeout(Duration::from_secs(2), async {
        while fake.record.open.load(Ordering::SeqCst) != 0 { tokio::time::sleep(Duration::from_millis(5)).await; }
    }).await.expect("server still sees the old socket open");
}

#[tokio::test]
async fn a_backlog_at_stop_is_sent_unpaced_so_the_last_sentence_is_kept() {
    let fake = fake(vec![Step::Ready], "the last sentence").await;
    let mut engine = engine(&fake, 2_000, 0.01);
    crate::stt::connection::wait_ready(engine.status.clone()).await.unwrap();
    // 12 s arriving at once (e.g. after a stall) would take 6 s at the 2x
    // catch-up pace — past the shutdown deadline.
    engine.push_audio(&tone(12.0), RATE);
    let began = Instant::now();
    let words = Box::new(engine).shutdown().await;
    assert!(began.elapsed() < Duration::from_secs(3), "{:?}", began.elapsed());
    assert_eq!(finals(&words), vec!["the last sentence"]);
    assert_eq!(fake.record.audio.lock().unwrap()[0], BYTES_PER_SEC * 12);
}

#[tokio::test]
async fn sequential_sessions_never_overlap_on_the_server() {
    let fake = fake(vec![Step::Ready], "done").await;
    for _ in 0..3 {
        let engine = engine(&fake, 2_000, 0.01);
        crate::stt::connection::wait_ready(engine.status.clone()).await.unwrap();
        Box::new(engine).shutdown().await;
    }
    assert_eq!(fake.record.max_open.load(Ordering::SeqCst), 1);
    assert_eq!(fake.record.connections.load(Ordering::SeqCst), 3);
}
