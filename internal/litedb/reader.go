package litedb

import (
	"encoding/binary"
	"encoding/json"
	"errors"
	"fmt"
	"net/netip"
	"sort"
)

var ErrFormat = errors.New("litedb: invalid or truncated database")

type DB struct {
	data       []byte
	flags      byte
	blockSize  int
	allocV4B   int
	allocV6B   int
	routeV4B   int
	routeV6B   int
	meta       Metadata
	countries  []byte
	registries []string
	statuses   []string
	sources    []string
	regions    []string
	cities     []string

	geoLocCount      int
	geoLocOffsetsOff int
	geoLocDataOff    int

	asnCount      int
	asnNumbersOff int
	asnOffsetsOff int
	asnDataOff    int

	multiCount      int
	multiOffsetsOff int
	multiDataOff    int

	allocV4, allocV6 segTable
	geoV4, geoV6     segTable
	routeV4, routeV6 segTable

	asnRouteOffsetsOff int
	asnRouteDataOff    int
}

type segTable struct {
	bits     int
	entries  int
	indexOff int
	dataOff  int
	dataLen  int
}

type AutnumRecord struct {
	ASNumber     uint32
	Registry     string
	Country      string
	Name         string
	Organization string
	Status       string
	Source       string
	Maintainers  string
	AbuseContact string
	Description  string
}

type IPResult struct {
	Found       bool
	Registry    string
	AllocCC     string
	AllocStart  netip.Addr
	AllocEnd    netip.Addr
	HasAlloc    bool
	GeoCC       string
	GeoRegion   string
	GeoCity     string
	HasGeo      bool
	HasRoute    bool
	RoutePrefix netip.Prefix
	RouteASNs   []AutnumRecord
}

type ASNResult struct {
	Found      bool
	ASNumber   uint32
	HasAutnum  bool
	Autnum     AutnumRecord
	Routes     []netip.Prefix
	TotalRoute int
}

func readUvarintAt(data []byte, pos, end int) (uint64, int, bool) {
	if pos >= end {
		return 0, pos, false
	}
	v, n := binary.Uvarint(data[pos:end])
	if n <= 0 {
		return 0, pos, false
	}
	return v, pos + n, true
}

func readStringDict(data []byte, pos int) ([]string, int, error) {
	count, p, ok := readUvarintAt(data, pos, len(data))
	if !ok || count > 1_000_000 {
		return nil, 0, ErrFormat
	}
	out := make([]string, count)
	for i := uint64(0); i < count; i++ {
		ln, p2, ok := readUvarintAt(data, p, len(data))
		if !ok || p2+int(ln) > len(data) {
			return nil, 0, ErrFormat
		}
		out[i] = string(data[p2 : p2+int(ln)])
		p = p2 + int(ln)
	}
	return out, p, nil
}

func openSegTable(data []byte, pos, bits, blockSize int) (segTable, int, error) {
	if pos+8 > len(data) {
		return segTable{}, 0, ErrFormat
	}
	entries := int(binary.LittleEndian.Uint32(data[pos:]))
	dataLen := int(binary.LittleEndian.Uint32(data[pos+4:]))
	pos += 8
	if entries < 1 {
		return segTable{}, 0, ErrFormat
	}
	kb := (bits + 7) / 8
	blocks := (entries + blockSize - 1) / blockSize
	indexLen := blocks * (kb + 4)
	if pos+indexLen+dataLen > len(data) {
		return segTable{}, 0, ErrFormat
	}
	return segTable{
		bits:     bits,
		entries:  entries,
		indexOff: pos,
		dataOff:  pos + indexLen,
		dataLen:  dataLen,
	}, pos + indexLen + dataLen, nil
}

