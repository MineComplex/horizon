//! Connections on the event loop: vanilla's pipeline (AES/CFB8, VarInt21 frames, threshold
//! compression) over tokio sockets, the packet limiter, the proxy's traffic counters and the
//! HAProxy PROXY protocol.
use horizon_protocol::{
    buf::{Reader as Buf, Writer as Buf2},
    crypto::Cfb8,
    frame,
};
use std::{
    net::{IpAddr, SocketAddr},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{
        TcpStream,
        tcp::{OwnedReadHalf, OwnedWriteHalf},
    },
};

/// Bytes the proxy has read and written on every socket (BandwidthHandler).
pub static BYTES_IN: AtomicU64 = AtomicU64::new(0);
pub static BYTES_OUT: AtomicU64 = AtomicU64::new(0);

pub type Result<T> = std::result::Result<T, String>;

/// The `[packet-limiter]` of velocity.toml (PacketLimiter): averages over a moving window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Limits {
    pub interval: u32,
    pub packets_per_second: i64,
    pub bytes_per_second: i64,
    pub decompressed_bytes_per_second: i64,
}

/// One client's counts: one bucket per second of the window.
pub struct Limiter {
    limits: Limits,
    buckets: Vec<[u64; 3]>,
    second: u64,
    start: Instant,
}
impl Limiter {
    pub fn new(limits: Limits) -> Option<Self> {
        let off = limits.packets_per_second < 0
            && limits.bytes_per_second < 0
            && limits.decompressed_bytes_per_second < 0;
        (!off && limits.interval > 0).then(|| Self {
            limits,
            buckets: vec![[0; 3]; limits.interval as usize],
            second: 0,
            start: Instant::now(),
        })
    }
    /// Count one packet: `wire` bytes on the socket, `decompressed` after decompression. Err when an
    /// average goes over its limit.
    pub fn record(&mut self, wire: usize, decompressed: usize) -> Result<()> {
        self.record_at(self.start.elapsed(), wire, decompressed)
    }
    fn record_at(&mut self, elapsed: Duration, wire: usize, decompressed: usize) -> Result<()> {
        let now = elapsed.as_secs();
        let n = self.buckets.len() as u64;
        // Seconds that passed empty.
        let gap = now.saturating_sub(self.second).min(n);
        for i in 1..=gap {
            self.buckets[((self.second + i) % n) as usize] = [0; 3];
        }
        self.second = self.second.max(now);
        let bucket = &mut self.buckets[(now % n) as usize];
        bucket[0] += 1;
        bucket[1] += wire as u64;
        bucket[2] += decompressed as u64;
        let sums = self
            .buckets
            .iter()
            .fold([0u64; 3], |a, b| [a[0] + b[0], a[1] + b[1], a[2] + b[2]]);
        let limits = [
            (self.limits.packets_per_second, "packets"),
            (self.limits.bytes_per_second, "bytes"),
            (
                self.limits.decompressed_bytes_per_second,
                "decompressed bytes",
            ),
        ];
        for ((limit, what), sum) in limits.into_iter().zip(sums) {
            if limit >= 0 && sum > limit as u64 * n {
                return Err(format!("sent too many {what} per second"));
            }
        }
        Ok(())
    }
}

pub struct ConnReader {
    stream: OwnedReadHalf,
    buffer: Vec<u8>,
    decrypt: Option<Cfb8>,
    pub threshold: Option<usize>,
    pub limiter: Option<Limiter>,
}
pub struct ConnWriter {
    stream: OwnedWriteHalf,
    encrypt: Option<Cfb8>,
    pub threshold: Option<usize>,
}
pub struct Conn {
    pub reader: ConnReader,
    pub writer: ConnWriter,
}

