//! One connection's byte pipeline (Netty's `decrypt` → `splitter` → `decompress` handlers and
//! `compress` → `prepender` → `encrypt` the other way), split into an inbound and an outbound half so
//! a reader task and a writer task can each own one.
//!
//! Plain functions over `bytes` buffers rather than a `tokio_util` codec: the halves change at runtime
//! (encryption and compression are switched on mid-login), and a handed-over connection must keep the
//! bytes already read, which owning the `BytesMut` makes explicit. `decode` has the shape of
//! `tokio_util::codec::Decoder::decode` should one be wanted later.

use bytes::{Buf, BufMut, Bytes, BytesMut};
use flate2::{Compress, Compression, Decompress, FlushCompress, FlushDecompress, Status};

use super::Fault;
use crate::azalea::crypto::{self, Aes128CfbDec, Aes128CfbEnc};

/// `CompressionDecoder.MAXIMUM_UNCOMPRESSED_LENGTH`, also the encoder's limit.
pub const MAXIMUM_UNCOMPRESSED_LENGTH: usize = 8_388_608;
/// `Varint21LengthFieldPrepender.MAX_VARINT21_BYTES`: a frame length fits in three VarInt bytes.
const MAX_VARINT21_BYTES: usize = 3;

pub fn var_int_size(value: i32) -> usize {
    match value as u32 {
        0..=0x7f => 1,
        0x80..=0x3fff => 2,
        0x4000..=0x1f_ffff => 3,
        0x20_0000..=0x0fff_ffff => 4,
        _ => 5,
    }
}

pub fn put_var_int(out: &mut impl BufMut, value: i32) {
    let mut v = value as u32;
    while v & !0x7f != 0 {
        out.put_u8((v & 0x7f) as u8 | 0x80);
        v >>= 7;
    }
    out.put_u8(v as u8);
}

/// `VarInt.read`: `None` when the bytes run out.
pub fn read_var_int(input: &[u8]) -> Result<Option<(i32, usize)>, Fault> {
    let mut out = 0u32;
    for (i, &b) in input.iter().enumerate() {
        out |= u32::from(b & 0x7f) << (7 * i);
        if i + 1 > 5 {
            return Err(Fault::decoder("java.lang.RuntimeException: VarInt too big"));
        }
        if b & 0x80 == 0 {
            return Ok(Some((out as i32, i + 1)));
        }
    }
    Ok(None)
}

/// The inbound half: decryption, frame splitting, decompression.
pub struct Inbound {
    cipher: Option<Aes128CfbDec>,
    /// How many bytes at the front of the read buffer are already decrypted.
    decrypted: usize,
    /// `Some(threshold)` once `setupCompression` ran (validation is always on for a server).
    threshold: Option<i32>,
    inflater: Decompress,
}

impl Default for Inbound {
    fn default() -> Self {
        Self::new()
    }
}

impl Inbound {
    pub fn new() -> Self {
        Self {
            cipher: None,
            decrypted: 0,
            threshold: None,
            inflater: Decompress::new(true),
        }
    }

    /// `Connection.setEncryptionKey`: bytes read from now on are decrypted; bytes already buffered
    /// were past Netty's `decrypt` handler (in the splitter's buffer) and stay as they are.
    pub fn enable_encryption(&mut self, key: &[u8; 16]) {
        self.cipher = Some(crypto::create_cipher(key).1);
    }

    /// `Connection.setupCompression`: a negative threshold removes decompression.
    pub fn set_compression(&mut self, threshold: i32) {
        self.threshold = (threshold >= 0).then_some(threshold);
    }

    /// The next packet body (id + payload) in `buf`, or `None` until more bytes arrive.
    pub fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<Bytes>, Fault> {
        if let Some(cipher) = &mut self.cipher {
            crypto::decrypt_packet(cipher, &mut buf[self.decrypted..]);
        }
        self.decrypted = buf.len();
        let Some(frame) = split_frame(buf)? else {
            return Ok(None);
        };
        self.decrypted = buf.len();
        match self.threshold {
            None => Ok(Some(frame)),
            Some(threshold) => self.decompress(frame, threshold).map(Some),
        }
    }