// Open validates and indexes an MNL1 byte slice in place.
func Open(data []byte) (*DB, error) {
	if len(data) < 13 || string(data[:4]) != magic || data[4] != formatVersion {
		return nil, ErrFormat
	}
	db := &DB{
		data:      data,
		flags:     data[5],
		blockSize: int(data[6]),
		allocV4B:  int(data[7]),
		allocV6B:  int(data[8]),
		routeV4B:  int(data[9]),
		routeV6B:  int(data[10]),
	}
	if db.blockSize < 1 || db.allocV4B < 1 || db.allocV4B > 32 || db.allocV6B < 1 || db.allocV6B > 64 ||
		db.routeV4B < 1 || db.routeV4B > 32 || db.routeV6B < 1 || db.routeV6B > 64 {
		return nil, ErrFormat
	}
	metaLen := int(binary.LittleEndian.Uint16(data[11:13]))
	pos := 13
	if pos+metaLen+2 > len(data) {
		return nil, ErrFormat
	}
	if metaLen > 0 {
		if err := json.Unmarshal(data[pos:pos+metaLen], &db.meta); err != nil {
			return nil, ErrFormat
		}
	}
	pos += metaLen

	ccCount := int(binary.LittleEndian.Uint16(data[pos : pos+2]))
	pos += 2
	if pos+2*ccCount > len(data) {
		return nil, ErrFormat
	}
	db.countries = data[pos : pos+2*ccCount]
	pos += 2 * ccCount

	var err error
	if db.registries, pos, err = readStringDict(data, pos); err != nil {
		return nil, err
	}
	if db.statuses, pos, err = readStringDict(data, pos); err != nil {
		return nil, err
	}
	if db.sources, pos, err = readStringDict(data, pos); err != nil {
		return nil, err
	}
	if db.regions, pos, err = readStringDict(data, pos); err != nil {
		return nil, err
	}
	if db.cities, pos, err = readStringDict(data, pos); err != nil {
		return nil, err
	}

	if pos+4 > len(data) {
		return nil, ErrFormat
	}
	db.geoLocCount = int(binary.LittleEndian.Uint32(data[pos:]))
	pos += 4
	db.geoLocOffsetsOff = pos
	pos += (db.geoLocCount + 1) * 4
	if pos > len(data) {
		return nil, ErrFormat
	}
	db.geoLocDataOff = pos
	geoLocDataLen := int(binary.LittleEndian.Uint32(data[db.geoLocOffsetsOff+db.geoLocCount*4:]))
	pos += geoLocDataLen

	if pos+4 > len(data) {
		return nil, ErrFormat
	}
	db.asnCount = int(binary.LittleEndian.Uint32(data[pos:]))
	pos += 4
	db.asnNumbersOff = pos
	pos += db.asnCount * 4
	db.asnOffsetsOff = pos
	pos += (db.asnCount + 1) * 4
	if pos > len(data) {
		return nil, ErrFormat
	}
	db.asnDataOff = pos
	asnDataLen := int(binary.LittleEndian.Uint32(data[db.asnOffsetsOff+db.asnCount*4:]))
	pos += asnDataLen
	if pos+4 > len(data) {
		return nil, ErrFormat
	}

	db.multiCount = int(binary.LittleEndian.Uint32(data[pos:]))
	pos += 4
	db.multiOffsetsOff = pos
	pos += (db.multiCount + 1) * 4
	if pos > len(data) {
		return nil, ErrFormat
	}
	db.multiDataOff = pos
	multiDataLen := int(binary.LittleEndian.Uint32(data[db.multiOffsetsOff+db.multiCount*4:]))
	pos += multiDataLen

	if db.allocV4, pos, err = openSegTable(data, pos, db.allocV4B, db.blockSize); err != nil {
		return nil, err
	}
	if db.allocV6, pos, err = openSegTable(data, pos, db.allocV6B, db.blockSize); err != nil {
		return nil, err
	}
	if db.geoV4, pos, err = openSegTable(data, pos, db.allocV4B, db.blockSize); err != nil {
		return nil, err
	}
	if db.geoV6, pos, err = openSegTable(data, pos, db.allocV6B, db.blockSize); err != nil {
		return nil, err
	}
	if db.routeV4, pos, err = openSegTable(data, pos, db.routeV4B, db.blockSize); err != nil {
		return nil, err
	}
	if db.routeV6, pos, err = openSegTable(data, pos, db.routeV6B, db.blockSize); err != nil {
		return nil, err
	}

	if db.flags&flagHasASNRoutes != 0 {
		db.asnRouteOffsetsOff = pos
		pos += (db.asnCount + 1) * 4
		if pos > len(data) {
			return nil, ErrFormat
		}
		db.asnRouteDataOff = pos
		rLen := int(binary.LittleEndian.Uint32(data[db.asnRouteOffsetsOff+db.asnCount*4:]))
		if pos+rLen > len(data) {
			return nil, ErrFormat
		}
	}
	return db, nil
}

func (db *DB) Metadata() Metadata { return db.meta }

func (db *DB) country(idx uint64) string {
	if idx == 0 || int(idx) > len(db.countries)/2 {
		return ""
	}
	i := (int(idx) - 1) * 2
	return string(db.countries[i : i+2])
}

func dictAt(list []string, idx uint64) string {
	if idx == 0 || int(idx) > len(list) {
		return ""
	}
	return list[idx-1]
}

