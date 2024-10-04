use std::hash::Hasher;

use cuckoofilter::{CuckooFilter, ExportedCuckooFilter};
use near_sdk::borsh::{BorshDeserialize, BorshSerialize};

pub struct BorshCuckooFilter<H>(pub CuckooFilter<H>);

impl<H> BorshSerialize for BorshCuckooFilter<H>
where
    H: Hasher + Default,
{
    fn serialize<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        let exported = self.0.export();
        exported.length.serialize(writer)?;
        exported.values.serialize(writer)?;
        Ok(())
    }
}

impl<H> BorshDeserialize for BorshCuckooFilter<H> {
    fn deserialize_reader<R: std::io::Read>(reader: &mut R) -> std::io::Result<Self> {
        let length = u32::deserialize_reader(reader)?;
        let values = Vec::<u8>::deserialize_reader(reader)?;
        let exported = ExportedCuckooFilter {
            length: length as usize,
            values,
        };
        Ok(Self(exported.into()))
    }
}

impl<H> From<CuckooFilter<H>> for BorshCuckooFilter<H> {
    fn from(value: CuckooFilter<H>) -> Self {
        Self(value)
    }
}

impl<H> From<BorshCuckooFilter<H>> for CuckooFilter<H> {
    fn from(value: BorshCuckooFilter<H>) -> Self {
        value.0
    }
}

#[cfg(test)]
mod tests {
    use cuckoofilter::CuckooFilter;
    use siphasher::sip::SipHasher;

    use super::*;

    #[test]
    fn test() {
        let mut c = CuckooFilter::<SipHasher>::with_capacity((1 << 16) - 1);

        c.add("red").unwrap();
        c.add("orange").unwrap();
        c.add("yellow").unwrap();

        assert!(c.contains("red"));
        assert!(c.contains("orange"));
        assert!(!c.contains("green"));
        assert!(!c.contains("purple"));

        c.add("green").unwrap();
        c.add("blue").unwrap();
        c.add("indigo").unwrap();
        c.add("violet").unwrap();

        assert!(c.contains("red"));
        assert!(c.contains("orange"));
        assert!(c.contains("green"));
        assert!(!c.contains("purple"));

        println!("before export space: {}", c.memory_usage());

        let f = BorshCuckooFilter(c);

        let s = near_sdk::borsh::to_vec(&f).unwrap();

        let f: BorshCuckooFilter<SipHasher> = BorshDeserialize::try_from_slice(&s).unwrap();

        let c = f.0;

        assert!(c.contains("red"));
        assert!(c.contains("orange"));
        assert!(c.contains("yellow"));
        assert!(c.contains("green"));
        assert!(c.contains("blue"));
        assert!(c.contains("indigo"));
        assert!(c.contains("violet"));

        assert!(!c.contains("purple"));
        assert!(!c.contains("white"));
        assert!(!c.contains("black"));
        assert!(!c.contains("brown"));
        assert!(!c.contains("gray"));

        println!("after export space: {}", c.memory_usage());
    }
}
