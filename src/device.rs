//! Работа с донглом через hidraw в отдельном потоке.
//!
//! Мышь отвечает, только пока не спит (первый сон по умолчанию через 10 с простоя),
//! поэтому чтения и записи, которые не прошли, повторяются после следующего движения.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use crate::protocol::{self, BUTTONS, DPI, LIGHT, MACRO, RATE};

const VENDOR: u32 = 0x1D57;
const DONGLE_8K: u32 = 0xFA65;
const READ_REQUEST: u8 = 0xA0;
/// После последней правки ждём столько, прежде чем писать: ползунок шлёт десятки значений.
const SETTLE: Duration = Duration::from_millis(150);

pub enum Cmd {
    Write(Vec<u8>),
    ReadMacro(u8),
}

pub enum Event {
    Connected,
    Disconnected,
    /// Нужно движение мыши: она спит, а дело ждёт.
    Asleep(bool),
    Battery { percent: u8, charging: bool },
    /// Уровень DPI переключили кнопкой на мыши (с нуля).
    DpiLevel(u8),
    Settings { dpi: Vec<u8>, light: Vec<u8>, rate: Vec<u8>, buttons: Vec<u8> },
    Macro(u8, Vec<u8>),
    Saved,
    SaveFailed,
}

pub fn spawn(events: async_channel::Sender<Event>) -> mpsc::Sender<Cmd> {
    let (tx, rx) = mpsc::channel();
    thread::Builder::new()
        .name("ajazz-device".into())
        .spawn(move || Worker::new(events, rx).run())
        .expect("поток устройства");
    tx
}

struct Dongle {
    cfg: File,
    motion: Option<File>,
    sizes: BTreeMap<u8, usize>,
}

struct Worker {
    events: async_channel::Sender<Event>,
    cmds: mpsc::Receiver<Cmd>,
    dongle: Option<Dongle>,
    last_scan: Option<Instant>,
    last_motion: Option<Instant>,
    need_settings: bool,
    pending: BTreeMap<u8, (Vec<u8>, Instant)>,
    macro_reads: Vec<u8>,
    asleep: bool,
}

impl Worker {
    fn new(events: async_channel::Sender<Event>, cmds: mpsc::Receiver<Cmd>) -> Self {
        Self {
            events,
            cmds,
            dongle: None,
            last_scan: None,
            last_motion: None,
            need_settings: true,
            pending: BTreeMap::new(),
            macro_reads: Vec::new(),
            asleep: false,
        }
    }

    fn send(&self, e: Event) {
        let _ = self.events.send_blocking(e);
    }

    fn run(mut self) {
        loop {
            if self.dongle.is_none() {
                if self.last_scan.is_some_and(|t| t.elapsed() < Duration::from_secs(1)) {
                    thread::sleep(Duration::from_millis(100));
                    if !self.drain_commands() {
                        return;
                    }
                    continue;
                }
                self.last_scan = Some(Instant::now());
                if let Some(d) = find_dongle() {
                    self.dongle = Some(d);
                    self.need_settings = true;
                    self.send(Event::Connected);
                } else {
                    continue;
                }
            }
            self.poll_input(50);
            if !self.drain_commands() {
                return;
            }
            self.do_work();
        }
    }

    /// Команды от окна; false — окно закрыто.
    fn drain_commands(&mut self) -> bool {
        loop {
            match self.cmds.try_recv() {
                Ok(Cmd::Write(report)) => {
                    self.pending.insert(report[0], (report, Instant::now()));
                }
                Ok(Cmd::ReadMacro(index)) => {
                    if !self.macro_reads.contains(&index) {
                        self.macro_reads.push(index);
                    }
                }
                Err(mpsc::TryRecvError::Empty) => return true,
                Err(mpsc::TryRecvError::Disconnected) => return false,
            }
        }
    }