func getKeyAt(data []byte, off, kb int) uint64 {
	var v uint64
	for i := kb - 1; i >= 0; i-- {
		v = v<<8 | uint64(data[off+i])
	}
	return v
}

// findSeg scans the block for key and returns payload bytes [pStart, pEnd) plus
// the segment's [segStart, nextStart) slot bounds (hasNext=false for the final entry).
func (db *DB) findSeg(t segTable, key uint64, skipPayload func(pos, end int) (int, bool)) (
	segStart, nextStart uint64, hasNext bool, pStart, pEnd int, ok bool,
) {
	kb := (t.bits + 7) / 8
	stride := kb + 4
	blocks := (t.entries + db.blockSize - 1) / db.blockSize
	b := sort.Search(blocks, func(i int) bool {
		return getKeyAt(db.data, t.indexOff+i*stride, kb) > key
	}) - 1
	if b < 0 {
		return 0, 0, false, 0, 0, false
	}
	curKey := getKeyAt(db.data, t.indexOff+b*stride, kb)
	off := t.dataOff + int(binary.LittleEndian.Uint32(db.data[t.indexOff+b*stride+kb:]))
	end := t.dataOff + t.dataLen
	if b+1 < blocks {
		end = t.dataOff + int(binary.LittleEndian.Uint32(db.data[t.indexOff+(b+1)*stride+kb:]))
	}
	var prevStart uint64
	var prevPStart, prevPEnd int
	found := false
	for off < end {
		delta, p, okDelta := readUvarintAt(db.data, off, end)
		if !okDelta {
			return 0, 0, false, 0, 0, false
		}
		curKey += delta
		nextOff, okPay := skipPayload(p, end)
		if !okPay {
			return 0, 0, false, 0, 0, false
		}
		if curKey > key {
			if !found {
				return 0, 0, false, 0, 0, false
			}
			return prevStart, curKey, true, prevPStart, prevPEnd, true
		}
		prevStart = curKey
		prevPStart, prevPEnd = p, nextOff
		found = true
		off = nextOff
	}
	if !found {
		return 0, 0, false, 0, 0, false
	}
	if b+1 < blocks {
		nextBlockKey := getKeyAt(db.data, t.indexOff+(b+1)*stride, kb)
		return prevStart, nextBlockKey, true, prevPStart, prevPEnd, true
	}
	return prevStart, 0, false, prevPStart, prevPEnd, true
}

func (db *DB) skipAlloc(pos, end int) (int, bool) {
	_, p1, ok1 := readUvarintAt(db.data, pos, end)
	if !ok1 {
		return 0, false
	}
	_, p2, ok2 := readUvarintAt(db.data, p1, end)
	return p2, ok2
}

func (db *DB) skipGeo(pos, end int) (int, bool) {
	_, p1, ok := readUvarintAt(db.data, pos, end)
	return p1, ok
}

func (db *DB) skipRoute(pos, end int) (int, bool) {
	if pos >= end {
		return 0, false
	}
	bits := db.data[pos]
	pos++
	if bits == 0 {
		return pos, true
	}
	_, p1, ok := readUvarintAt(db.data, pos, end)
	return p1, ok
}

func (db *DB) autnumByIdx(idx int) (AutnumRecord, bool) {
	if idx < 1 || idx > db.asnCount {
		return AutnumRecord{}, false
	}
	asn := binary.LittleEndian.Uint32(db.data[db.asnNumbersOff+(idx-1)*4:])
	off := db.asnDataOff + int(binary.LittleEndian.Uint32(db.data[db.asnOffsetsOff+(idx-1)*4:]))
	end := db.asnDataOff + int(binary.LittleEndian.Uint32(db.data[db.asnOffsetsOff+idx*4:]))
	if off >= end || end > len(db.data) {
		return AutnumRecord{ASNumber: asn}, false
	}
	if db.data[off] == 0 {
		return AutnumRecord{ASNumber: asn}, false
	}
	p := off + 1
	regID, p, ok1 := readUvarintAt(db.data, p, end)
	ccID, p, ok2 := readUvarintAt(db.data, p, end)
	stID, p, ok3 := readUvarintAt(db.data, p, end)
	srcID, p, ok4 := readUvarintAt(db.data, p, end)
	nLen, p, ok5 := readUvarintAt(db.data, p, end)
	if !ok1 || !ok2 || !ok3 || !ok4 || !ok5 || p+int(nLen) > end {
		return AutnumRecord{ASNumber: asn}, false
	}
	name := string(db.data[p : p+int(nLen)])
	p += int(nLen)
	oLen, p, ok6 := readUvarintAt(db.data, p, end)
	if !ok6 || p+int(oLen) > end {
		return AutnumRecord{ASNumber: asn}, false
	}
	org := string(db.data[p : p+int(oLen)])
	p += int(oLen)
	mLen, p, ok7 := readUvarintAt(db.data, p, end)
	if !ok7 || p+int(mLen) > end {
		return AutnumRecord{ASNumber: asn}, false
	}
	maintainers := string(db.data[p : p+int(mLen)])
	p += int(mLen)
	aLen, p, ok8 := readUvarintAt(db.data, p, end)
	if !ok8 || p+int(aLen) > end {
		return AutnumRecord{ASNumber: asn}, false
	}
	abuse := string(db.data[p : p+int(aLen)])
	p += int(aLen)
	dLen, p, ok9 := readUvarintAt(db.data, p, end)
	if !ok9 || p+int(dLen) > end {
		return AutnumRecord{ASNumber: asn}, false
	}
	desc := string(db.data[p : p+int(dLen)])
	return AutnumRecord{
		ASNumber:     asn,
		Registry:     dictAt(db.registries, regID),
		Country:      db.country(ccID),
		Name:         name,
		Organization: org,
		Status:       dictAt(db.statuses, stID),
		Source:       dictAt(db.sources, srcID),
		Maintainers:  maintainers,
		AbuseContact: abuse,
		Description:  desc,
	}, true
}