impl Conn {
    pub fn new(stream: TcpStream) -> Self {
        let _ = stream.set_nodelay(true);
        let (read, write) = stream.into_split();
        Self {
            reader: ConnReader {
                stream: read,
                buffer: Vec::new(),
                decrypt: None,
                threshold: None,
                limiter: None,
            },
            writer: ConnWriter {
                stream: write,
                encrypt: None,
                threshold: None,
            },
        }
    }
    pub async fn connect(host: &str, port: u16, timeout: Duration) -> Result<Self> {
        match tokio::time::timeout(timeout, TcpStream::connect((host, port))).await {
            Ok(Ok(stream)) => Ok(Self::new(stream)),
            Ok(Err(e)) => Err(format!("Connection refused: {e}")),
            Err(_) => Err("Connection timed out".into()),
        }
    }
    /// SetCompression: negative thresholds disable compression.
    pub fn set_compression(&mut self, threshold: i32) {
        let t = usize::try_from(threshold).ok();
        self.reader.threshold = t;
        self.writer.threshold = t;
    }
    pub fn enable_encryption(&mut self, secret: &[u8; 16]) {
        let mut decrypt = Cfb8::new(secret, secret);
        // Bytes already read past the last packet were sent encrypted.
        decrypt.decrypt(&mut self.reader.buffer);
        self.reader.decrypt = Some(decrypt);
        self.writer.encrypt = Some(Cfb8::new(secret, secret));
    }
    pub async fn send(&mut self, body: &[u8]) -> Result<()> {
        self.writer.send(body).await
    }
    pub async fn recv(&mut self) -> Result<(i32, Vec<u8>)> {
        self.reader.recv().await
    }
    /// `recv` that gives up after `timeout`.
    pub async fn recv_within(&mut self, timeout: Duration) -> Result<(i32, Vec<u8>)> {
        self.reader.recv_within(Some(timeout)).await
    }
}

impl ConnReader {
    /// The next whole packet: (id, payload).
    pub async fn recv(&mut self) -> Result<(i32, Vec<u8>)> {
        loop {
            if let Some(frame) = frame::split(&mut self.buffer).map_err(|e| e.0)? {
                let wire = frame.len();
                let body = frame::decompress(frame, self.threshold).map_err(|e| e.0)?;
                if let Some(limiter) = &mut self.limiter {
                    limiter.record(wire, body.len())?;
                }
                let mut r = Buf::new(&body);
                let id = r.varint().map_err(|e| e.0)?;
                let start = r.position();
                return Ok((id, body[start..].to_vec()));
            }
            let mut chunk = [0u8; 16384];
            let n = self
                .stream
                .read(&mut chunk)
                .await
                .map_err(|e| e.to_string())?;
            if n == 0 {
                return Err("Connection closed".into());
            }
            BYTES_IN.fetch_add(n as u64, Ordering::Relaxed);
            let data = &mut chunk[..n];
            if let Some(c) = &mut self.decrypt {
                c.decrypt(data);
            }
            self.buffer.extend_from_slice(data);
        }
    }
    pub async fn recv_within(&mut self, timeout: Option<Duration>) -> Result<(i32, Vec<u8>)> {
        match timeout {
            None => self.recv().await,
            Some(t) => tokio::time::timeout(t, self.recv())
                .await
                .unwrap_or_else(|_| Err("Read timed out".into())),
        }
    }
}

impl ConnWriter {
    pub async fn send(&mut self, body: &[u8]) -> Result<()> {
        let contents = frame::contents(body, self.threshold).map_err(|e| e.0)?;
        let mut framed = frame::prefixed(&contents);
        if let Some(c) = &mut self.encrypt {
            c.encrypt(&mut framed);
        }
        self.stream
            .write_all(&framed)
            .await
            .map_err(|e| e.to_string())?;
        BYTES_OUT.fetch_add(framed.len() as u64, Ordering::Relaxed);
        Ok(())
    }
    pub async fn shutdown(&mut self) {
        let _ = self.stream.shutdown().await;
    }
}

/// A packet body: VarInt id then payload.
pub fn body(id: i32, payload: &[u8]) -> Vec<u8> {
    let mut w = Buf2::new();
    w.varint(id);
    w.bytes(payload);
    w.data
}

const V2_SIGNATURE: [u8; 12] = *b"\r\n\r\n\0\r\nQUIT\n";

/// The HAProxy PROXY header (v1 or v2) at the start of a connection: the client's address, or
/// `None` for a health check (LOCAL / UNKNOWN).
pub async fn read_proxy_header(stream: &mut TcpStream) -> Result<Option<SocketAddr>> {
    let err = |e: std::io::Error| e.to_string();
    let first = stream.read_u8().await.map_err(err)?;
    if first == b'P' {
        // v1: "PROXY TCP4 src dst sport dport\r\n", at most 107 bytes.
        let mut line = vec![first];
        while !line.ends_with(b"\r\n") {
            if line.len() >= 107 {
                return Err("PROXY header too long".into());
            }
            line.push(stream.read_u8().await.map_err(err)?);
        }
        return parse_v1(&line);
    }
    let mut header = [0u8; 16];
    header[0] = first;
    stream.read_exact(&mut header[1..]).await.map_err(err)?;
    if header[..12] != V2_SIGNATURE {
        return Err("no PROXY protocol header".into());
    }
    let len = usize::from(u16::from_be_bytes([header[14], header[15]]));
    let mut rest = vec![0u8; len];
    stream.read_exact(&mut rest).await.map_err(err)?;
    parse_v2(&header, &rest)
}

