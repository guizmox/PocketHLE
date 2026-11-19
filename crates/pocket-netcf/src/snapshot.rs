//! Managed checkpoint data. VM objects retain IDs; host paths/resources are rebuilt.
pub use crate::runtime::snapshot::ManagedSnapshot;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeMap;

/// Borrowed names during normal execution; owned names after decoding.
/// New CF properties/events need no second snapshot-only name whitelist.
#[derive(Clone, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct ServiceName(std::borrow::Cow<'static, str>);
impl ServiceName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl From<&'static str> for ServiceName {
    fn from(value: &'static str) -> Self {
        Self(std::borrow::Cow::Borrowed(value))
    }
}
impl std::borrow::Borrow<str> for ServiceName {
    fn borrow(&self) -> &str {
        &self.0
    }
}
pub(crate) mod array {
    use super::*;
    pub fn serialize<S: Serializer, T: Serialize, const N: usize>(
        value: &[T; N],
        s: S,
    ) -> Result<S::Ok, S::Error> {
        value.as_slice().serialize(s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>, T: Deserialize<'de>, const N: usize>(
        d: D,
    ) -> Result<[T; N], D::Error> {
        Vec::<T>::deserialize(d)?
            .try_into()
            .map_err(|_| serde::de::Error::custom("Invalid managed fixed array"))
    }
}
pub(crate) mod frames {
    use super::*;
    use pocket_kernel::{framebuffer::PixelSnapshot, Framebuffer};
    pub fn serialize<S: Serializer>(
        value: &BTreeMap<usize, Framebuffer>,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        value
            .iter()
            .map(|(&id, frame)| (id, PixelSnapshot::capture(frame)))
            .collect::<BTreeMap<_, _>>()
            .serialize(s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(
        d: D,
    ) -> Result<BTreeMap<usize, Framebuffer>, D::Error> {
        BTreeMap::<usize, PixelSnapshot>::deserialize(d)?
            .into_iter()
            .map(|(id, image)| Ok((id, image.restore().map_err(serde::de::Error::custom)?)))
            .collect()
    }
}
pub(crate) mod images {
    use super::*;
    #[derive(Serialize, Deserialize)]
    struct Image {
        width: u32,
        height: u32,
        #[serde(with = "serde_bytes")]
        bytes: Vec<u8>,
    }
    pub fn serialize<S: Serializer>(
        value: &[Option<image::RgbaImage>],
        s: S,
    ) -> Result<S::Ok, S::Error> {
        value
            .iter()
            .map(|image| {
                image.as_ref().map(|p| Image {
                    width: p.width(),
                    height: p.height(),
                    bytes: p.as_raw().clone(),
                })
            })
            .collect::<Vec<_>>()
            .serialize(s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(
        d: D,
    ) -> Result<Vec<Option<image::RgbaImage>>, D::Error> {
        Vec::<Option<Image>>::deserialize(d)?
            .into_iter()
            .map(|i| {
                i.map(|p| {
                    if p.width == 0 || p.height == 0 || p.width > 4096 || p.height > 4096 {
                        return Err(serde::de::Error::custom("Invalid managed image dimensions"));
                    }
                    image::RgbaImage::from_raw(p.width, p.height, p.bytes)
                        .ok_or_else(|| serde::de::Error::custom("Invalid managed image length"))
                })
                .transpose()
            })
            .collect()
    }
}