func (db *DB) resolveASNSet(val uint64) []AutnumRecord {
	if val == 0 {
		return nil
	}
	if val <= uint64(db.asnCount) {
		rec, _ := db.autnumByIdx(int(val))
		return []AutnumRecord{rec}
	}
	mIdx := int(val) - db.asnCount - 1
	if mIdx < 0 || mIdx >= db.multiCount {
		return nil
	}
	off := db.multiDataOff + int(binary.LittleEndian.Uint32(db.data[db.multiOffsetsOff+mIdx*4:]))
	end := db.multiDataOff + int(binary.LittleEndian.Uint32(db.data[db.multiOffsetsOff+(mIdx+1)*4:]))
	count, p, ok := readUvarintAt(db.data, off, end)
	if !ok {
		return nil
	}
	out := make([]AutnumRecord, 0, count)
	for i := uint64(0); i < count; i++ {
		idx, p2, ok := readUvarintAt(db.data, p, end)
		if !ok {
			break
		}
		p = p2
		rec, _ := db.autnumByIdx(int(idx))
		out = append(out, rec)
	}
	return out
}

func slotBoundsToAddrs(isV6 bool, bits int, startSlot, nextSlot uint64, hasNext bool) (netip.Addr, netip.Addr) {
	if !isV6 {
		shift := uint(32 - bits)
		s := uint32(startSlot << shift)
		var e uint32 = 0xffffffff
		if hasNext {
			e = uint32((nextSlot << shift) - 1)
		}
		var a, b [4]byte
		binary.BigEndian.PutUint32(a[:], s)
		binary.BigEndian.PutUint32(b[:], e)
		return netip.AddrFrom4(a), netip.AddrFrom4(b)
	}
	shift := uint(64 - bits)
	sHi := startSlot << shift
	eHi := ^uint64(0)
	if hasNext {
		eHi = (nextSlot << shift) - 1
	}
	var a, b [16]byte
	binary.BigEndian.PutUint64(a[:8], sHi)
	binary.BigEndian.PutUint64(b[:8], eHi)
	binary.BigEndian.PutUint64(b[8:], ^uint64(0))
	return netip.AddrFrom16(a), netip.AddrFrom16(b)
}

