package mmapstore

import (
	"bytes"
	"context"
	"encoding/binary"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/netip"
	"os"
	"sort"

	"golang.org/x/sys/unix"
)

const (
	SchemaVersion uint32 = 1
	FormatVersion uint32 = 1

	headerFixedSize  = 16
	sectionEntrySize = 16
	sectionCount     = 31
	HeaderSize       = headerFixedSize + sectionCount*sectionEntrySize // 512 bytes

	autnumIndexStride   = 16 // asNumber[4] + offset[8] + length[4]
	asnRouteIndexStride = 16 // asNumber[4] + firstIdx[8] + count[4]
	summaryIndexStride  = 17 // network[4] + prefixLen[1] + offset[8] + length[4]

	prefixV4Stride = 9  // network[4] + prefixLen[1] + id[4]
	prefixV6Stride = 21 // network[16] + prefixLen[1] + id[4]
	rangeV4Stride  = 12 // start[4] + end[4] + id[4]
	rangeV6Stride  = 36 // start[16] + end[16] + id[4]
	RangeKeySize   = 37 // version[1] + start[16] + end[16] + id[4]

	maxPoolStrings = 2_000_000
	warmBufferSize = 4 << 20
)

var (
	Magic             = [4]byte{'M', 'N', 'M', 'F'}
	ErrQueryTooBroad  = errors.New("overlap scan limit exceeded")
	errTruncatedSlice = errors.New("truncated mmap record")
)

const (
	secMetadata = iota
	secStringPoolOffsets
	secStringPoolData
	secAllocationOffsets
	secAllocationData
	secRouteOffsets
	secRouteData
	secGeofeedOffsets
	secGeofeedData
	secAutnumIndex
	secAutnumData
	secASNRouteIndex
	secASNRouteIDs
	secAllocLPMv4
	secAllocLPMv6
	secGeoLPMv4
	secGeoLPMv6
	secRouteLPMv4
	secRouteLPMv6
	secAllocPrefixV4
	secAllocPrefixV6
	secRoutePrefixV4
	secRoutePrefixV6
	secGeofeedPrefixV4
	secGeofeedPrefixV6
	secAllocRangeV4
	secAllocRangeV6
	secRouteRangeV4
	secRouteRangeV6
	secSummaryIndex
	secSummaryData
)

type sectionSpec struct {
	offset uint64
	size   uint64
}

// Address stores IPv6 bytes directly and IPv4 bytes in the lower four bytes so
// every address uses one fixed 16-byte big-endian layout.
type Address [16]byte

func AddressFromAddr(addr netip.Addr) Address {
	if addr.Is4() {
		var result Address
		v4 := addr.As4()
		copy(result[12:], v4[:])
		return result
	}
	return addr.As16()
}

func (a Address) Addr(version int) netip.Addr {
	if version == 4 {
		return netip.AddrFrom4([4]byte(a[12:]))
	}
	return netip.AddrFrom16(a)
}

type Allocation struct {
	Version        uint8
	Start          Address
	End            Address
	Registry       string
	Country        string
	Name           string
	AllocationDate string
	Status         string
	Created        string
	LastModified   string
	Source         string
	Maintainers    string
	Organization   string
	AbuseContact   string
	Description    string
}

type Route struct {
	Version      uint8
	PrefixLength uint8
	Network      Address
	OriginASN    string
	ASNumber     uint32
	Registry     string
	Source       string
	Maintainers  string
	Organization string
	AbuseContact string
	Description  string
}

type Geofeed struct {
	Version uint8
	Start   Address
	End     Address
	Country string
	Region  string
	City    string
}

type Autnum struct {
	ASNumber     uint32
	ASN          string
	Registry     string
	Country      string
	Name         string
	Organization string
	Status       string
	Created      string
	LastModified string
	Source       string
	Maintainers  string
	AbuseContact string
	Description  string
}

type Facet struct {
	Value string
	Count uint64
}

type RangeSummary struct {
	PrefixLength      uint8
	AllocationRecords uint64
	Routes            uint64
	Countries         []Facet
	ASNs              []Facet
}

type Metadata struct {
	SchemaVersion uint32 `json:"schema_version"`
	ReleaseTag    string `json:"release_tag,omitempty"`
	BuiltAt       string `json:"built_at,omitempty"`
	SourceCommit  string `json:"source_commit,omitempty"`
}

type TableKind uint8

const (
	TableAllocations TableKind = iota + 1
	TableRoutes
	TableGeofeeds
)

type RangeIndexEntry struct {
	ID  uint32
	Key [RangeKeySize]byte
}

// DB maps a single immutable MNMF file into virtual memory for zero-copy reads.
type DB struct {
	path        string
	size        int64
	data        []byte
	meta        Metadata
	sec         [sectionCount][]byte
	poolOffsets []byte
	poolData    []byte
	poolCount   uint64
}

