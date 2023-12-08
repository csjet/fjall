use crate::{
    serde::{Deserializable, DeserializeError, Serializable, SerializeError},
    value::{SeqNo, UserData, UserKey},
};
use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use std::io::{Read, Write};

/// Journal marker. Every batch is wrapped in a Start marker, followed by N items, followed by an end marker.
///
/// The start marker contains the numbers of items. If the numbers of items following doesn't match, the batch is broken.
///
/// The end marker contains a CRC value. If the CRC of the items doesn't match that, the batch is broken.
///
/// If a start marker is detected, while inside a batch, the batch is broken.
///
/// # Disk representation
///
/// start: \[tag (0x0); 1 byte] \[item count; 4 byte] \[seqno; 8 bytes]
///
/// item: \[tag (0x1); 1 byte] \[tombstone; 1 byte] \[key length; 2 bytes] \[key; N bytes] \[value length; 2 bytes] \[value: N bytes]
///
/// end: \[tag (0x2): 1 byte] \[crc value; 4 byte]
#[derive(Debug, Eq, PartialEq)]
pub enum Marker {
    Start {
        item_count: u32,
        seqno: SeqNo,
    },
    Item {
        key: UserKey,
        value: UserData,
        is_tombstone: bool,
    },
    End(u32),
}

pub enum Tag {
    Start = 0,
    Item = 1,
    End = 2,
}

impl TryFrom<u8> for Tag {
    type Error = DeserializeError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        use Tag::{End, Item, Start};

        match value {
            0 => Ok(Start),
            1 => Ok(Item),
            2 => Ok(End),
            _ => Err(DeserializeError::InvalidTag(value)),
        }
    }
}

impl From<Tag> for u8 {
    fn from(val: Tag) -> Self {
        val as Self
    }
}

impl Serializable for Marker {
    fn serialize<W: Write>(&self, writer: &mut W) -> Result<(), SerializeError> {
        use Marker::{End, Item, Start};

        match self {
            Start { item_count, seqno } => {
                writer.write_u8(Tag::Start.into())?;
                writer.write_u32::<BigEndian>(*item_count)?;
                writer.write_u64::<BigEndian>(*seqno)?;
            }
            Item {
                key,
                value,
                is_tombstone,
            } => {
                writer.write_u8(Tag::Item.into())?;

                writer.write_u8(u8::from(*is_tombstone))?;

                // NOTE: Truncation is okay and actually needed
                #[allow(clippy::cast_possible_truncation)]
                writer.write_u16::<BigEndian>(key.len() as u16)?;
                writer.write_all(key)?;

                // NOTE: Truncation is okay and actually needed
                #[allow(clippy::cast_possible_truncation)]
                writer.write_u16::<BigEndian>(value.len() as u16)?;
                writer.write_all(value)?;
            }
            End(val) => {
                writer.write_u8(Tag::End.into())?;
                writer.write_u32::<BigEndian>(*val)?;
            }
        }
        Ok(())
    }
}

impl Deserializable for Marker {
    fn deserialize<R: Read>(reader: &mut R) -> Result<Self, DeserializeError> {
        match reader.read_u8()?.try_into()? {
            Tag::Start => {
                let item_count = reader.read_u32::<BigEndian>()?;
                let seqno = reader.read_u64::<BigEndian>()?;
                Ok(Self::Start { item_count, seqno })
            }
            Tag::Item => {
                let is_tombstone = reader.read_u8()? > 0;

                let key_len = reader.read_u16::<BigEndian>()?;
                let mut key = vec![0; key_len.into()];
                reader.read_exact(&mut key)?;

                let value_len = reader.read_u16::<BigEndian>()?;
                let mut value = vec![0; value_len as usize];
                reader.read_exact(&mut value)?;

                Ok(Self::Item {
                    is_tombstone,
                    key: key.into(),
                    value: value.into(),
                })
            }
            Tag::End => {
                let crc = reader.read_u32::<BigEndian>()?;
                Ok(Self::End(crc))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_log::test;

    #[test]
    fn test_serialize_and_deserialize_success() -> crate::Result<()> {
        let item = Marker::Item {
            key: vec![1, 2, 3].into(),
            value: vec![].into(),
            is_tombstone: false,
        };

        // Serialize
        let mut serialized_data = Vec::new();
        item.serialize(&mut serialized_data)?;

        // Deserialize
        let mut reader = &serialized_data[..];
        let deserialized_item = Marker::deserialize(&mut reader)?;

        assert_eq!(item, deserialized_item);

        Ok(())
    }

    #[test]
    fn test_invalid_deserialize() {
        let invalid_data = [Tag::Start as u8; 1]; // Should be followed by a u32

        // Try to deserialize with invalid data
        let mut reader = &invalid_data[..];
        let result = Marker::deserialize(&mut reader);

        match result {
            Ok(_) => panic!("should error"),
            Err(error) => match error {
                DeserializeError::Io(error) => match error.kind() {
                    std::io::ErrorKind::UnexpectedEof => {}
                    _ => panic!("should throw UnexpectedEof"),
                },
                _ => panic!("should throw UnexpectedEof"),
            },
        }
    }

    #[test]
    fn test_invalid_tag() {
        let invalid_data = [3u8; 1]; // Invalid tag

        // Try to deserialize with invalid data
        let mut reader = &invalid_data[..];
        let result = Marker::deserialize(&mut reader);

        match result {
            Ok(_) => panic!("should error"),
            Err(error) => match error {
                DeserializeError::InvalidTag(3) => {}
                _ => panic!("should throw InvalidTag"),
            },
        }
    }
}
