use std::{collections::BTreeMap, hash::Hasher};

use cuckoofilter::{CuckooFilter, ExportedCuckooFilter};
use near_sdk::borsh::{
    schema::{Declaration, Definition},
    BorshDeserialize, BorshSchema, BorshSerialize,
};

pub struct BorshCuckooFilter<H>(pub CuckooFilter<H>);

mod dummy_schema {
    use super::*;

    #[allow(dead_code)]
    #[derive(BorshSchema)]
    #[borsh(crate = "near_sdk::borsh")]
    pub struct ExportedCuckooFilter {
        pub length: u32,
        pub values: Vec<u8>,
    }
}

impl<H> BorshSchema for BorshCuckooFilter<H> {
    fn add_definitions_recursively(definitions: &mut BTreeMap<Declaration, Definition>) {
        dummy_schema::ExportedCuckooFilter::add_definitions_recursively(definitions);
    }

    fn declaration() -> Declaration {
        "BorshCuckooFilter".into()
    }
}

impl<H> BorshSerialize for BorshCuckooFilter<H>
where
    H: Hasher + Default,
{
    fn serialize<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        let exported = self.0.export();
        (exported.length as u32).serialize(writer)?;
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