func Open(path string) (*DB, error) {
	file, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer file.Close()

	info, err := file.Stat()
	if err != nil {
		return nil, err
	}
	size := info.Size()
	if size < HeaderSize {
		return nil, fmt.Errorf("mmap file %s is too small (%d bytes)", path, size)
	}
	maxInt := int64(int(^uint(0) >> 1))
	if size > maxInt {
		return nil, fmt.Errorf("mmap file %s exceeds addressable size", path)
	}

	mapped, err := unix.Mmap(int(file.Fd()), 0, int(size), unix.PROT_READ, unix.MAP_SHARED)
	if err != nil {
		return nil, fmt.Errorf("mmap %s: %w", path, err)
	}

	db := &DB{path: path, size: size, data: mapped}
	if err := db.validate(); err != nil {
		_ = unix.Munmap(mapped)
		return nil, err
	}
	return db, nil
}

func (db *DB) Close() error {
	if db == nil || db.data == nil {
		return nil
	}
	err := unix.Munmap(db.data)
	db.data = nil
	return err
}

func (db *DB) Size() int64 { return db.size }

func (db *DB) Metadata() Metadata { return db.meta }

func (db *DB) SummaryCount() int {
	return len(db.sec[secSummaryIndex]) / summaryIndexStride
}

func (db *DB) validate() error {
	data := db.data
	if !bytes.Equal(data[:4], Magic[:]) {
		return errors.New("invalid mmap database magic")
	}
	version := binary.BigEndian.Uint32(data[4:8])
	if version != FormatVersion {
		return fmt.Errorf("unsupported mmap format version %d", version)
	}
	count := binary.BigEndian.Uint32(data[8:12])
	if count != sectionCount {
		return fmt.Errorf("unexpected mmap section count %d", count)
	}
	total := uint64(len(data))
	for index := 0; index < sectionCount; index++ {
		pos := headerFixedSize + index*sectionEntrySize
		off := binary.BigEndian.Uint64(data[pos : pos+8])
		sz := binary.BigEndian.Uint64(data[pos+8 : pos+16])
		if off > total || sz > total-off {
			return fmt.Errorf("section %d out of bounds", index)
		}
		db.sec[index] = data[off : off+sz]
	}

	var meta Metadata
	if err := json.Unmarshal(db.sec[secMetadata], &meta); err != nil {
		return fmt.Errorf("decode metadata: %w", err)
	}
	if meta.SchemaVersion != SchemaVersion {
		return fmt.Errorf("unsupported schema version %d", meta.SchemaVersion)
	}
	db.meta = meta

	if len(db.sec[secStringPoolOffsets]) < 8 || len(db.sec[secStringPoolOffsets])%8 != 0 {
		return errors.New("invalid string pool offsets section")
	}
	db.poolOffsets = db.sec[secStringPoolOffsets]
	db.poolData = db.sec[secStringPoolData]
	db.poolCount = uint64(len(db.poolOffsets)/8) - 1
	if binary.BigEndian.Uint64(db.poolOffsets[db.poolCount*8:]) != uint64(len(db.poolData)) {
		return errors.New("string pool size mismatch")
	}

	for _, pair := range [][2]int{
		{secAllocationOffsets, secAllocationData},
		{secRouteOffsets, secRouteData},
		{secGeofeedOffsets, secGeofeedData},
	} {
		offs := db.sec[pair[0]]
		if len(offs) < 8 || len(offs)%8 != 0 {
			return fmt.Errorf("invalid offsets section %d", pair[0])
		}
		if binary.BigEndian.Uint64(offs[len(offs)-8:]) != uint64(len(db.sec[pair[1]])) {
			return fmt.Errorf("data length mismatch for section %d", pair[1])
		}
	}

	for _, check := range []struct {
		sec    int
		stride int
	}{
		{secAutnumIndex, autnumIndexStride},
		{secASNRouteIndex, asnRouteIndexStride},
		{secASNRouteIDs, 4},
		{secAllocPrefixV4, prefixV4Stride},
		{secAllocPrefixV6, prefixV6Stride},
		{secRoutePrefixV4, prefixV4Stride},
		{secRoutePrefixV6, prefixV6Stride},
		{secGeofeedPrefixV4, prefixV4Stride},
		{secGeofeedPrefixV6, prefixV6Stride},
		{secAllocRangeV4, rangeV4Stride},
		{secAllocRangeV6, rangeV6Stride},
		{secRouteRangeV4, rangeV4Stride},
		{secRouteRangeV6, rangeV6Stride},
		{secSummaryIndex, summaryIndexStride},
	} {
		if len(db.sec[check.sec])%check.stride != 0 {
			return fmt.Errorf("section %d misaligned to stride %d", check.sec, check.stride)
		}
	}

	for _, check := range []struct {
		sec     int
		version uint8
	}{
		{secAllocLPMv4, 4},
		{secAllocLPMv6, 6},
		{secGeoLPMv4, 4},
		{secGeoLPMv6, 6},
	} {
		if err := validateSelectionLPM(db.sec[check.sec], check.version); err != nil {
			return fmt.Errorf("section %d: %w", check.sec, err)
		}
	}
	for _, check := range []struct {
		sec     int
		version uint8
	}{
		{secRouteLPMv4, 4},
		{secRouteLPMv6, 6},
	} {
		if err := validateRouteLPM(db.sec[check.sec], check.version); err != nil {
			return fmt.Errorf("section %d: %w", check.sec, err)
		}
	}
	return nil
}

// CompactSelectorBytes returns the combined size of the six LPM selector
// sections directly mapped from the file.
func (db *DB) CompactSelectorBytes() int64 {
	return int64(len(db.sec[secAllocLPMv4]) + len(db.sec[secAllocLPMv6]) +
		len(db.sec[secGeoLPMv4]) + len(db.sec[secGeoLPMv6]) +
		len(db.sec[secRouteLPMv4]) + len(db.sec[secRouteLPMv6]))
}

