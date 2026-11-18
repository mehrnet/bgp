use crate::types::StoreError;

#[inline]
pub(crate) fn be_u32(slice: &[u8]) -> Result<u32, StoreError> {
    let bytes: [u8; 4] = slice
        .try_into()
        .map_err(|_| StoreError::InvalidFormat("truncated u32".into()))?;
    Ok(u32::from_be_bytes(bytes))
}

#[inline]
pub(crate) fn be_u64(slice: &[u8]) -> Result<u64, StoreError> {
    let bytes: [u8; 8] = slice
        .try_into()
        .map_err(|_| StoreError::InvalidFormat("truncated u64".into()))?;
    Ok(u64::from_be_bytes(bytes))
}

pub(crate) struct StringPool<'a> {
    offsets: &'a [u8],
    data: &'a [u8],
    count: u64,
}

impl<'a> StringPool<'a> {
    pub(crate) fn new(offsets: &'a [u8], data: &'a [u8]) -> Result<Self, StoreError> {
        if offsets.len() < 8 || !offsets.len().is_multiple_of(8) {
            return Err(StoreError::InvalidFormat(
                "invalid string pool offsets".into(),
            ));
        }
        let count = (offsets.len() / 8) as u64 - 1;
        let end_off = offsets
            .get((count as usize) * 8..(count as usize + 1) * 8)
            .ok_or_else(|| StoreError::InvalidFormat("string pool end missing".into()))?;
        if be_u64(end_off)? != data.len() as u64 {
            return Err(StoreError::InvalidFormat(
                "string pool length mismatch".into(),
            ));
        }
        Ok(Self {
            offsets,
            data,
            count,
        })
    }

    pub(crate) fn get_str(&self, id: u64) -> Result<String, StoreError> {
        if id >= self.count {
            return Err(StoreError::InvalidFormat(
                "string pool ID out of range".into(),
            ));
        }
        let idx = usize::try_from(id)
            .map_err(|_| StoreError::InvalidFormat("string pool ID overflow".into()))?;
        let off_pos = idx * 8;
        let start = be_u64(
            self.offsets
                .get(off_pos..off_pos + 8)
                .ok_or_else(|| StoreError::InvalidFormat("truncated pool offset".into()))?,
        )? as usize;
        let end = be_u64(
            self.offsets
                .get(off_pos + 8..off_pos + 16)
                .ok_or_else(|| StoreError::InvalidFormat("truncated pool end".into()))?,
        )? as usize;
        let bytes = self
            .data
            .get(start..end)
            .ok_or_else(|| StoreError::InvalidFormat("string pool slice out of bounds".into()))?;
        Ok(String::from_utf8_lossy(bytes).into_owned())
    }
}

pub(crate) struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    pub(crate) fn take(&mut self, len: usize) -> Result<&'a [u8], StoreError> {
        let end = self
            .pos
            .checked_add(len)
            .ok_or_else(|| StoreError::InvalidFormat("reader offset overflow".into()))?;
        let slice = self
            .buf
            .get(self.pos..end)
            .ok_or_else(|| StoreError::InvalidFormat("truncated mmap record".into()))?;
        self.pos = end;
        Ok(slice)
    }

    pub(crate) fn byte(&mut self) -> Result<u8, StoreError> {
        let b = *self
            .buf
            .get(self.pos)
            .ok_or_else(|| StoreError::InvalidFormat("truncated byte in mmap record".into()))?;
        self.pos += 1;
        Ok(b)
    }

    pub(crate) fn uvarint(&mut self) -> Result<u64, StoreError> {
        let mut x: u64 = 0;
        let mut s: u32 = 0;
        for i in 0..10 {
            let b = self.byte()?;
            if b < 0x80 {
                if i == 9 && b > 1 {
                    return Err(StoreError::InvalidFormat("uvarint overflow".into()));
                }
                return Ok(x | (u64::from(b) << s));
            }
            x |= u64::from(b & 0x7f) << s;
            s += 7;
        }
        Err(StoreError::InvalidFormat("uvarint too long".into()))
    }

    pub(crate) fn address(&mut self, version: u8) -> Result<crate::types::Address, StoreError> {
        let mut out = [0u8; 16];
        if version == 4 {
            out[12..16].copy_from_slice(self.take(4)?);
        } else if version == 6 {
            out.copy_from_slice(self.take(16)?);
        } else {
            return Err(StoreError::InvalidFormat("invalid address version".into()));
        }
        Ok(crate::types::Address(out))
    }

    pub(crate) fn inline_str(&mut self) -> Result<String, StoreError> {
        let len = usize::try_from(self.uvarint()?)
            .map_err(|_| StoreError::InvalidFormat("string length overflow".into()))?;
        if len == 0 {
            return Ok(String::new());
        }
        let bytes = self.take(len)?;
        Ok(String::from_utf8_lossy(bytes).into_owned())
    }

    pub(crate) fn pooled_str(&mut self, pool: &StringPool<'_>) -> Result<String, StoreError> {
        match self.uvarint()? {
            0 => Ok(String::new()),
            1 => self.inline_str(),
            tag => pool.get_str(tag - 2),
        }
    }

    pub(crate) fn done(&self) -> Result<(), StoreError> {
        if self.pos == self.buf.len() {
            Ok(())
        } else {
            Err(StoreError::InvalidFormat(
                "trailing bytes in mmap record".into(),
            ))
        }
    }
}
