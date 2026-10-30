//! Port of `com.velocitypowered.natives.compression.JavaVelocityCompressor`: zlib through flate2 (Java's
//! `Deflater`/`Inflater` are zlib too, so the bytes match Paper's).
use crate::com::velocitypowered::natives::buffer_preference::BufferPreference;
use crate::com::velocitypowered::natives::compression::data_format_exception::DataFormatException;
use crate::com::velocitypowered::natives::compression::velocity_compressor_factory::VelocityCompressorFactory;
use crate::io::netty::buffer::ByteBuf;
use crate::java::lang::Throwable;
use crate::prelude::*;
use flate2::{Compress, Compression, Decompress, FlushCompress, FlushDecompress, Status};

/// `JavaVelocityCompressor.FACTORY`.
pub static FACTORY: VelocityCompressorFactory = VelocityCompressorFactory;

/// Velocity's `ZLIB_BUFFER_SIZE`.
pub const ZLIB_BUFFER_SIZE: i32 = 8192;

pub struct JavaVelocityCompressor {
    pub this: Weak<Self>,
    deflater: parking_lot::Mutex<Compress>,
    inflater: parking_lot::Mutex<Decompress>,
    disposed: AtomicBool,
}
crate::jobject!(JavaVelocityCompressor);

fn data_format(message: String) -> Throwable {
    DataFormatException::new_1(&message).into()
}

impl JavaVelocityCompressor {
    pub fn this(&self) -> JRef<Self> {
        self.this.upgrade().expect("this")
    }
    /// `new JavaVelocityCompressor(level)`: `new Deflater(level)` (-1 = default) and `new Inflater()`.
    pub fn new(level: i32) -> JRef<Self> {
        let compression = if (0..=9).contains(&level) { Compression::new(level as u32) } else { Compression::default() };
        Arc::new_cyclic(|this| JavaVelocityCompressor {
            this: this.clone(),
            deflater: parking_lot::Mutex::new(Compress::new(compression, true)),
            inflater: parking_lot::Mutex::new(Decompress::new(true)),
            disposed: AtomicBool::new(false),
        })
    }
    fn ensure_not_disposed(&self) {
        if self.disposed.load(AtomicOrdering::Acquire) {
            panic!("IllegalStateException: Object already disposed");
        }
    }
    pub fn inflate(&self, source: ByteBuf, destination: ByteBuf, uncompressed_size: i32) -> Result<(), Throwable> {
        self.ensure_not_disposed();
        let orig_idx = source.reader_index();
        let input = source.with_readable(|b| b.to_vec());
        let mut inflater = self.inflater.lock();
        inflater.reset(true);
        let mut out = vec![0u8; ZLIB_BUFFER_SIZE as usize];
        let result = (|| {
            let mut finished = false;
            while !finished && (inflater.total_in() as usize) < input.len() {
                let before_in = inflater.total_in() as usize;
                let before_out = inflater.total_out();
                let status = inflater
                    .decompress(&input[before_in..], &mut out, FlushDecompress::None)
                    .map_err(|e| data_format(e.to_string()))?;
                let produced = (inflater.total_out() - before_out) as usize;
                if produced > 0 {
                    destination.write_byte_slice(&out[..produced]);
                }
                finished = status == Status::StreamEnd;
                if !finished && produced == 0 && inflater.total_in() as usize == before_in {
                    break;
                }
            }
            if !finished {
                return Err(data_format(format!(
                    "Received a deflate stream that was too large, wanted {uncompressed_size}"
                )));
            }
            source.reader_index_1(orig_idx + inflater.total_in() as i32);
            Ok(())
        })();
        inflater.reset(true);
        result
    }
    pub fn deflate(&self, source: ByteBuf, destination: ByteBuf) -> Result<(), Throwable> {
        self.ensure_not_disposed();
        let orig_idx = source.reader_index();
        let input = source.with_readable(|b| b.to_vec());
        let mut deflater = self.deflater.lock();
        deflater.reset();
        let mut out = vec![0u8; ZLIB_BUFFER_SIZE as usize];
        loop {
            let before_in = deflater.total_in() as usize;
            let before_out = deflater.total_out();
            let status = deflater
                .compress(&input[before_in..], &mut out, FlushCompress::Finish)
                .map_err(|e| data_format(e.to_string()))?;
            let produced = (deflater.total_out() - before_out) as usize;
            if produced > 0 {
                destination.write_byte_slice(&out[..produced]);
            }
            if status == Status::StreamEnd {
                break;
            }
        }
        source.reader_index_1(orig_idx + deflater.total_in() as i32);
        deflater.reset();
        Ok(())
    }
    pub fn close(&self) {
        self.disposed.store(true, AtomicOrdering::Release);
    }
    pub fn preferred_buffer_type(&self) -> BufferPreference {
        BufferPreference::HEAP_PREFERRED
    }
}