// WarmCompactSelectors advises the kernel to page in only the six LPM sections
// into the shared OS page cache without allocating Go heap memory.
func (db *DB) WarmCompactSelectors() (int64, error) {
	var total int64
	for _, idx := range []int{
		secAllocLPMv4, secAllocLPMv6,
		secGeoLPMv4, secGeoLPMv6,
		secRouteLPMv4, secRouteLPMv6,
	} {
		slice := db.sec[idx]
		total += int64(len(slice))
		if len(slice) == 0 {
			continue
		}
		_ = unix.Madvise(slice, unix.MADV_WILLNEED)
		var sink byte
		for off := 0; off < len(slice); off += 4096 {
			sink ^= slice[off]
		}
		sink ^= slice[len(slice)-1]
		_ = sink
	}
	return total, nil
}

// WarmDataset sequentially reads the backing file through the kernel page cache
// with a bounded buffer so no heap copy of the database is created.
func (db *DB) WarmDataset(ctx context.Context) (int64, error) {
	file, err := os.Open(db.path)
	if err != nil {
		return 0, fmt.Errorf("open mmap database for warmup: %w", err)
	}
	defer file.Close()

	buffer := make([]byte, warmBufferSize)
	var warmed int64
	for {
		if err := contextError(ctx); err != nil {
			return warmed, err
		}
		n, readErr := file.Read(buffer)
		warmed += int64(n)
		if readErr == io.EOF {
			return warmed, nil
		}
		if readErr != nil {
			return warmed, fmt.Errorf("warm mmap dataset page cache: %w", readErr)
		}
	}
}

func (db *DB) LookupAllocationLPM(addr netip.Addr) (uint32, error) {
	if addr.Is4() {
		return LookupSelectionLPM(db.sec[secAllocLPMv4], addr)
	}
	return LookupSelectionLPM(db.sec[secAllocLPMv6], addr)
}

func (db *DB) LookupGeofeedLPM(addr netip.Addr) (uint32, error) {
	if addr.Is4() {
		return LookupSelectionLPM(db.sec[secGeoLPMv4], addr)
	}
	return LookupSelectionLPM(db.sec[secGeoLPMv6], addr)
}

func (db *DB) LookupRouteLPM(addr netip.Addr) ([]uint32, error) {
	if addr.Is4() {
		return LookupRouteLPM(db.sec[secRouteLPMv4], addr)
	}
	return LookupRouteLPM(db.sec[secRouteLPMv6], addr)
}

func (db *DB) recordSlice(offsetsSec, dataSec int, id uint32) ([]byte, error) {
	if id == 0 {
		return nil, errors.New("record ID 0 is invalid")
	}
	offs := db.sec[offsetsSec]
	count := uint64(len(offs)/8) - 1
	idx := uint64(id - 1)
	if idx >= count {
		return nil, fmt.Errorf("record ID %d out of range", id)
	}
	start := binary.BigEndian.Uint64(offs[idx*8:])
	end := binary.BigEndian.Uint64(offs[(idx+1)*8:])
	data := db.sec[dataSec]
	if start > end || end > uint64(len(data)) {
		return nil, errTruncatedSlice
	}
	return data[start:end], nil
}

func (db *DB) Allocation(id uint32) (Allocation, error) {
	raw, err := db.recordSlice(secAllocationOffsets, secAllocationData, id)
	if err != nil {
		return Allocation{}, fmt.Errorf("allocation %d: %w", id, err)
	}
	return db.decodeAllocation(raw, false)
}

func (db *DB) AllocationLookup(id uint32) (Allocation, error) {
	raw, err := db.recordSlice(secAllocationOffsets, secAllocationData, id)
	if err != nil {
		return Allocation{}, fmt.Errorf("allocation %d: %w", id, err)
	}
	return db.decodeAllocation(raw, true)
}

func (db *DB) Route(id uint32) (Route, error) {
	raw, err := db.recordSlice(secRouteOffsets, secRouteData, id)
	if err != nil {
		return Route{}, fmt.Errorf("route %d: %w", id, err)
	}
	return db.decodeRoute(raw, false)
}

func (db *DB) RouteLookup(id uint32) (Route, error) {
	raw, err := db.recordSlice(secRouteOffsets, secRouteData, id)
	if err != nil {
		return Route{}, fmt.Errorf("route %d: %w", id, err)
	}
	return db.decodeRoute(raw, true)
}

func (db *DB) Geofeed(id uint32) (Geofeed, error) {
	raw, err := db.recordSlice(secGeofeedOffsets, secGeofeedData, id)
	if err != nil {
		return Geofeed{}, fmt.Errorf("geofeed %d: %w", id, err)
	}
	return db.decodeGeofeed(raw)
}

func (db *DB) Autnum(asn uint32) (Autnum, bool, error) {
	return db.lookupAutnum(asn, false)
}

func (db *DB) AutnumIdentity(asn uint32) (Autnum, bool, error) {
	return db.lookupAutnum(asn, true)
}