    /// `CompressionDecoder.decode` with `validateDecompressed`.
    fn decompress(&mut self, mut frame: Bytes, threshold: i32) -> Result<Bytes, Fault> {
        let Some((length, n)) = read_var_int(&frame)? else {
            return Err(Fault::decoder("java.lang.IndexOutOfBoundsException"));
        };
        frame.advance(n);
        if length == 0 {
            return Ok(frame);
        }
        if length < threshold {
            return Err(Fault::decoder(format!(
                "Badly compressed packet - size of {length} is below server threshold of {threshold}"
            )));
        }
        if length as usize > MAXIMUM_UNCOMPRESSED_LENGTH {
            return Err(Fault::decoder(format!(
                "Badly compressed packet - size of {length} is larger than protocol maximum of 8388608"
            )));
        }
        // `Inflater.inflate` into a buffer of the declared size: at most that much comes out.
        let mut out = vec![0; length as usize];
        self.inflater.reset(true);
        let result = self
            .inflater
            .decompress(&frame, &mut out, FlushDecompress::None);
        let actual = self.inflater.total_out() as usize;
        if let Err(e) = result {
            return Err(Fault::decoder(format!(
                "java.util.zip.DataFormatException: {e}"
            )));
        }
        if actual != length as usize {
            return Err(Fault::decoder(format!(
                "Badly compressed packet - actual length of uncompressed payload {actual} is does not match declared size {length}"
            )));
        }
        Ok(out.into())
    }
}

/// `Varint21FrameDecoder.decode`.
fn split_frame(buf: &mut BytesMut) -> Result<Option<Bytes>, Fault> {
    let mut length = 0usize;
    for i in 0..MAX_VARINT21_BYTES {
        let Some(&b) = buf.get(i) else {
            return Ok(None);
        };
        length |= usize::from(b & 0x7f) << (7 * i);
        if b & 0x80 == 0 {
            if length == 0 {
                return Err(Fault(
                    "io.netty.handler.codec.CorruptedFrameException: Frame length cannot be zero"
                        .into(),
                ));
            }
            if buf.len() < i + 1 + length {
                return Ok(None);
            }
            buf.advance(i + 1);
            return Ok(Some(buf.split_to(length).freeze()));
        }
    }
    Err(Fault(
        "io.netty.handler.codec.CorruptedFrameException: length wider than 21-bit".into(),
    ))
}

/// The outbound half: Paper's size check, compression, the length prefix, encryption.
pub struct Outbound {
    cipher: Option<Aes128CfbEnc>,
    threshold: Option<i32>,
    deflater: Compress,
}

impl Default for Outbound {
    fn default() -> Self {
        Self::new()
    }
}

impl Outbound {
    pub fn new() -> Self {
        Self {
            cipher: None,
            threshold: None,
            deflater: Compress::new(Compression::default(), true),
        }
    }

    pub fn enable_encryption(&mut self, key: &[u8; 16]) {
        self.cipher = Some(crypto::create_cipher(key).0);
    }

    pub fn set_compression(&mut self, threshold: i32) {
        self.threshold = (threshold >= 0).then_some(threshold);
    }

