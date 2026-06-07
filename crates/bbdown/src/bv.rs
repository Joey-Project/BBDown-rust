use crate::{Error, Result};

const ALPHABET: &[u8; 58] = b"FcwAPNKTMug3GV5Lj7EJnHpWsx4tb8haYeviqBz6rkCy12mUSDQX9RdoZf";
const XOR_CODE: u64 = 23_442_827_791_579;
const MASK_CODE: u64 = (1_u64 << 51) - 1;
const BASE: u64 = 58;
const BV_LEN: usize = 9;

pub fn decode(input: &str) -> Result<u64> {
    let bv = normalize(input)?;
    let mut bytes = bv.into_bytes();
    bytes.swap(0, 6);
    bytes.swap(1, 4);

    let mut avid = 0_u64;
    for byte in bytes {
        let table_index = ALPHABET
            .iter()
            .position(|candidate| *candidate == byte)
            .ok_or_else(|| Error::InvalidInput(format!("invalid BV id `{input}`")))?;
        avid = avid
            .checked_mul(BASE)
            .and_then(|value| value.checked_add(u64::try_from(table_index).ok()?))
            .ok_or_else(|| Error::InvalidInput(format!("invalid BV id `{input}`")))?;
    }

    Ok((avid & MASK_CODE) ^ XOR_CODE)
}

fn normalize(input: &str) -> Result<String> {
    if input.len() == 12 && input.starts_with("BV1") {
        Ok(input[3..].to_owned())
    } else if input.len() == BV_LEN {
        Ok(input.to_owned())
    } else {
        Err(Error::InvalidInput(format!("invalid BV id `{input}`")))
    }
}

#[cfg(test)]
mod tests {
    use super::decode;

    #[test]
    fn decodes_known_bv() -> anyhow::Result<()> {
        assert_eq!(decode("BV1Q541167Qg")?, 455_017_605);
        Ok(())
    }
}