func (db *DB) lookupAutnum(asn uint32, identityOnly bool) (Autnum, bool, error) {
	index := db.sec[secAutnumIndex]
	n := len(index) / autnumIndexStride
	pos := sort.Search(n, func(i int) bool {
		return binary.BigEndian.Uint32(index[i*autnumIndexStride:]) >= asn
	})
	if pos >= n {
		return Autnum{}, false, nil
	}
	entry := index[pos*autnumIndexStride:]
	if binary.BigEndian.Uint32(entry[:4]) != asn {
		return Autnum{}, false, nil
	}
	off := binary.BigEndian.Uint64(entry[4:12])
	length := uint64(binary.BigEndian.Uint32(entry[12:16]))
	data := db.sec[secAutnumData]
	if off > uint64(len(data)) || length > uint64(len(data))-off {
		return Autnum{}, false, errTruncatedSlice
	}
	autnum, err := db.decodeAutnum(data[off:off+length], identityOnly)
	if err != nil {
		return Autnum{}, false, err
	}
	return autnum, true, nil
}

// ASNRouteSlice returns total routes for the ASN plus the requested slice of
// ascending route IDs. When afterCursor > 0, it binary-searches past that ID.
func (db *DB) ASNRouteSlice(asn uint32, numbered bool, pageNumber int, cursor int64, limit int) (total int64, ids []uint32, err error) {
	index := db.sec[secASNRouteIndex]
	n := len(index) / asnRouteIndexStride
	pos := sort.Search(n, func(i int) bool {
		return binary.BigEndian.Uint32(index[i*asnRouteIndexStride:]) >= asn
	})
	if pos >= n {
		return 0, nil, nil
	}
	entry := index[pos*asnRouteIndexStride:]
	if binary.BigEndian.Uint32(entry[:4]) != asn {
		return 0, nil, nil
	}
	firstIdx := binary.BigEndian.Uint64(entry[4:12])
	count := uint64(binary.BigEndian.Uint32(entry[12:16]))
	rawIDs := db.sec[secASNRouteIDs]
	maxEntries := uint64(len(rawIDs) / 4)
	if firstIdx > maxEntries || count > maxEntries-firstIdx {
		return 0, nil, errTruncatedSlice
	}
	slice := rawIDs[firstIdx*4 : (firstIdx+count)*4]
	total = int64(count)
	start := 0
	if numbered {
		start64 := int64(pageNumber-1) * int64(limit)
		if start64 < total {
			start = int(start64)
		} else {
			start = int(total)
		}
	} else if cursor > 0 {
		start = sort.Search(int(total), func(i int) bool {
			return int64(binary.BigEndian.Uint32(slice[i*4:])) > cursor
		})
	}
	end := start + limit
	if !numbered {
		end++
	}
	if end > int(total) {
		end = int(total)
	}
	if end <= start {
		return total, nil, nil
	}
	out := make([]uint32, end-start)
	for i := range out {
		out[i] = binary.BigEndian.Uint32(slice[(start+i)*4:])
	}
	return total, out, nil
}

func (db *DB) Summary(prefix netip.Prefix) (RangeSummary, bool, error) {
	if !prefix.Addr().Is4() {
		return RangeSummary{}, false, nil
	}
	prefix = prefix.Masked()
	v4 := prefix.Addr().As4()
	var key [5]byte
	copy(key[:4], v4[:])
	key[4] = byte(prefix.Bits())

	index := db.sec[secSummaryIndex]
	n := len(index) / summaryIndexStride
	pos := sort.Search(n, func(i int) bool {
		off := i * summaryIndexStride
		return bytes.Compare(index[off:off+5], key[:]) >= 0
	})
	if pos >= n {
		return RangeSummary{}, false, nil
	}
	entry := index[pos*summaryIndexStride:]
	if !bytes.Equal(entry[:5], key[:]) {
		return RangeSummary{}, false, nil
	}
	off := binary.BigEndian.Uint64(entry[5:13])
	length := uint64(binary.BigEndian.Uint32(entry[13:17]))
	data := db.sec[secSummaryData]
	if off > uint64(len(data)) || length > uint64(len(data))-off {
		return RangeSummary{}, false, errTruncatedSlice
	}
	summary, err := decodeSummary(data[off : off+length])
	if err != nil {
		return RangeSummary{}, false, err
	}
	return summary, true, nil
}

// MatchingPrefixIDs returns every record ID whose decomposed CIDR prefix equals
// addr masked at each prefix length from host down to /0.
func (db *DB) MatchingPrefixIDs(ctx context.Context, kind TableKind, addr netip.Addr, limit int) ([]uint32, error) {
	table, version, addrBytes, stride := db.prefixTable(kind, addr.Is4())
	bits := 128
	if version == 4 {
		bits = 32
	}
	n := len(table) / stride
	keyLen := addrBytes + 1
	var target [17]byte
	var ids []uint32

	for length := bits; length >= 0 && (limit == 0 || len(ids) < limit); length-- {
		if err := contextError(ctx); err != nil {
			return nil, err
		}
		masked := netip.PrefixFrom(addr, length).Masked().Addr()
		if version == 4 {
			v4 := masked.As4()
			copy(target[:4], v4[:])
		} else {
			v6 := masked.As16()
			copy(target[:16], v6[:])
		}
		target[addrBytes] = byte(length)

		pos := sort.Search(n, func(i int) bool {
			off := i * stride
			return bytes.Compare(table[off:off+keyLen], target[:keyLen]) >= 0
		})
		for ; pos < n && (limit == 0 || len(ids) < limit); pos++ {
			off := pos * stride
			if !bytes.Equal(table[off:off+keyLen], target[:keyLen]) {
				break
			}
			ids = append(ids, binary.BigEndian.Uint32(table[off+keyLen:]))
		}
	}
	return ids, nil
}