    /// Append the bytes on the wire for one packet body (id + payload) to `out`.
    pub fn encode(&mut self, body: &[u8], out: &mut BytesMut) -> Result<(), Fault> {
        if body.len() > MAXIMUM_UNCOMPRESSED_LENGTH {
            // Paper's `PacketEncoder.PacketTooLargeException` (packets with a fallback are the
            // caller's to split before they get here).
            return Err(Fault(format!(
                "net.minecraft.network.PacketEncoder$PacketTooLargeException: PacketTooLarge - packet is {}. Max is 8388608",
                body.len()
            )));
        }
        let mut contents = BytesMut::new();
        let contents: &[u8] = match self.threshold {
            None => body,
            Some(threshold) if (body.len() as i32) < threshold => {
                contents.reserve(body.len() + 1);
                contents.put_u8(0);
                contents.put_slice(body);
                &contents
            }
            Some(_) => {
                put_var_int(&mut contents, body.len() as i32);
                self.deflate(body, &mut contents);
                &contents
            }
        };
        let start = out.len();
        if var_int_size(contents.len() as i32) > MAX_VARINT21_BYTES {
            return Err(Fault(format!(
                "io.netty.handler.codec.EncoderException: Packet too large: size {} is over 8",
                contents.len()
            )));
        }
        put_var_int(out, contents.len() as i32);
        out.put_slice(contents);
        if let Some(cipher) = &mut self.cipher {
            crypto::encrypt_packet(cipher, &mut out[start..]);
        }
        Ok(())
    }

