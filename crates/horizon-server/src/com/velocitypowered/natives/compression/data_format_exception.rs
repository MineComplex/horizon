//! `java.util.zip.DataFormatException`, which Velocity's compressors throw on a bad zlib stream.
//! PORT: replace with the java.util.zip port's class when it exists.
use crate::prelude::*;

crate::netty_exception!(DataFormatException);