// OverlapRouteIDs resolves sorted distinct route IDs overlapping [startAddr, endAddr].
func (db *DB) OverlapRouteIDs(ctx context.Context, startAddr, endAddr netip.Addr, after int64, scanLimit int) ([]uint32, error) {
	table, version, addrBytes, stride := db.prefixTable(TableRoutes, startAddr.Is4())
	bits := 128
	if version == 4 {
		bits = 32
	}
	n := len(table) / stride
	keyLen := addrBytes + 1
	ids := make(map[uint32]struct{})
	scanned := 0
	add := func(id uint32) {
		if int64(id) > after {
			ids[id] = struct{}{}
		}
	}

	var target [17]byte
	for length := bits; length >= 0; length-- {
		if err := contextError(ctx); err != nil {
			return nil, err
		}
		masked := netip.PrefixFrom(startAddr, length).Masked().Addr()
		writeAddrBytes(target[:addrBytes], masked, version)
		target[addrBytes] = byte(length)
		pos := sort.Search(n, func(i int) bool {
			off := i * stride
			return bytes.Compare(table[off:off+keyLen], target[:keyLen]) >= 0
		})
		for ; pos < n; pos++ {
			off := pos * stride
			if !bytes.Equal(table[off:off+keyLen], target[:keyLen]) {
				break
			}
			scanned++
			if scanned > scanLimit {
				return nil, ErrQueryTooBroad
			}
			add(binary.BigEndian.Uint32(table[off+keyLen:]))
		}
	}

	var startBytes, endBytes [16]byte
	writeAddrBytes(startBytes[:addrBytes], startAddr, version)
	writeAddrBytes(endBytes[:addrBytes], endAddr, version)

	pos := sort.Search(n, func(i int) bool {
		off := i * stride
		return bytes.Compare(table[off:off+addrBytes], startBytes[:addrBytes]) >= 0
	})
	for ; pos < n; pos++ {
		off := pos * stride
		if bytes.Compare(table[off:off+addrBytes], endBytes[:addrBytes]) > 0 {
			break
		}
		scanned++
		if scanned > scanLimit {
			return nil, ErrQueryTooBroad
		}
		add(binary.BigEndian.Uint32(table[off+keyLen:]))
		if len(ids)%4096 == 0 {
			if err := contextError(ctx); err != nil {
				return nil, err
			}
		}
	}

	result := make([]uint32, 0, len(ids))
	for id := range ids {
		result = append(result, id)
	}
	sort.Slice(result, func(left, right int) bool { return result[left] < result[right] })
	return result, nil
}

// DirectRangeEntries streams entries whose start falls within [start, end]
// ordered by (start, end, id), optionally strictly after a cursor key.
func (db *DB) DirectRangeEntries(ctx context.Context, kind TableKind, version uint8, start, end Address, after *[RangeKeySize]byte, maximum int) ([]RangeIndexEntry, bool, error) {
	if maximum < 1 {
		maximum = 1
	}
	table, addrBytes, stride := db.rangeTable(kind, version)
	n := len(table) / stride

	var seekStart, seekEnd, limitEnd []byte
	var seekID uint32
	if after != nil {
		if version == 4 {
			seekStart = after[13:17]
			seekEnd = after[29:33]
		} else {
			seekStart = after[1:17]
			seekEnd = after[17:33]
		}
		seekID = binary.BigEndian.Uint32(after[33:37])
	} else {
		if version == 4 {
			seekStart = start[12:16]
		} else {
			seekStart = start[:]
		}
	}
	if version == 4 {
		limitEnd = end[12:16]
	} else {
		limitEnd = end[:]
	}

	pos := sort.Search(n, func(i int) bool {
		off := i * stride
		cmp := bytes.Compare(table[off:off+addrBytes], seekStart)
		if cmp != 0 || after == nil {
			return cmp >= 0
		}
		cmpEnd := bytes.Compare(table[off+addrBytes:off+2*addrBytes], seekEnd)
		if cmpEnd != 0 {
			return cmpEnd > 0
		}
		return binary.BigEndian.Uint32(table[off+2*addrBytes:off+stride]) > seekID
	})

	entries := make([]RangeIndexEntry, 0, maximum)
	for ; pos < n; pos++ {
		if err := contextError(ctx); err != nil {
			return nil, false, err
		}
		off := pos * stride
		entryStart := table[off : off+addrBytes]
		if bytes.Compare(entryStart, limitEnd) > 0 {
			break
		}
		entryEnd := table[off+addrBytes : off+2*addrBytes]
		id := binary.BigEndian.Uint32(table[off+2*addrBytes : off+stride])
		var entry RangeIndexEntry
		entry.ID = id
		PutRangeKey(entry.Key[:], version, addressFromSlice(entryStart, version), addressFromSlice(entryEnd, version), id)
		entries = append(entries, entry)
		if len(entries) == maximum {
			return entries, true, nil
		}
	}
	return entries, false, nil
}

