pub mod codec;
pub mod format;
pub mod index;
pub mod lpm;
pub mod queries;
pub mod range_index;
pub mod records;
pub mod route_lpm;
pub mod types;

use std::fs::File;
use std::net::IpAddr;
use std::path::{Path, PathBuf};

use memmap2::Mmap;

use crate::codec::StringPool;
use crate::format::{
    parse_section_table, selector_sections, validate_dataset, warm_file_page_cache,
    warm_selector_slices,
};
use crate::lpm::lookup_selection_lpm;
use crate::route_lpm::lookup_route_lpm;
use crate::types::{HEADER_SIZE, Metadata, SECTION_COUNT, Section, StoreError};

#[derive(Debug)]
enum Backing {
    Mmap(Mmap),
    Owned(Vec<u8>),
}

impl Backing {
    fn as_slice(&self) -> &[u8] {
        match self {
            Self::Mmap(m) => m,
            Self::Owned(v) => v,
        }
    }
}

#[derive(Debug)]
pub struct Db {
    path: Option<PathBuf>,
    backing: Backing,
    sections: [(usize, usize); SECTION_COUNT],
    meta: Metadata,
}

impl Db {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let path_buf = path.as_ref().to_path_buf();
        let file = File::open(&path_buf)?;
        if file.metadata()?.len() < HEADER_SIZE as u64 {
            return Err(StoreError::InvalidFormat("mmap file too small".into()));
        }
        // SAFETY: The MNMF dataset file is opened read-only and never mutated in place.
        #[allow(unsafe_code)]
        let mmap = unsafe { Mmap::map(&file)? };
        Self::init(Some(path_buf), Backing::Mmap(mmap))
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, StoreError> {
        Self::init(None, Backing::Owned(bytes))
    }

    fn init(path: Option<PathBuf>, backing: Backing) -> Result<Self, StoreError> {
        let data = backing.as_slice();
        let sections = parse_section_table(data)?;
        let meta = validate_dataset(data, &sections)?;
        Ok(Self {
            path,
            backing,
            sections,
            meta,
        })
    }

    fn sec(&self, section: Section) -> &[u8] {
        let (off, len) = self
            .sections
            .get(section as usize)
            .copied()
            .unwrap_or((0, 0));
        self.backing.as_slice().get(off..off + len).unwrap_or(&[])
    }

    fn pool(&self) -> Result<StringPool<'_>, StoreError> {
        StringPool::new(
            self.sec(Section::StringPoolOffsets),
            self.sec(Section::StringPoolData),
        )
    }

    #[must_use]
    pub fn size_bytes(&self) -> u64 {
        self.backing.as_slice().len() as u64
    }

    #[must_use]
    pub fn metadata(&self) -> &Metadata {
        &self.meta
    }

    #[must_use]
    pub fn compact_selector_bytes(&self) -> u64 {
        selector_sections()
            .iter()
            .map(|&s| self.sec(s).len() as u64)
            .sum()
    }

    pub fn warm_selectors(&self) -> u64 {
        warm_selector_slices(selector_sections().into_iter().map(|s| self.sec(s)))
    }

    pub fn warm_dataset(&self) -> Result<u64, StoreError> {
        match &self.path {
            Some(path) => warm_file_page_cache(path),
            None => Ok(self.warm_selectors().max(self.size_bytes())),
        }
    }

    pub fn lookup_allocation_lpm(&self, ip: IpAddr) -> Result<u32, StoreError> {
        let sec = if ip.is_ipv4() {
            Section::AllocLpmV4
        } else {
            Section::AllocLpmV6
        };
        lookup_selection_lpm(self.sec(sec), ip)
    }

    pub fn lookup_geofeed_lpm(&self, ip: IpAddr) -> Result<u32, StoreError> {
        let sec = if ip.is_ipv4() {
            Section::GeoLpmV4
        } else {
            Section::GeoLpmV6
        };
        lookup_selection_lpm(self.sec(sec), ip)
    }

    pub fn lookup_route_lpm(&self, ip: IpAddr) -> Result<Vec<u32>, StoreError> {
        let sec = if ip.is_ipv4() {
            Section::RouteLpmV4
        } else {
            Section::RouteLpmV6
        };
        lookup_route_lpm(self.sec(sec), ip)
    }
}