    fn poll_input(&mut self, timeout_ms: i32) {
        let Some(d) = &self.dongle else { return };
        let mut fds = vec![libc::pollfd { fd: d.cfg.as_raw_fd(), events: libc::POLLIN, revents: 0 }];
        if let Some(m) = &d.motion {
            fds.push(libc::pollfd { fd: m.as_raw_fd(), events: libc::POLLIN, revents: 0 });
        }
        let n = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as _, timeout_ms) };
        if n <= 0 {
            return;
        }
        if fds.iter().any(|f| f.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0) {
            self.lost();
            return;
        }
        if fds[0].revents & libc::POLLIN != 0 {
            let mut buf = [0u8; 64];
            match self.dongle.as_mut().unwrap().cfg.read(&mut buf) {
                Ok(n) if n >= 5 && buf[0] == 3 => self.on_status(&buf[..n]),
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(_) => return self.lost(),
            }
        }
        if fds.len() > 1 && fds[1].revents & libc::POLLIN != 0 {
            let mut buf = [0u8; 64];
            let motion = self.dongle.as_mut().unwrap().motion.as_mut().unwrap();
            while motion.read(&mut buf).is_ok() {}
            self.last_motion = Some(Instant::now());
        }
    }

    /// Входной отчёт 3: `[3, id мыши, тип, a, b]`.
    fn on_status(&mut self, r: &[u8]) {
        match r[2] {
            0x40 | 0x41 => self.send(Event::Battery { percent: r[4], charging: r[3] == 3 }),
            0x10 => self.send(Event::DpiLevel(r[3].saturating_sub(1))),
            _ => {}
        }
    }

    fn lost(&mut self) {
        self.dongle = None;
        self.last_scan = Some(Instant::now());
        self.send(Event::Disconnected);
    }

    fn set_asleep(&mut self, asleep: bool) {
        if self.asleep != asleep {
            self.asleep = asleep;
            self.send(Event::Asleep(asleep));
        }
    }

    /// Мышь недавно двигали — значит, ответит.
    fn awake(&self) -> bool {
        self.last_motion.is_some_and(|t| t.elapsed() < Duration::from_secs(8))
    }

    fn do_work(&mut self) {
        let has_work = self.need_settings
            || !self.macro_reads.is_empty()
            || self.pending.values().any(|(_, t)| t.elapsed() >= SETTLE);
        if !has_work {
            return;
        }
        // Пока мышь спит, не дёргаем донгл: ждём движения (кроме первой попытки после подключения).
        if self.asleep && !self.awake() {
            return;
        }
        let ok = self.try_work();
        self.set_asleep(!ok);
    }

    /// false — мышь не ответила, повторим после движения.
    fn try_work(&mut self) -> bool {
        if self.need_settings {
            let mut got = Vec::new();
            for rid in [DPI, LIGHT, RATE, BUTTONS] {
                match self.read(rid, 0) {
                    Some(r) => got.push(r),
                    None => return false,
                }
            }
            let buttons = got.pop().unwrap();
            let rate = got.pop().unwrap();
            let light = got.pop().unwrap();
            let dpi = got.pop().unwrap();
            self.need_settings = false;
            self.send(Event::Settings { dpi, light, rate, buttons });
        }
        while let Some(&index) = self.macro_reads.first() {
            let Some(r) = self.read(MACRO, index) else { return false };
            self.macro_reads.remove(0);
            self.send(Event::Macro(index, r));
        }
        let ready: Vec<u8> = self.pending.iter().filter(|(_, (_, t))| t.elapsed() >= SETTLE).map(|(&rid, _)| rid).collect();
        for rid in ready {
            let (report, _) = self.pending.remove(&rid).unwrap();
            match self.write_verified(&report) {
                Some(true) => self.send(Event::Saved),
                Some(false) => self.send(Event::SaveFailed),
                None => {
                    // Не дождались ответа: вернём в очередь, если окно не прислало новее.
                    self.pending.entry(rid).or_insert((report, Instant::now() - SETTLE));
                    return false;
                }
            }
        }
        true
    }

    /// Запись и сверка чтением; None — мышь не ответила.
    fn write_verified(&mut self, report: &[u8]) -> Option<bool> {
        for _ in 0..2 {
            self.write(report);
            thread::sleep(Duration::from_millis(200));
            let param = if report[0] == MACRO { report[2] } else { 0 };
            let back = self.read(report[0], param)?;
            // Сумму макроса мышь хранит в другом порядке байтов, поэтому сверяем только события.
            let n = if report[0] == MACRO { 29 + 2 * report[28] as usize } else { report.len() }.min(back.len());
            if back[..n] == report[..n] {
                return Some(true);
            }
        }
        Some(false)
    }

    fn write(&mut self, report: &[u8]) {
        if report[0] != MACRO {
            self.set_feature(report);
            return;
        }
        // Макрос длиннее отчёта 9: драйвер шлёт его кусками по 60 байт с заголовком.
        let mut data = report[3..].to_vec();
        data.resize(180, 0);
        for i in 0..3u8 {
            let mut chunk = vec![MACRO, if i == 2 { 0x0C } else { 0x40 }, report[2], i];
            chunk.extend_from_slice(&data[i as usize * 60..i as usize * 60 + 60]);
            self.set_feature(&chunk);
            thread::sleep(Duration::from_millis(150));
        }
    }

    /// Чтение настроек: просим донгл забрать отчёт у мыши (A0), ждём, забираем.
    /// Частый опрос готовности сбивает донгл, поэтому проверяем не чаще раза в 200 мс.
    fn read(&mut self, rid: u8, param: u8) -> Option<Vec<u8>> {
        let len = protocol::report_len(rid);
        let p = if rid == MACRO { param } else { param + 1 };
        thread::sleep(Duration::from_millis(60));
        if !self.set_feature(&[READ_REQUEST, rid, len as u8, 0, p, 0, 0, 0]) {
            return None;
        }
        let mut ready = false;
        for _ in 0..3 {
            thread::sleep(Duration::from_millis(200));
            let st = self.get_feature(READ_REQUEST, 8)?;
            if st.get(1) == Some(&1) {
                ready = true;
                break;
            }
        }
        if !ready {
            return None;
        }
        thread::sleep(Duration::from_millis(60));
        let mut r = self.get_feature(rid, len)?;
        r.resize(len, 0);
        (r[0] == rid).then_some(r)
    }

    fn set_feature(&mut self, data: &[u8]) -> bool {
        let Some(d) = &self.dongle else { return false };
        let size = d.sizes.get(&data[0]).copied().unwrap_or(data.len());
        let mut buf = data[..data.len().min(size)].to_vec();
        buf.resize(size, 0);
        let req = ioc(0x06, buf.len());
        unsafe { libc::ioctl(d.cfg.as_raw_fd(), req as _, buf.as_mut_ptr()) >= 0 }
    }

    fn get_feature(&mut self, rid: u8, len: usize) -> Option<Vec<u8>> {
        let d = self.dongle.as_ref()?;
        let mut buf = vec![0u8; len];
        buf[0] = rid;
        let n = unsafe { libc::ioctl(d.cfg.as_raw_fd(), ioc(0x07, len) as _, buf.as_mut_ptr()) };
        if n < 0 {
            return None;
        }
        buf.truncate(n as usize);
        Some(buf)
    }
}