func (db *DB) prefixTable(kind TableKind, is4 bool) (table []byte, version uint8, addrBytes, stride int) {
	version, addrBytes, stride = 6, 16, prefixV6Stride
	if is4 {
		version, addrBytes, stride = 4, 4, prefixV4Stride
	}
	switch kind {
	case TableAllocations:
		if is4 {
			table = db.sec[secAllocPrefixV4]
		} else {
			table = db.sec[secAllocPrefixV6]
		}
	case TableRoutes:
		if is4 {
			table = db.sec[secRoutePrefixV4]
		} else {
			table = db.sec[secRoutePrefixV6]
		}
	case TableGeofeeds:
		if is4 {
			table = db.sec[secGeofeedPrefixV4]
		} else {
			table = db.sec[secGeofeedPrefixV6]
		}
	}
	return table, version, addrBytes, stride
}

func (db *DB) rangeTable(kind TableKind, version uint8) (table []byte, addrBytes, stride int) {
	if version == 4 {
		addrBytes, stride = 4, rangeV4Stride
		if kind == TableAllocations {
			return db.sec[secAllocRangeV4], addrBytes, stride
		}
		return db.sec[secRouteRangeV4], addrBytes, stride
	}
	addrBytes, stride = 16, rangeV6Stride
	if kind == TableAllocations {
		return db.sec[secAllocRangeV6], addrBytes, stride
	}
	return db.sec[secRouteRangeV6], addrBytes, stride
}

func PutRangeKey(key []byte, version uint8, start, end Address, id uint32) []byte {
	key[0] = version
	copy(key[1:17], start[:])
	copy(key[17:33], end[:])
	binary.BigEndian.PutUint32(key[33:37], id)
	return key[:RangeKeySize]
}

func RangeKeyStartKey(key []byte, version uint8, start Address) []byte {
	key[0] = version
	copy(key[1:17], start[:])
	clear(key[17:RangeKeySize])
	return key[:RangeKeySize]
}

func writeAddrBytes(dst []byte, addr netip.Addr, version uint8) {
	if version == 4 {
		v4 := addr.As4()
		copy(dst, v4[:])
		return
	}
	v6 := addr.As16()
	copy(dst, v6[:])
}

func addressFromSlice(src []byte, version uint8) Address {
	var out Address
	if version == 4 {
		copy(out[12:], src[:4])
		return out
	}
	copy(out[:], src[:16])
	return out
}

// ---- Record binary codecs ----

type stringPool struct {
	index   map[string]uint32
	offsets []uint64
	data    []byte
}

func newStringPool() *stringPool {
	return &stringPool{
		index:   make(map[string]uint32),
		offsets: []uint64{0},
	}
}

func (p *stringPool) encodePooled(dst []byte, value string) []byte {
	if value == "" {
		return append(dst, 0)
	}
	if id, ok := p.index[value]; ok {
		return binary.AppendUvarint(dst, uint64(id)+2)
	}
	if len(p.index) < maxPoolStrings {
		id := uint32(len(p.offsets) - 1)
		p.data = append(p.data, value...)
		p.offsets = append(p.offsets, uint64(len(p.data)))
		// Slice into pooled backing bytes when storing map key to avoid pinning CSV slices.
		key := string(p.data[p.offsets[id]:p.offsets[id+1]])
		p.index[key] = id
		return binary.AppendUvarint(dst, uint64(id)+2)
	}
	dst = append(dst, 1)
	dst = binary.AppendUvarint(dst, uint64(len(value)))
	return append(dst, value...)
}

func encodeInline(dst []byte, value string) []byte {
	dst = binary.AppendUvarint(dst, uint64(len(value)))
	return append(dst, value...)
}

func encodeCompactAddress(dst []byte, version uint8, addr Address) []byte {
	if version == 4 {
		return append(dst, addr[12:16]...)
	}
	return append(dst, addr[:]...)
}

func appendAllocation(dst []byte, pool *stringPool, v Allocation) []byte {
	dst = append(dst, v.Version)
	dst = encodeCompactAddress(dst, v.Version, v.Start)
	dst = encodeCompactAddress(dst, v.Version, v.End)
	// Compact lookup fields first:
	dst = pool.encodePooled(dst, v.Registry)
	dst = pool.encodePooled(dst, v.Country)
	dst = encodeInline(dst, v.Name)
	dst = pool.encodePooled(dst, v.AllocationDate)
	dst = pool.encodePooled(dst, v.Status)
	dst = pool.encodePooled(dst, v.AbuseContact)
	// Full/resource fields:
	dst = pool.encodePooled(dst, v.Source)
	dst = pool.encodePooled(dst, v.Maintainers)
	dst = pool.encodePooled(dst, v.Organization)
	dst = encodeInline(dst, v.Created)
	dst = encodeInline(dst, v.LastModified)
	dst = encodeInline(dst, v.Description)
	return dst
}

