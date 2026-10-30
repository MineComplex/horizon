//! Port of `net.kyori.adventure.nbt.api.BinaryTagHolder` (and `BinaryTagHolderImpl`): SNBT text.
use crate::net::kyori::adventure::util::Codec;

// ---- interface net.kyori.adventure.nbt.api.BinaryTagHolder
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct BinaryTagHolder {
    string: String,
}
impl BinaryTagHolder {
    /// `BinaryTagHolder.encode(T, Codec<? super T, String, ?, EX>)`.
    pub fn encode<T, DX, EX>(nbt: T, codec: &Codec<T, String, DX, EX>) -> Result<BinaryTagHolder, EX> {
        Ok(BinaryTagHolder { string: codec.encode(nbt)? })
    }
    /// `BinaryTagHolder.binaryTagHolder(String)`.
    pub fn binary_tag_holder(string: &str) -> BinaryTagHolder {
        BinaryTagHolder { string: string.to_string() }
    }
    /// `BinaryTagHolder.of(String)` (deprecated alias).
    pub fn of(string: &str) -> BinaryTagHolder {
        BinaryTagHolder::binary_tag_holder(string)
    }
    pub fn string(&self) -> String {
        self.string.clone()
    }
    /// `get(Codec<T, String, DX, ?>)`.
    pub fn get<T, DX, EX>(&self, codec: &Codec<T, String, DX, EX>) -> Result<T, DX> {
        codec.decode(self.string.clone())
    }
}