/// `_IOWR('H', nr, len)` для HIDIOCSFEATURE / HIDIOCGFEATURE.
fn ioc(nr: u32, len: usize) -> u32 {
    (3 << 30) | ((len as u32) << 16) | ((b'H' as u32) << 8) | nr
}

struct Node {
    path: String,
    phys: String,
    descriptor: Vec<u8>,
    pid: u32,
}

fn find_dongle() -> Option<Dongle> {
    let mut nodes = Vec::new();
    for entry in fs::read_dir("/sys/class/hidraw").ok()?.flatten() {
        let dev = entry.path().join("device");
        let Ok(uevent) = fs::read_to_string(dev.join("uevent")) else { continue };
        let field = |key: &str| uevent.lines().find_map(|l| l.strip_prefix(key)).map(str::to_string);
        let Some(id) = field("HID_ID=") else { continue };
        let parts: Vec<u32> = id.split(':').filter_map(|x| u32::from_str_radix(x, 16).ok()).collect();
        if parts.len() != 3 || parts[1] != VENDOR || parts[2] == DONGLE_8K {
            continue;
        }
        let phys = field("HID_PHYS=").unwrap_or_default();
        let phys = phys.split("/input").next().unwrap_or_default().to_string();
        let Ok(descriptor) = fs::read(dev.join("report_descriptor")) else { continue };
        nodes.push(Node { path: format!("/dev/{}", entry.file_name().to_string_lossy()), phys, descriptor, pid: parts[2] });
    }
    let cfg = nodes.iter().find(|n| {
        let s = feature_sizes(&n.descriptor);
        s.contains_key(&DPI) && s.contains_key(&READ_REQUEST)
    })?;
    let open = |path: &str, write: bool| {
        OpenOptions::new().read(true).write(write).custom_flags(libc::O_NONBLOCK).open(path).ok()
    };
    // Движение — с мышиного интерфейса того же донгла: по нему видно, что мышь не спит.
    let motion = nodes
        .iter()
        .find(|n| n.phys == cfg.phys && n.pid == cfg.pid && n.descriptor.starts_with(&[0x05, 0x01, 0x09, 0x02]))
        .and_then(|n| open(&n.path, false));
    Some(Dongle { cfg: open(&cfg.path, true)?, motion, sizes: feature_sizes(&cfg.descriptor) })
}

/// Размеры feature-отчётов (с байтом номера) из дескриптора HID.
fn feature_sizes(d: &[u8]) -> BTreeMap<u8, usize> {
    let (mut sizes, mut rid, mut size, mut count, mut i) = (BTreeMap::new(), 0u8, 0u32, 0u32, 0usize);
    while i < d.len() {
        let prefix = d[i];
        if prefix == 0xFE {
            i += 3 + *d.get(i + 1).unwrap_or(&0) as usize;
            continue;
        }
        let n = [0, 1, 2, 4][(prefix & 3) as usize];
        let mut value = 0u32;
        for k in 0..n {
            value |= (*d.get(i + 1 + k).unwrap_or(&0) as u32) << (8 * k);
        }
        i += 1 + n;
        match prefix & 0xFC {
            0x74 => size = value,
            0x94 => count = value,
            0x84 => rid = value as u8,
            0xB0 => *sizes.entry(rid).or_insert(1) += (size * count / 8) as usize,
            _ => {}
        }
    }
    sizes
}