func appendRoute(dst []byte, pool *stringPool, v Route) []byte {
	dst = append(dst, v.Version, v.PrefixLength)
	dst = encodeCompactAddress(dst, v.Version, v.Network)
	dst = binary.AppendUvarint(dst, uint64(v.ASNumber))
	// Compact lookup fields first:
	dst = pool.encodePooled(dst, v.OriginASN)
	dst = pool.encodePooled(dst, v.AbuseContact)
	// Full/resource fields:
	dst = pool.encodePooled(dst, v.Registry)
	dst = pool.encodePooled(dst, v.Source)
	dst = pool.encodePooled(dst, v.Maintainers)
	dst = pool.encodePooled(dst, v.Organization)
	dst = encodeInline(dst, v.Description)
	return dst
}

func appendGeofeed(dst []byte, pool *stringPool, v Geofeed) []byte {
	dst = append(dst, v.Version)
	dst = encodeCompactAddress(dst, v.Version, v.Start)
	dst = encodeCompactAddress(dst, v.Version, v.End)
	dst = pool.encodePooled(dst, v.Country)
	dst = pool.encodePooled(dst, v.Region)
	dst = pool.encodePooled(dst, v.City)
	return dst
}

func appendAutnum(dst []byte, pool *stringPool, v Autnum) []byte {
	dst = binary.AppendUvarint(dst, uint64(v.ASNumber))
	// Compact identity fields first:
	dst = encodeInline(dst, v.Name)
	dst = pool.encodePooled(dst, v.Organization)
	// Full/resource fields:
	dst = pool.encodePooled(dst, v.ASN)
	dst = pool.encodePooled(dst, v.Registry)
	dst = pool.encodePooled(dst, v.Country)
	dst = pool.encodePooled(dst, v.Status)
	dst = pool.encodePooled(dst, v.Source)
	dst = pool.encodePooled(dst, v.Maintainers)
	dst = pool.encodePooled(dst, v.AbuseContact)
	dst = encodeInline(dst, v.Created)
	dst = encodeInline(dst, v.LastModified)
	dst = encodeInline(dst, v.Description)
	return dst
}

func appendSummary(dst []byte, v RangeSummary) []byte {
	dst = append(dst, v.PrefixLength)
	dst = binary.AppendUvarint(dst, v.AllocationRecords)
	dst = binary.AppendUvarint(dst, v.Routes)
	for _, facets := range [][]Facet{v.Countries, v.ASNs} {
		dst = binary.AppendUvarint(dst, uint64(len(facets)))
		for _, facet := range facets {
			dst = encodeInline(dst, facet.Value)
			dst = binary.AppendUvarint(dst, facet.Count)
		}
	}
	return dst
}

type byteDecoder struct {
	buf []byte
	pos int
}

func (d *byteDecoder) byte() (byte, error) {
	if d.pos >= len(d.buf) {
		return 0, errTruncatedSlice
	}
	v := d.buf[d.pos]
	d.pos++
	return v, nil
}

func (d *byteDecoder) uvarint() (uint64, error) {
	v, n := binary.Uvarint(d.buf[d.pos:])
	if n <= 0 {
		return 0, errTruncatedSlice
	}
	d.pos += n
	return v, nil
}

func (d *byteDecoder) address(version uint8) (Address, error) {
	var out Address
	if version == 4 {
		if d.pos+4 > len(d.buf) {
			return Address{}, errTruncatedSlice
		}
		copy(out[12:], d.buf[d.pos:d.pos+4])
		d.pos += 4
		return out, nil
	}
	if d.pos+16 > len(d.buf) {
		return Address{}, errTruncatedSlice
	}
	copy(out[:], d.buf[d.pos:d.pos+16])
	d.pos += 16
	return out, nil
}

func (d *byteDecoder) inlineString() (string, error) {
	length, err := d.uvarint()
	if err != nil {
		return "", err
	}
	if length == 0 {
		return "", nil
	}
	if length > uint64(len(d.buf)-d.pos) {
		return "", errTruncatedSlice
	}
	s := string(d.buf[d.pos : d.pos+int(length)])
	d.pos += int(length)
	return s, nil
}

func (d *byteDecoder) pooledString(db *DB) (string, error) {
	tag, err := d.uvarint()
	if err != nil {
		return "", err
	}
	switch tag {
	case 0:
		return "", nil
	case 1:
		return d.inlineString()
	default:
		id := tag - 2
		if id >= db.poolCount {
			return "", errTruncatedSlice
		}
		start := binary.BigEndian.Uint64(db.poolOffsets[id*8:])
		end := binary.BigEndian.Uint64(db.poolOffsets[(id+1)*8:])
		if start > end || end > uint64(len(db.poolData)) {
			return "", errTruncatedSlice
		}
		return string(db.poolData[start:end]), nil
	}
}

func (d *byteDecoder) done() error {
	if d.pos != len(d.buf) {
		return errors.New("trailing bytes in mmap record")
	}
	return nil
}