fn parse_v1(line: &[u8]) -> Result<Option<SocketAddr>> {
    let text = std::str::from_utf8(line).map_err(|_| "bad PROXY header")?;
    let parts: Vec<&str> = text.trim_end().split(' ').collect();
    match parts.as_slice() {
        ["PROXY", "UNKNOWN", ..] => Ok(None),
        ["PROXY", "TCP4" | "TCP6", src, _dst, sport, _dport] => {
            let ip: IpAddr = src.parse().map_err(|_| "bad PROXY source address")?;
            let port: u16 = sport.parse().map_err(|_| "bad PROXY source port")?;
            Ok(Some(SocketAddr::new(ip, port)))
        }
        _ => Err("bad PROXY header".into()),
    }
}

fn parse_v2(header: &[u8; 16], rest: &[u8]) -> Result<Option<SocketAddr>> {
    if header[12] >> 4 != 2 {
        return Err("bad PROXY version".into());
    }
    // LOCAL: the balancer's own health check.
    if header[12] & 0x0f == 0 {
        return Ok(None);
    }
    match header[13] >> 4 {
        1 if rest.len() >= 12 => {
            let ip = IpAddr::from([rest[0], rest[1], rest[2], rest[3]]);
            Ok(Some(SocketAddr::new(
                ip,
                u16::from_be_bytes([rest[8], rest[9]]),
            )))
        }
        2 if rest.len() >= 36 => {
            let mut octets = [0u8; 16];
            octets.copy_from_slice(&rest[..16]);
            let port = u16::from_be_bytes([rest[32], rest[33]]);
            Ok(Some(SocketAddr::new(IpAddr::from(octets), port)))
        }
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proxy_headers() {
        assert_eq!(
            parse_v1(b"PROXY TCP4 203.0.113.7 10.0.0.1 51000 25565\r\n").unwrap(),
            Some("203.0.113.7:51000".parse().unwrap())
        );
        assert_eq!(parse_v1(b"PROXY UNKNOWN\r\n").unwrap(), None);
        assert!(parse_v1(b"PROXY TCP4 x y 1 2\r\n").is_err());
        let mut header = [0u8; 16];
        header[..12].copy_from_slice(&V2_SIGNATURE);
        header[12] = 0x21;
        header[13] = 0x11;
        let rest = [198, 51, 100, 9, 10, 0, 0, 1, 0x1f, 0x90, 0x63, 0xdd];
        assert_eq!(
            parse_v2(&header, &rest).unwrap(),
            Some("198.51.100.9:8080".parse().unwrap())
        );
        header[12] = 0x20;
        assert_eq!(parse_v2(&header, &rest).unwrap(), None);
    }

    #[test]
    fn limiter_averages_over_the_window() {
        let limits = Limits {
            interval: 2,
            packets_per_second: 3,
            bytes_per_second: -1,
            decompressed_bytes_per_second: 100,
        };
        let mut l = Limiter::new(limits).unwrap();
        let at = Duration::from_millis;
        // 6 packets in 2 seconds is an average of 3: allowed; the 7th is not.
        for i in 0..6 {
            l.record_at(at(i * 100), 1, 1).unwrap();
        }
        assert!(l.record_at(at(700), 1, 1).is_err());
        // Two seconds later the window is empty again.
        let mut l = Limiter::new(limits).unwrap();
        for i in 0..6 {
            l.record_at(at(i * 100), 1, 1).unwrap();
        }
        l.record_at(at(2500), 1, 1).unwrap();
        // A compression bomb: few packets, many decompressed bytes.
        let mut l = Limiter::new(limits).unwrap();
        assert!(l.record_at(at(0), 10, 1000).is_err());
        let off = Limits {
            interval: 7,
            packets_per_second: -1,
            bytes_per_second: -1,
            decompressed_bytes_per_second: -1,
        };
        assert!(Limiter::new(off).is_none());
    }
}
