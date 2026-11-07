use super::*;
use dbus::channel::{MatchingReceiver, Sender};
use std::{
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

const NOTICE: u32 = 7;

/// A private session bus with a minimal notification server that answers
/// `Notify` and never reports a close on its own, like GNOME Shell.
struct Bus {
    daemon: Child,
    address: String,
    stop: Arc<AtomicBool>,
    server: Option<std::thread::JoinHandle<()>>,
}

impl Bus {
    fn start() -> Self {
        let mut daemon = Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--print-address"])
            .stdout(Stdio::piped())
            .spawn()
            .expect("dbus-daemon is installed with the Linux WebKit dependencies");
        let mut address = String::new();
        BufReader::new(daemon.stdout.take().unwrap())
            .read_line(&mut address)
            .unwrap();
        let address = address.trim().to_owned();
        let server = Connection::new_address(&address).unwrap();
        server.request_name(SERVICE, false, true, false).unwrap();
        server.start_receive(
            MatchRule::new_method_call(),
            Box::new(|call: Message, bus: &Connection| {
                if call.member().as_deref() == Some("Notify") {
                    let _ = bus.send(call.method_return().append1(NOTICE));
                }
                true
            }),
        );
        let stop = Arc::new(AtomicBool::new(false));
        let running = stop.clone();
        let server = std::thread::spawn(move || {
            while !running.load(Ordering::Acquire) {
                server.process(Duration::from_millis(20)).unwrap();
            }
        });
        Self {
            daemon,
            address,
            stop,
            server: Some(server),
        }
    }

    fn client(&self) -> Connection {
        Connection::new_address(&self.address).unwrap()
    }

    fn emit(&self, member: &'static str, message: impl FnOnce(Message) -> Message) {
        let signal = Message::new_signal(PATH, SERVICE, member).unwrap();
        self.client().send(message(signal)).unwrap();
    }
}

impl Drop for Bus {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(server) = self.server.take() {
            let _ = server.join();
        }
        let _ = self.daemon.kill();
        let _ = self.daemon.wait();
    }
}

#[test]
fn a_click_on_the_shown_notice_routes_to_its_chat() {
    let bus = Bus::start();
    let shown = Shown::send(bus.client(), Event::Completed).unwrap();
    bus.emit("ActionInvoked", |signal| {
        signal.append2(NOTICE + 1, "default")
    });
    bus.emit("ActionInvoked", |signal| signal.append2(NOTICE, "default"));
    assert_eq!(
        shown.wait(Duration::from_secs(5)),
        Some(Response::Clicked),
        "another notice's click is ignored"
    );
}

#[test]
fn a_closed_notice_ends_its_route_without_a_click() {
    let bus = Bus::start();
    let shown = Shown::send(bus.client(), Event::Approval).unwrap();
    bus.emit("NotificationClosed", |signal| signal.append2(NOTICE, 2_u32));
    assert_eq!(shown.wait(Duration::from_secs(5)), Some(Response::Closed));
}

#[test]
fn a_notice_the_server_never_closes_stops_listening_at_its_deadline() {
    let bus = Bus::start();
    let shown = Shown::send(bus.client(), Event::Failed).unwrap();
    let started = Instant::now();
    assert_eq!(shown.wait(Duration::from_millis(200)), None);
    assert!(started.elapsed() < Duration::from_secs(2));
}
