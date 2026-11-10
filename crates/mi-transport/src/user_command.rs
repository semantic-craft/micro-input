use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    fs::{self, DirBuilder, File},
    io::{self, Read},
    os::unix::{
        fs::{DirBuilderExt, PermissionsExt},
        net::UnixDatagram,
    },
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const MAX_DATAGRAM: usize = 256;
const MAX_PENDING: usize = 32;
// One AX toggle may need 500 ms traversal + 900 ms readback. A second
// intentional click may wait for that operation, but never replay an old burst.
const MAX_AGE: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ActivationPayload {
    pub version: u8,
    pub generation: String,
    pub control: u8,
    pub action: ActivationKind,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub enum ActivationKind {
    Activate,
}

impl ActivationPayload {
    pub fn new(generation: &str, control: u8) -> Self {
        Self {
            version: 1,
            generation: generation.into(),
            control,
            action: ActivationKind::Activate,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Activation {
    pub control: u8,
    pub generation: String,
    /// Receiver order, not a HID sequence number.
    pub received_sequence: u64,
    pub received_at: Instant,
}

#[derive(Default)]
struct Queue {
    armed: bool,
    failure: Option<String>,
    sequence: u64,
    events: VecDeque<Activation>,
}

pub struct UserCommandReceiver {
    generation: String,
    path: PathBuf,
    queue: Arc<Mutex<Queue>>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    arm_requests: std::sync::mpsc::Sender<std::sync::mpsc::SyncSender<Result<(), String>>>,
}

impl UserCommandReceiver {
    pub fn bind() -> io::Result<Self> {
        let mut random = [0_u8; 16];
        File::open("/dev/urandom")?.read_exact(&mut random)?;
        let generation: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
        let directory = Path::new("/tmp").join(format!("micro-input-{generation}"));
        DirBuilder::new().mode(0o700).create(&directory)?;
        let path = directory.join("in");
        let socket = match UnixDatagram::bind(&path) {
            Ok(socket) => socket,
            Err(error) => {
                let _ = fs::remove_dir(&directory);
                return Err(error);
            }
        };
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        socket.set_read_timeout(Some(Duration::from_millis(50)))?;
        let queue = Arc::new(Mutex::new(Queue::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let (started_tx, started_rx) = std::sync::mpsc::sync_channel(0);
        let (arm_requests, arm_rx) =
            std::sync::mpsc::channel::<std::sync::mpsc::SyncSender<Result<(), String>>>();
        let worker = {
            let generation = generation.clone();
            let queue = Arc::clone(&queue);
            let stop = Arc::clone(&stop);
            thread::spawn(move || {
                let _ = started_tx.send(());
                let mut bytes = [0; MAX_DATAGRAM + 1];
                while !stop.load(Ordering::Acquire) {
                    if let Ok(reply) = arm_rx.try_recv() {
                        let result = (|| {
                            socket.set_nonblocking(true).map_err(|e| e.to_string())?;
                            while socket.recv(&mut bytes).is_ok() {}
                            socket.set_nonblocking(false).map_err(|e| e.to_string())?;
                            let mut pending = queue.lock().unwrap();
                            if let Some(error) = &pending.failure {
                                return Err(error.clone());
                            }
                            pending.events.clear();
                            pending.armed = true;
                            Ok(())
                        })();
                        let _ = reply.send(result);
                    }
                    let size = match socket.recv(&mut bytes) {
                        Ok(size) => size,
                        Err(error)
                            if matches!(
                                error.kind(),
                                io::ErrorKind::WouldBlock
                                    | io::ErrorKind::TimedOut
                                    | io::ErrorKind::Interrupted
                            ) =>
                        {
                            continue
                        }
                        Err(_) => {
                            invalidate(&queue, "输入接收器已断开");
                            break;
                        }
                    };
                    if size > MAX_DATAGRAM {
                        continue;
                    }
                    let Ok(payload) = serde_json::from_slice::<ActivationPayload>(&bytes[..size])
                    else {
                        continue;
                    };
                    if payload.version != 1
                        || payload.generation != generation
                        || payload.control >= 60
                    {
                        continue;
                    }
                    let mut pending = queue.lock().unwrap();
                    if !pending.armed || pending.failure.is_some() {
                        continue;
                    }
                    if pending.events.len() == MAX_PENDING {
                        pending.events.clear();
                        pending.armed = false;
                        pending.failure = Some("输入积压，已暂停；请重新连接".into());
                        continue;
                    }
                    pending.sequence += 1;
                    let received_sequence = pending.sequence;
                    pending.events.push_back(Activation {
                        control: payload.control,
                        generation: generation.clone(),
                        received_sequence,
                        received_at: Instant::now(),
                    });
                }
            })
        };
        started_rx
            .recv()
            .map_err(|_| io::Error::other("receiver did not start"))?;
        Ok(Self {
            generation,
            path,
            queue,
            stop,
            worker: Some(worker),
            arm_requests,
        })
    }

    pub fn generation(&self) -> &str {
        &self.generation
    }
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn arm(&self) -> Result<(), String> {
        let (tx, rx) = std::sync::mpsc::sync_channel(0);
        self.arm_requests.send(tx).map_err(|_| "输入线程已退出")?;
        let _ = UnixDatagram::unbound().and_then(|socket| socket.send_to(&[], &self.path));
        rx.recv_timeout(Duration::from_secs(1))
            .map_err(|_| "输入启动屏障超时")?
    }

    pub fn invalidate(&self, reason: &str) {
        invalidate(&self.queue, reason);
    }

    pub fn failure(&self) -> Option<String> {
        self.queue.lock().unwrap().failure.clone()
    }

    pub fn take(&self) -> Result<Vec<Activation>, String> {
        self.take_at(Instant::now())
    }

    pub fn next(&self) -> Result<Option<Activation>, String> {
        let mut queue = self.queue.lock().unwrap();
        if let Some(error) = &queue.failure {
            return Err(error.clone());
        }
        if queue
            .events
            .front()
            .is_some_and(|event| event.received_at.elapsed() > MAX_AGE)
        {
            queue.events.clear();
            queue.armed = false;
            queue.failure = Some("输入已过期，已暂停；请重新连接".into());
            return Err(queue.failure.clone().unwrap());
        }
        Ok(queue.events.pop_front())
    }

    fn take_at(&self, now: Instant) -> Result<Vec<Activation>, String> {
        let mut queue = self.queue.lock().unwrap();
        if let Some(failure) = &queue.failure {
            return Err(failure.clone());
        }
        if queue
            .events
            .front()
            .is_some_and(|event| now.saturating_duration_since(event.received_at) > MAX_AGE)
        {
            queue.events.clear();
            queue.armed = false;
            queue.failure = Some("输入已过期，已暂停；请重新连接".into());
            return Err(queue.failure.clone().unwrap());
        }
        Ok(queue.events.drain(..).collect())
    }
}

fn invalidate(queue: &Mutex<Queue>, reason: &str) {
    let mut queue = queue.lock().unwrap();
    queue.events.clear();
    queue.armed = false;
    queue.failure = Some(reason.into());
}

impl Drop for UserCommandReceiver {
    fn drop(&mut self) {
        self.invalidate("接收器已关闭");
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        let _ = fs::remove_file(&self.path);
        if let Some(parent) = self.path.parent() {
            let _ = fs::remove_dir(parent);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn send(receiver: &UserCommandReceiver, value: serde_json::Value) {
        UnixDatagram::unbound()
            .unwrap()
            .send_to(&serde_json::to_vec(&value).unwrap(), receiver.path())
            .unwrap();
    }
    fn settle() {
        thread::sleep(Duration::from_millis(30));
    }

    #[test]
    fn receiver_is_private_strict_and_generation_bound() {
        let receiver = UserCommandReceiver::bind().unwrap();
        let next = UserCommandReceiver::bind().unwrap();
        assert_ne!(receiver.generation(), next.generation());
        assert!(receiver.path().as_os_str().len() < 104);
        assert_eq!(
            fs::metadata(receiver.path()).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(receiver.path().parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        let valid =
            serde_json::to_value(ActivationPayload::new(receiver.generation(), 12)).unwrap();
        send(&receiver, valid.clone());
        settle();
        assert!(receiver.take().unwrap().is_empty());
        receiver.arm().unwrap();
        for invalid in [
            serde_json::json!({"version":1,"generation":next.generation(),"control":12,"action":"Activate"}),
            serde_json::json!({"version":1,"generation":receiver.generation(),"control":60,"action":"Activate"}),
            serde_json::json!({"version":1,"generation":receiver.generation(),"control":12,"action":"Activate","command":"anything"}),
        ] {
            send(&receiver, invalid);
        }
        send(&receiver, valid);
        settle();
        let events = receiver.take().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].control, 12);
        assert_eq!(events[0].received_sequence, 1);
        receiver.invalidate("test generation ended");
        send(
            &receiver,
            serde_json::to_value(ActivationPayload::new(receiver.generation(), 12)).unwrap(),
        );
        settle();
        assert!(receiver.take().is_err());
        assert!(receiver.arm().is_err());
    }

    #[test]
    fn receiver_drops_overflow_without_replay() {
        let receiver = UserCommandReceiver::bind().unwrap();
        receiver.arm().unwrap();
        for _ in 0..=MAX_PENDING {
            send(
                &receiver,
                serde_json::to_value(ActivationPayload::new(receiver.generation(), 0)).unwrap(),
            );
        }
        settle();
        assert!(receiver.take().unwrap_err().contains("积压"));
        assert!(receiver.arm().is_err());
    }

    #[test]
    fn expired_clicks_do_not_disable_new_clicks() {
        let receiver = UserCommandReceiver::bind().unwrap();
        receiver.arm().unwrap();
        send(
            &receiver,
            serde_json::to_value(ActivationPayload::new(receiver.generation(), 0)).unwrap(),
        );
        settle();
        assert!(receiver
            .take_at(Instant::now() + MAX_AGE)
            .unwrap()
            .is_empty());
        assert!(receiver.failure().is_none());
        send(
            &receiver,
            serde_json::to_value(ActivationPayload::new(receiver.generation(), 1)).unwrap(),
        );
        settle();
        assert_eq!(receiver.next().unwrap().unwrap().control, 1);
    }

    #[test]
    fn reads_preserve_fresh_clicks_after_an_expired_prefix() {
        for take_all in [false, true] {
            let receiver = UserCommandReceiver::bind().unwrap();
            receiver.arm().unwrap();
            for control in [0, 1, 2] {
                send(
                    &receiver,
                    serde_json::to_value(ActivationPayload::new(receiver.generation(), control))
                        .unwrap(),
                );
            }
            settle();
            {
                let mut queue = receiver.queue.lock().unwrap();
                for event in queue.events.iter_mut().take(2) {
                    event.received_at = Instant::now() - MAX_AGE - Duration::from_secs(1);
                }
            }
            let events = if take_all {
                receiver.take().unwrap()
            } else {
                vec![receiver.next().unwrap().unwrap()]
            };
            assert_eq!(
                events.iter().map(|event| event.control).collect::<Vec<_>>(),
                vec![2]
            );
            assert!(receiver.next().unwrap().is_none());
            assert!(receiver.failure().is_none());
        }
    }
}