func (db *DB) LookupIP(addr netip.Addr) IPResult {
	addr = addr.Unmap()
	if !addr.IsValid() {
		return IPResult{}
	}
	isV6 := addr.Is6()
	var allocKey, routeKey uint64
	allocTable, geoTable, routeTable := db.allocV4, db.geoV4, db.routeV4
	if isV6 {
		a16 := addr.As16()
		hi := binary.BigEndian.Uint64(a16[:8])
		allocKey = hi >> uint(64-db.allocV6B)
		routeKey = hi >> uint(64-db.routeV6B)
		allocTable, geoTable, routeTable = db.allocV6, db.geoV6, db.routeV6
	} else {
		a4 := addr.As4()
		v := uint64(binary.BigEndian.Uint32(a4[:]))
		allocKey = v >> uint(32-db.allocV4B)
		routeKey = v >> uint(32-db.routeV4B)
	}

	var res IPResult
	if sStart, sNext, hasNext, pStart, pEnd, ok := db.findSeg(allocTable, allocKey, db.skipAlloc); ok {
		regID, p1, ok1 := readUvarintAt(db.data, pStart, pEnd)
		ccID, _, ok2 := readUvarintAt(db.data, p1, pEnd)
		if ok1 && ok2 && (regID != 0 || ccID != 0) {
			res.HasAlloc = true
			res.Registry = dictAt(db.registries, regID)
			res.AllocCC = db.country(ccID)
			res.AllocStart, res.AllocEnd = slotBoundsToAddrs(isV6, allocTable.bits, sStart, sNext, hasNext)
		}
	}
	if _, _, _, pStart, pEnd, ok := db.findSeg(geoTable, allocKey, db.skipGeo); ok {
		locID, _, ok1 := readUvarintAt(db.data, pStart, pEnd)
		if ok1 && locID != 0 && int(locID) <= db.geoLocCount {
			idx := int(locID) - 1
			off := db.geoLocDataOff + int(binary.LittleEndian.Uint32(db.data[db.geoLocOffsetsOff+idx*4:]))
			end := db.geoLocDataOff + int(binary.LittleEndian.Uint32(db.data[db.geoLocOffsetsOff+(idx+1)*4:]))
			if end <= len(db.data) {
				ccID, p1, okA := readUvarintAt(db.data, off, end)
				regID, p2, okB := readUvarintAt(db.data, p1, end)
				cityID, _, okC := readUvarintAt(db.data, p2, end)
				if okA && okB && okC {
					res.HasGeo = true
					res.GeoCC = db.country(ccID)
					res.GeoRegion = dictAt(db.regions, regID)
					res.GeoCity = dictAt(db.cities, cityID)
				}
			}
		}
	}
	if _, _, _, pStart, pEnd, ok := db.findSeg(routeTable, routeKey, db.skipRoute); ok && pStart < pEnd {
		bits := int(db.data[pStart])
		if bits > 0 {
			val, _, ok1 := readUvarintAt(db.data, pStart+1, pEnd)
			if ok1 && val != 0 {
				res.HasRoute = true
				res.RoutePrefix = netip.PrefixFrom(addr, bits).Masked()
				res.RouteASNs = db.resolveASNSet(val)
			}
		}
	}
	res.Found = res.HasAlloc || res.HasGeo || res.HasRoute
	return res
}

func (db *DB) LookupASN(asNumber uint32, offset, limit int) ASNResult {
	idx := sort.Search(db.asnCount, func(i int) bool {
		return binary.LittleEndian.Uint32(db.data[db.asnNumbersOff+i*4:]) >= asNumber
	})
	if idx >= db.asnCount || binary.LittleEndian.Uint32(db.data[db.asnNumbersOff+idx*4:]) != asNumber {
		return ASNResult{}
	}
	rec, hasAutnum := db.autnumByIdx(idx + 1)
	res := ASNResult{Found: true, ASNumber: asNumber, HasAutnum: hasAutnum, Autnum: rec}
	if db.flags&flagHasASNRoutes == 0 {
		return res
	}
	start := db.asnRouteDataOff + int(binary.LittleEndian.Uint32(db.data[db.asnRouteOffsetsOff+idx*4:]))
	end := db.asnRouteDataOff + int(binary.LittleEndian.Uint32(db.data[db.asnRouteOffsetsOff+(idx+1)*4:]))
	p := start
	itemIdx := 0
	for p < end {
		rawLen := db.data[p]
		p++
		isV6 := rawLen >= 128
		bits := int(rawLen)
		if isV6 {
			bits -= 128
		}
		nb := (bits + 7) / 8
		if p+nb > end {
			break
		}
		if itemIdx >= offset && (limit <= 0 || len(res.Routes) < limit) {
			var prefix netip.Prefix
			if isV6 {
				var a [16]byte
				copy(a[:], db.data[p:p+nb])
				prefix = netip.PrefixFrom(netip.AddrFrom16(a), bits)
			} else {
				var a [4]byte
				copy(a[:], db.data[p:p+nb])
				prefix = netip.PrefixFrom(netip.AddrFrom4(a), bits)
			}
			res.Routes = append(res.Routes, prefix)
		}
		p += nb
		itemIdx++
	}
	res.TotalRoute = itemIdx
	if !hasAutnum && itemIdx == 0 {
		return ASNResult{}
	}
	return res
}

func (db *DB) String() string {
	return fmt.Sprintf("MNL1(autnums=%d, bytes=%d)", db.asnCount, len(db.data))
}