    /// `CompressionEncoder`: one zlib stream per packet (`Deflater.finish`, then `reset`).
    fn deflate(&mut self, body: &[u8], out: &mut BytesMut) {
        self.deflater.reset();
        let mut chunk = [0u8; 8192];
        loop {
            let before_in = self.deflater.total_in() as usize;
            let before_out = self.deflater.total_out();
            let status = self
                .deflater
                .compress(&body[before_in..], &mut chunk, FlushCompress::Finish)
                .expect("deflate on an in-memory buffer cannot fail");
            out.put_slice(&chunk[..(self.deflater.total_out() - before_out) as usize]);
            if status == Status::StreamEnd {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    fn pattern(n: usize) -> Vec<u8> {
        (0..n).map(|i| (i % 7) as u8).collect()
    }

    #[test]
    fn frames_like_java() {
        let mut out = BytesMut::new();
        Outbound::new().encode(&[0, 1, 2, 3], &mut out).unwrap();
        assert_eq!(&out[..], hex("0400010203"));
        out.clear();
        Outbound::new().encode(&[0; 300], &mut out).unwrap();
        assert_eq!(&out[..3], hex("ac0200"));
        // Split across reads, two frames in one buffer.
        let mut inbound = Inbound::new();
        let mut buf = BytesMut::from(&hex("0400010203")[..2]);
        assert_eq!(inbound.decode(&mut buf).unwrap(), None);
        buf.extend_from_slice(&hex("010203")[..]);
        buf.extend_from_slice(&hex("0105"));
        assert_eq!(
            &inbound.decode(&mut buf).unwrap().unwrap()[..],
            [0, 1, 2, 3]
        );
        assert_eq!(&inbound.decode(&mut buf).unwrap().unwrap()[..], [5]);
        assert!(buf.is_empty());
    }

    #[test]
    fn frame_errors() {
        let err = |b: &[u8]| Inbound::new().decode(&mut BytesMut::from(b)).unwrap_err().0;
        assert_eq!(
            err(&[0x80, 0x80, 0x80]),
            "io.netty.handler.codec.CorruptedFrameException: length wider than 21-bit"
        );
        assert_eq!(
            err(&[0]),
            "io.netty.handler.codec.CorruptedFrameException: Frame length cannot be zero"
        );
        let mut out = BytesMut::new();
        let fault = Outbound::new()
            .encode(&vec![0; 1 << 21], &mut out)
            .unwrap_err();
        assert_eq!(
            fault.0,
            "io.netty.handler.codec.EncoderException: Packet too large: size 2097152 is over 8"
        );
    }

    /// `CompressionEncoder(256)` from the 26.3 jar: 255 bytes stay raw behind a zero, 256 are deflated.
    #[test]
    fn compression_threshold_like_java() {
        let mut out = Outbound::new();
        out.set_compression(256);
        let mut bytes = BytesMut::new();
        out.encode(&pattern(255), &mut bytes).unwrap();
        let mut expected = vec![0x80, 0x02, 0x00];
        expected.extend(pattern(255));
        assert_eq!(&bytes[..], expected);
        bytes.clear();
        out.encode(&pattern(256), &mut bytes).unwrap();
        let java = hex("8002789c636064626661656318a914007e8b02fb");
        assert_eq!(&bytes[1..], java);
        // Java's frame decodes too.
        let mut inbound = Inbound::new();
        inbound.set_compression(256);
        let mut buf = BytesMut::from(&[java.len() as u8][..]);
        buf.extend_from_slice(&java);
        assert_eq!(
            &inbound.decode(&mut buf).unwrap().unwrap()[..],
            pattern(256)
        );
    }

    #[test]
    fn badly_compressed_packets() {
        let frame = |inner: &[u8]| {
            let mut b = BytesMut::new();
            put_var_int(&mut b, inner.len() as i32);
            b.extend_from_slice(inner);
            b
        };
        let decode = |inner: &[u8]| {
            let mut i = Inbound::new();
            i.set_compression(256);
            i.decode(&mut frame(inner))
        };
        let mut zlib = Vec::new();
        put_var_int(&mut zlib, 300);
        let mut o = Outbound::new();
        let mut z = BytesMut::new();
        o.deflate(&pattern(299), &mut z);
        zlib.extend_from_slice(&z);
        assert_eq!(
            decode(&zlib).unwrap_err().0,
            "io.netty.handler.codec.DecoderException: Badly compressed packet - actual length of uncompressed payload 299 is does not match declared size 300"
        );
        assert_eq!(
            decode(&[10, 0x78]).unwrap_err().0,
            "io.netty.handler.codec.DecoderException: Badly compressed packet - size of 10 is below server threshold of 256"
        );
        assert_eq!(
            decode(&[0x81, 0x80, 0x80, 0x04, 0x78]).unwrap_err().0,
            "io.netty.handler.codec.DecoderException: Badly compressed packet - size of 8388609 is larger than protocol maximum of 8388608"
        );
        assert_eq!(&decode(&[0, 7, 8]).unwrap().unwrap()[..], [7, 8]);
    }

    /// `CipherEncoder` with the key as IV (`Crypt.getCipher`), over two writes: the CFB8 state carries on.
    #[test]
    fn encryption_like_java() {
        let key = *b"1234567890123456";
        let plain: Vec<u8> = (0..40).map(|i| (i * 3) as u8).collect();
        let mut out = Outbound::new();
        out.enable_encryption(&key);
        let mut wire = BytesMut::new();
        // A 16-byte body framed is 17 bytes: the Java run encrypted plain[0..17] then plain[17..40].
        out.encode(&plain[1..17], &mut wire).unwrap();
        let mut framed = vec![16];
        framed.extend_from_slice(&plain[1..17]);
        crypto::encrypt_packet(&mut crypto::create_cipher(&key).0, &mut framed);
        assert_eq!(&wire[..], framed);
        let mut enc = crypto::create_cipher(&key).0;
        let mut a = plain[..17].to_vec();
        crypto::encrypt_packet(&mut enc, &mut a);
        assert_eq!(a, hex("759538305d91559a438c3ce9ce2a4652b1"));
        let mut b = plain[17..].to_vec();
        crypto::encrypt_packet(&mut enc, &mut b);
        assert_eq!(b, hex("37486528088f3c491d3b515b4946d568302698fbfac87d"));
        // What the outbound half writes, the inbound half reads back, even byte by byte.
        let mut inbound = Inbound::new();
        inbound.enable_encryption(&key);
        let mut buf = BytesMut::new();
        for &byte in &wire[..wire.len() - 1] {
            buf.put_u8(byte);
            assert_eq!(inbound.decode(&mut buf).unwrap(), None);
        }
        buf.put_u8(wire[wire.len() - 1]);
        assert_eq!(
            &inbound.decode(&mut buf).unwrap().unwrap()[..],
            &plain[1..17]
        );
    }
}