func (db *DB) decodeAllocation(raw []byte, lookupOnly bool) (Allocation, error) {
	d := byteDecoder{buf: raw}
	var out Allocation
	var err error
	if out.Version, err = d.byte(); err != nil {
		return out, err
	}
	if out.Start, err = d.address(out.Version); err != nil {
		return out, err
	}
	if out.End, err = d.address(out.Version); err != nil {
		return out, err
	}
	if out.Registry, err = d.pooledString(db); err != nil {
		return out, err
	}
	if out.Country, err = d.pooledString(db); err != nil {
		return out, err
	}
	if out.Name, err = d.inlineString(); err != nil {
		return out, err
	}
	if out.AllocationDate, err = d.pooledString(db); err != nil {
		return out, err
	}
	if out.Status, err = d.pooledString(db); err != nil {
		return out, err
	}
	if out.AbuseContact, err = d.pooledString(db); err != nil {
		return out, err
	}
	if lookupOnly {
		return out, nil
	}
	if out.Source, err = d.pooledString(db); err != nil {
		return out, err
	}
	if out.Maintainers, err = d.pooledString(db); err != nil {
		return out, err
	}
	if out.Organization, err = d.pooledString(db); err != nil {
		return out, err
	}
	if out.Created, err = d.inlineString(); err != nil {
		return out, err
	}
	if out.LastModified, err = d.inlineString(); err != nil {
		return out, err
	}
	if out.Description, err = d.inlineString(); err != nil {
		return out, err
	}
	return out, d.done()
}

func (db *DB) decodeRoute(raw []byte, lookupOnly bool) (Route, error) {
	d := byteDecoder{buf: raw}
	var out Route
	var err error
	if out.Version, err = d.byte(); err != nil {
		return out, err
	}
	if out.PrefixLength, err = d.byte(); err != nil {
		return out, err
	}
	if out.Network, err = d.address(out.Version); err != nil {
		return out, err
	}
	num, err := d.uvarint()
	if err != nil {
		return out, err
	}
	out.ASNumber = uint32(num)
	if out.OriginASN, err = d.pooledString(db); err != nil {
		return out, err
	}
	if out.AbuseContact, err = d.pooledString(db); err != nil {
		return out, err
	}
	if lookupOnly {
		return out, nil
	}
	if out.Registry, err = d.pooledString(db); err != nil {
		return out, err
	}
	if out.Source, err = d.pooledString(db); err != nil {
		return out, err
	}
	if out.Maintainers, err = d.pooledString(db); err != nil {
		return out, err
	}
	if out.Organization, err = d.pooledString(db); err != nil {
		return out, err
	}
	if out.Description, err = d.inlineString(); err != nil {
		return out, err
	}
	return out, d.done()
}

func (db *DB) decodeGeofeed(raw []byte) (Geofeed, error) {
	d := byteDecoder{buf: raw}
	var out Geofeed
	var err error
	if out.Version, err = d.byte(); err != nil {
		return out, err
	}
	if out.Start, err = d.address(out.Version); err != nil {
		return out, err
	}
	if out.End, err = d.address(out.Version); err != nil {
		return out, err
	}
	if out.Country, err = d.pooledString(db); err != nil {
		return out, err
	}
	if out.Region, err = d.pooledString(db); err != nil {
		return out, err
	}
	if out.City, err = d.pooledString(db); err != nil {
		return out, err
	}
	return out, d.done()
}

func (db *DB) decodeAutnum(raw []byte, identityOnly bool) (Autnum, error) {
	d := byteDecoder{buf: raw}
	var out Autnum
	num, err := d.uvarint()
	if err != nil {
		return out, err
	}
	out.ASNumber = uint32(num)
	if out.Name, err = d.inlineString(); err != nil {
		return out, err
	}
	if out.Organization, err = d.pooledString(db); err != nil {
		return out, err
	}
	if identityOnly {
		return out, nil
	}
	if out.ASN, err = d.pooledString(db); err != nil {
		return out, err
	}
	if out.Registry, err = d.pooledString(db); err != nil {
		return out, err
	}
	if out.Country, err = d.pooledString(db); err != nil {
		return out, err
	}
	if out.Status, err = d.pooledString(db); err != nil {
		return out, err
	}
	if out.Source, err = d.pooledString(db); err != nil {
		return out, err
	}
	if out.Maintainers, err = d.pooledString(db); err != nil {
		return out, err
	}
	if out.AbuseContact, err = d.pooledString(db); err != nil {
		return out, err
	}
	if out.Created, err = d.inlineString(); err != nil {
		return out, err
	}
	if out.LastModified, err = d.inlineString(); err != nil {
		return out, err
	}
	if out.Description, err = d.inlineString(); err != nil {
		return out, err
	}
	return out, d.done()
}

func decodeSummary(raw []byte) (RangeSummary, error) {
	d := byteDecoder{buf: raw}
	var out RangeSummary
	var err error
	if out.PrefixLength, err = d.byte(); err != nil {
		return out, err
	}
	if out.AllocationRecords, err = d.uvarint(); err != nil {
		return out, err
	}
	if out.Routes, err = d.uvarint(); err != nil {
		return out, err
	}
	for _, dst := range []*[]Facet{&out.Countries, &out.ASNs} {
		count, err := d.uvarint()
		if err != nil {
			return out, err
		}
		if count > 1_000_000 {
			return out, fmt.Errorf("invalid facet count %d", count)
		}
		items := make([]Facet, 0, count)
		for i := uint64(0); i < count; i++ {
			val, err := d.inlineString()
			if err != nil {
				return out, err
			}
			cnt, err := d.uvarint()
			if err != nil {
				return out, err
			}
			items = append(items, Facet{Value: val, Count: cnt})
		}
		*dst = items
	}
	return out, d.done()
}

func contextError(ctx context.Context) error {
	select {
	case <-ctx.Done():
		return ctx.Err()
	default:
		return nil
	}
}
