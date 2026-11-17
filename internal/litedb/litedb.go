// Package litedb builds the MNL1 compact read-only binary database for the
// serverless bgp-lite Cloudflare Worker.
//
// It packs:
//   - dataset metadata (release_tag, built_at, source_commit)
//   - deduplicated dictionaries (countries, registries, autnum statuses/sources)
//   - all autnum identity records sorted by AS number
//   - flattened RIR allocation segments (registry + country)
//   - flattened geofeed country override segments
//   - flattened longest-prefix-match (LPM) route segments (prefix_len + ASN set)
//   - per-ASN deduplicated route prefix lists
//
// All tables are stored so a Worker can binary-search and varint-scan directly
// over the static ArrayBuffer without startup parsing or heap allocation.
package litedb

import (
	"bytes"
	"container/heap"
	"encoding/binary"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/netip"
	"sort"
	"strings"
)

const (
	magic         = "MNL1"
	formatVersion = 1

	DefaultBlockSize   = 64
	DefaultAllocV4Bits = 32
	DefaultAllocV6Bits = 64
	DefaultRouteV4Bits = 32
	DefaultRouteV6Bits = 64

	flagHasASNRoutes = 1 << 0
)

// Metadata mirrors the dataset block returned by /v1/health and meta.dataset.
type Metadata struct {
	ReleaseTag   string `json:"release_tag,omitempty"`
	BuiltAt      string `json:"built_at,omitempty"`
	SourceCommit string `json:"source_commit,omitempty"`
}

// AutnumInput holds the compact identity fields retained per AS number.
type AutnumInput struct {
	ASNumber     uint32
	Registry     string
	Country      string
	Name         string
	Organization string
	Status       string
	Source       string
}

// Stats summarizes the built MNL1 artifact.
type Stats struct {
	Bytes         int
	Countries     int
	Registries    int
	Autnums       int
	AllocV4Segs   int
	AllocV6Segs   int
	GeoV4Segs     int
	GeoV6Segs     int
	RouteV4Segs   int
	RouteV6Segs   int
	MultiASNSets  int
	ASNRouteItems int
}

// BuildOptions controls MNL1 encoding.
type BuildOptions struct {
	Metadata         Metadata
	IncludeASNRoutes bool
	AllocV4Bits      int
	AllocV6Bits      int
}

type interval struct {
	start, end uint64
	val        uint32
	aux        uint8 // prefix length for routes; unused for alloc/geo
	priority   int   // higher wins on overlap (e.g. prefix length or negative width)
	seq        int
}

type segment struct {
	start uint64
	val   uint32
	aux   uint8
}

type asnRoute struct {
	isV6   bool
	prefix [16]byte
	bits   uint8
	keyHi  uint64
	keyLo  uint64
}

type Builder struct {
	opts BuildOptions

	autnums   map[uint32]AutnumInput
	allASNs   map[uint32]struct{}
	asnRoutes map[uint32][]asnRoute

	countries  *freqDict
	registries *freqDict
	statuses   *freqDict
	sources    *freqDict

	allocV4, allocV6 []interval
	geoV4, geoV6     []interval

	// Route deduplication before LPM flattening.
	routeOriginsV4 map[routeKeyV4][]uint32
	routeOriginsV6 map[routeKeyV6][]uint32
	seq            int
}

type routeKeyV4 struct {
	addr uint32
	bits uint8
}

type routeKeyV6 struct {
	hi   uint64
	bits uint8
}

func NewBuilder(opts BuildOptions) *Builder {
	if opts.AllocV4Bits <= 0 {
		opts.AllocV4Bits = DefaultAllocV4Bits
	}
	if opts.AllocV6Bits <= 0 {
		opts.AllocV6Bits = DefaultAllocV6Bits
	}
	return &Builder{
		opts:           opts,
		autnums:        make(map[uint32]AutnumInput),
		allASNs:        make(map[uint32]struct{}),
		asnRoutes:      make(map[uint32][]asnRoute),
		countries:      newFreqDict(),
		registries:     newFreqDict(),
		statuses:       newFreqDict(),
		sources:        newFreqDict(),
		routeOriginsV4: make(map[routeKeyV4][]uint32),
		routeOriginsV6: make(map[routeKeyV6][]uint32),
	}
}

func normalizeCountry(s string) string {
	s = strings.ToUpper(strings.TrimSpace(s))
	if len(s) != 2 {
		return ""
	}
	if s[0] < 'A' || s[0] > 'Z' || s[1] < 'A' || s[1] > 'Z' {
		return ""
	}
	return s
}

func normalizeLower(s string) string {
	return strings.ToLower(strings.TrimSpace(s))
}

func normalizeTrim(s string) string {
	return strings.TrimSpace(s)
}

// AddAutnum records an aut-num metadata entry (first record per ASNumber wins,
// matching boltstore).
func (b *Builder) AddAutnum(in AutnumInput) bool {
	if in.ASNumber == 0 {
		return false
	}
	b.allASNs[in.ASNumber] = struct{}{}
	if _, exists := b.autnums[in.ASNumber]; exists {
		return false
	}
	in.Registry = normalizeLower(in.Registry)
	in.Country = normalizeCountry(in.Country)
	in.Name = normalizeTrim(in.Name)
	in.Organization = normalizeTrim(in.Organization)
	in.Status = normalizeTrim(in.Status)
	in.Source = normalizeTrim(in.Source)
	b.autnums[in.ASNumber] = in
	b.registries.bump(in.Registry)
	b.countries.bump(in.Country)
	b.statuses.bump(in.Status)
	b.sources.bump(in.Source)
	return true
}

// AddAllocation records an RIR allocation range (inclusive bounds).
func (b *Builder) AddAllocation(start, end netip.Addr, registry, country string) bool {
	reg := normalizeLower(registry)
	cc := normalizeCountry(country)
	if reg == "" && cc == "" {
		return false
	}
	family, lo, hi, ok := slotRange(start, end, b.opts.AllocV4Bits, b.opts.AllocV6Bits)
	if !ok {
		return false
	}
	regID := b.registries.id(reg)
	ccID := b.countries.id(cc)
	val := regID<<16 | ccID
	b.seq++
	// Narrower allocations take precedence; invert width into priority.
	width := hi - lo
	iv := interval{start: lo, end: hi, val: val, priority: -int(minUint64(width, 1<<30)), seq: b.seq}
	if family == 4 {
		b.allocV4 = append(b.allocV4, iv)
	} else {
		b.allocV6 = append(b.allocV6, iv)
	}
	return true
}

// AddGeofeed records a geofeed country override range (inclusive bounds).
func (b *Builder) AddGeofeed(start, end netip.Addr, country string) bool {
	cc := normalizeCountry(country)
	if cc == "" {
		return false
	}
	family, lo, hi, ok := slotRange(start, end, b.opts.AllocV4Bits, b.opts.AllocV6Bits)
	if !ok {
		return false
	}
	ccID := b.countries.id(cc)
	b.seq++
	width := hi - lo
	iv := interval{start: lo, end: hi, val: ccID, priority: -int(minUint64(width, 1<<30)), seq: b.seq}
	if family == 4 {
		b.geoV4 = append(b.geoV4, iv)
	} else {
		b.geoV6 = append(b.geoV6, iv)
	}
	return true
}

// AddRoute records a registered route prefix -> origin ASN.
func (b *Builder) AddRoute(prefix netip.Prefix, asNumber uint32) bool {
	if !prefix.IsValid() || asNumber == 0 {
		return false
	}
	prefix = prefix.Masked()
	addr := prefix.Addr().Unmap()
	bits := prefix.Bits()
	b.allASNs[asNumber] = struct{}{}

	if addr.Is4() {
		a4 := addr.As4()
		v := binary.BigEndian.Uint32(a4[:])
		k := routeKeyV4{addr: v, bits: uint8(bits)}
		b.routeOriginsV4[k] = appendUniqueUint32(b.routeOriginsV4[k], asNumber)
		if b.opts.IncludeASNRoutes {
			var raw [16]byte
			copy(raw[:4], a4[:])
			b.asnRoutes[asNumber] = append(b.asnRoutes[asNumber], asnRoute{
				isV6: false, prefix: raw, bits: uint8(bits), keyHi: uint64(v),
			})
		}
		return true
	}

	a16 := addr.As16()
	hi := binary.BigEndian.Uint64(a16[:8])
	lo := binary.BigEndian.Uint64(a16[8:])
	if b.opts.IncludeASNRoutes {
		b.asnRoutes[asNumber] = append(b.asnRoutes[asNumber], asnRoute{
			isV6: true, prefix: a16, bits: uint8(bits), keyHi: hi, keyLo: lo,
		})
	}
	// IPv6 LPM routes are quantized at /64 (covering virtually all BGP/IRR routes).
	if bits > DefaultRouteV6Bits {
		return true
	}
	k := routeKeyV6{hi: hi, bits: uint8(bits)}
	b.routeOriginsV6[k] = appendUniqueUint32(b.routeOriginsV6[k], asNumber)
	return true
}

func appendUniqueUint32(list []uint32, v uint32) []uint32 {
	for _, x := range list {
		if x == v {
			return list
		}
	}
	return append(list, v)
}

func minUint64(a, b uint64) uint64 {
	if a < b {
		return a
	}
	return b
}

// slotRange converts inclusive bounds into quantized slot [lo, hi].
func slotRange(start, end netip.Addr, v4Bits, v6Bits int) (family int, lo, hi uint64, ok bool) {
	start, end = start.Unmap(), end.Unmap()
	if !start.IsValid() || !end.IsValid() || start.Is4() != end.Is4() || start.Compare(end) > 0 {
		return 0, 0, 0, false
	}
	if start.Is4() {
		a, b := start.As4(), end.As4()
		s, e := uint64(binary.BigEndian.Uint32(a[:])), uint64(binary.BigEndian.Uint32(b[:]))
		shift := uint(32 - v4Bits)
		mask := (uint64(1) << shift) - 1
		if s&mask != 0 || e&mask != mask {
			return 4, 0, 0, false
		}
		return 4, s >> shift, e >> shift, true
	}
	a, b := start.As16(), end.As16()
	sHi, sLo := binary.BigEndian.Uint64(a[:8]), binary.BigEndian.Uint64(a[8:])
	eHi, eLo := binary.BigEndian.Uint64(b[:8]), binary.BigEndian.Uint64(b[8:])
	shift := uint(64 - v6Bits)
	mask := (uint64(1) << shift) - 1
	if sLo != 0 || eLo != ^uint64(0) || sHi&mask != 0 || eHi&mask != mask {
		return 6, 0, 0, false
	}
	return 6, sHi >> shift, eHi >> shift, true
}

type freqDict struct {
	ids   map[string]uint32
	items []string
	freq  []int
}

func newFreqDict() *freqDict {
	return &freqDict{
		ids:   map[string]uint32{"": 0},
		items: []string{""},
		freq:  []int{0},
	}
}

func (d *freqDict) id(s string) uint32 {
	if s == "" {
		return 0
	}
	if idx, ok := d.ids[s]; ok {
		d.freq[idx]++
		return idx
	}
	idx := uint32(len(d.items))
	d.ids[s] = idx
	d.items = append(d.items, s)
	d.freq = append(d.freq, 1)
	return idx
}

func (d *freqDict) bump(s string) {
	if s != "" {
		_ = d.id(s)
	}
}

// compactOrder returns a remapping oldID -> compactID sorted by descending freq.
func (d *freqDict) compactOrder() ([]string, []uint32) {
	n := len(d.items) - 1
	order := make([]uint32, n)
	for i := 0; i < n; i++ {
		order[i] = uint32(i + 1)
	}
	sort.Slice(order, func(i, j int) bool {
		a, b := order[i], order[j]
		if d.freq[a] != d.freq[b] {
			return d.freq[a] > d.freq[b]
		}
		return d.items[a] < d.items[b]
	})
	remap := make([]uint32, len(d.items))
	list := make([]string, n)
	for i, oldID := range order {
		remap[oldID] = uint32(i + 1)
		list[i] = d.items[oldID]
	}
	return list, remap
}

type activeHeap []interval

func (h activeHeap) Len() int { return len(h) }
func (h activeHeap) Less(i, j int) bool {
	a, b := h[i], h[j]
	if a.priority != b.priority {
		return a.priority > b.priority
	}
	if aw, bw := a.end-a.start, b.end-b.start; aw != bw {
		return aw < bw
	}
	return a.seq > b.seq
}
func (h activeHeap) Swap(i, j int) { h[i], h[j] = h[j], h[i] }
func (h *activeHeap) Push(x any)   { *h = append(*h, x.(interval)) }
func (h *activeHeap) Pop() any {
	old := *h
	last := old[len(old)-1]
	*h = old[:len(old)-1]
	return last
}

// flatten resolves overlaps across [0, limit) using a sweep line.
// canMerge determines whether two consecutive entries with identical val and aux
// represent the same logical region (for routes, they must share the same /aux block).
func flatten(ivs []interval, bits int, canMerge func(prevStart, currStart uint64, aux uint8) bool) []segment {
	var limit uint64
	if bits < 64 {
		limit = uint64(1) << uint(bits)
	}
	sort.Slice(ivs, func(i, j int) bool {
		if ivs[i].start != ivs[j].start {
			return ivs[i].start < ivs[j].start
		}
		return ivs[i].seq < ivs[j].seq
	})
	out := []segment{{0, 0, 0}}
	sameEntry := func(a, b segment) bool {
		if a.val != b.val || a.aux != b.aux {
			return false
		}
		if a.val == 0 {
			return true
		}
		if canMerge == nil {
			return true
		}
		return canMerge(a.start, b.start, a.aux)
	}
	emit := func(at uint64, val uint32, aux uint8) {
		if limit != 0 && at >= limit {
			return
		}
		cand := segment{at, val, aux}
		last := out[len(out)-1]
		if sameEntry(last, cand) {
			return
		}
		if last.start == at {
			out[len(out)-1] = cand
			if len(out) > 1 && sameEntry(out[len(out)-2], cand) {
				out = out[:len(out)-1]
			}
			return
		}
		out = append(out, cand)
	}
	var active activeHeap
	i, n := 0, len(ivs)
	var p uint64
	for i < n || active.Len() > 0 {
		if active.Len() == 0 {
			p = ivs[i].start
		}
		for i < n && ivs[i].start <= p {
			heap.Push(&active, ivs[i])
			i++
		}
		for active.Len() > 0 && active[0].end < p {
			heap.Pop(&active)
		}
		if active.Len() == 0 {
			emit(p, 0, 0)
			continue
		}
		top := active[0]
		emit(p, top.val, top.aux)
		var next uint64
		if top.end == ^uint64(0) {
			if i >= n {
				break
			}
			next = ivs[i].start
		} else {
			next = top.end + 1
			if i < n && ivs[i].start < next {
				next = ivs[i].start
			}
		}
		if limit != 0 && next >= limit {
			break
		}
		p = next
	}
	return out
}

func sameCIDRBlock(bits int) func(prevStart, currStart uint64, prefixLen uint8) bool {
	return func(prevStart, currStart uint64, prefixLen uint8) bool {
		if prefixLen == 0 {
			return true
		}
		shift := uint(bits) - uint(prefixLen)
		if shift >= 64 {
			return true
		}
		return (prevStart >> shift) == (currStart >> shift)
	}
}

func encodeSegTable(buf *bytes.Buffer, segs []segment, bits, blockSize int, writePayload func(*bytes.Buffer, segment)) {
	kb := (bits + 7) / 8
	blocks := (len(segs) + blockSize - 1) / blockSize
	var data bytes.Buffer
	type blockInfo struct {
		key    uint64
		offset uint32
	}
	infos := make([]blockInfo, 0, blocks)
	var tmp [binary.MaxVarintLen64]byte
	var prev uint64
	for i, s := range segs {
		if i%blockSize == 0 {
			infos = append(infos, blockInfo{s.start, uint32(data.Len())})
			prev = s.start
		}
		n := binary.PutUvarint(tmp[:], s.start-prev)
		data.Write(tmp[:n])
		writePayload(&data, s)
		prev = s.start
	}
	var word [4]byte
	binary.LittleEndian.PutUint32(word[:], uint32(len(segs)))
	buf.Write(word[:])
	binary.LittleEndian.PutUint32(word[:], uint32(data.Len()))
	buf.Write(word[:])
	key := make([]byte, kb)
	for _, info := range infos {
		for j := 0; j < kb; j++ {
			key[j] = byte(info.key >> (8 * uint(j)))
		}
		buf.Write(key)
		binary.LittleEndian.PutUint32(word[:], info.offset)
		buf.Write(word[:])
	}
	buf.Write(data.Bytes())
}

func writeUvarint(buf *bytes.Buffer, v uint64) {
	var tmp [binary.MaxVarintLen64]byte
	n := binary.PutUvarint(tmp[:], v)
	buf.Write(tmp[:n])
}

func writeStringDict(buf *bytes.Buffer, items []string) {
	writeUvarint(buf, uint64(len(items)))
	for _, s := range items {
		writeUvarint(buf, uint64(len(s)))
		buf.WriteString(s)
	}
}

// Encode serializes all accumulated tables into the MNL1 binary format.
func (b *Builder) Encode(w io.Writer) (Stats, error) {
	metaJSON, err := json.Marshal(b.opts.Metadata)
	if err != nil {
		return Stats{}, err
	}
	if len(metaJSON) > 65535 {
		return Stats{}, errors.New("litedb: metadata exceeds 64KiB")
	}

	// Build sorted ASN list (all ASNs from autnums + routes) -> 1-based asnIdx.
	asns := make([]uint32, 0, len(b.allASNs))
	for asn := range b.allASNs {
		asns = append(asns, asn)
	}
	sort.Slice(asns, func(i, j int) bool { return asns[i] < asns[j] })
	asnToIdx := make(map[uint32]uint32, len(asns))
	for i, asn := range asns {
		asnToIdx[asn] = uint32(i + 1)
	}

	// Compact string dictionaries.
	countries, countryMap := b.countries.compactOrder()
	registries, registryMap := b.registries.compactOrder()
	statuses, statusMap := b.statuses.compactOrder()
	sources, sourceMap := b.sources.compactOrder()

	// Remap alloc and geo intervals to compact dictionary IDs.
	for i := range b.allocV4 {
		regID := registryMap[b.allocV4[i].val>>16]
		ccID := countryMap[b.allocV4[i].val&0xffff]
		b.allocV4[i].val = regID<<16 | ccID
	}
	for i := range b.allocV6 {
		regID := registryMap[b.allocV6[i].val>>16]
		ccID := countryMap[b.allocV6[i].val&0xffff]
		b.allocV6[i].val = regID<<16 | ccID
	}
	for i := range b.geoV4 {
		b.geoV4[i].val = countryMap[b.geoV4[i].val]
	}
	for i := range b.geoV6 {
		b.geoV6[i].val = countryMap[b.geoV6[i].val]
	}

	// Build route intervals & multi-origin ASN sets.
	asnCount := uint32(len(asns))
	multiSets := make([][]uint32, 0)
	multiSetMap := make(map[string]uint32)
	resolveASNSet := func(list []uint32) uint32 {
		sort.Slice(list, func(i, j int) bool { return list[i] < list[j] })
		if len(list) == 1 {
			return asnToIdx[list[0]]
		}
		var kb strings.Builder
		for i, a := range list {
			if i > 0 {
				kb.WriteByte(',')
			}
			fmt.Fprintf(&kb, "%d", a)
		}
		k := kb.String()
		if idx, ok := multiSetMap[k]; ok {
			return idx
		}
		indices := make([]uint32, len(list))
		for i, a := range list {
			indices[i] = asnToIdx[a]
		}
		idx := asnCount + uint32(len(multiSets)) + 1
		multiSetMap[k] = idx
		multiSets = append(multiSets, indices)
		return idx
	}

	routeV4Ivs := make([]interval, 0, len(b.routeOriginsV4))
	for k, origs := range b.routeOriginsV4 {
		shift := uint(32 - k.bits)
		lo := uint64(k.addr)
		var hi uint64
		if shift == 32 {
			hi = 0xffffffff
		} else {
			hi = lo | ((uint64(1) << shift) - 1)
		}
		b.seq++
		routeV4Ivs = append(routeV4Ivs, interval{
			start: lo, end: hi, val: resolveASNSet(origs), aux: k.bits, priority: int(k.bits), seq: b.seq,
		})
	}
	routeV6Ivs := make([]interval, 0, len(b.routeOriginsV6))
	for k, origs := range b.routeOriginsV6 {
		shift := uint(64 - k.bits)
		lo := k.hi
		var hi uint64
		if shift == 64 {
			hi = ^uint64(0)
		} else {
			hi = lo | ((uint64(1) << shift) - 1)
		}
		b.seq++
		routeV6Ivs = append(routeV6Ivs, interval{
			start: lo, end: hi, val: resolveASNSet(origs), aux: k.bits, priority: int(k.bits), seq: b.seq,
		})
	}

	// Flatten all interval layers.
	allocSegs4 := flatten(b.allocV4, b.opts.AllocV4Bits, nil)
	allocSegs6 := flatten(b.allocV6, b.opts.AllocV6Bits, nil)
	geoSegs4 := flatten(b.geoV4, b.opts.AllocV4Bits, nil)
	geoSegs6 := flatten(b.geoV6, b.opts.AllocV6Bits, nil)
	routeSegs4 := flatten(routeV4Ivs, DefaultRouteV4Bits, sameCIDRBlock(DefaultRouteV4Bits))
	routeSegs6 := flatten(routeV6Ivs, DefaultRouteV6Bits, sameCIDRBlock(DefaultRouteV6Bits))

	var buf bytes.Buffer
	var flags byte
	if b.opts.IncludeASNRoutes {
		flags |= flagHasASNRoutes
	}

	// 1. Header
	buf.WriteString(magic)
	buf.Write([]byte{
		formatVersion, flags, DefaultBlockSize,
		byte(b.opts.AllocV4Bits), byte(b.opts.AllocV6Bits),
		DefaultRouteV4Bits, DefaultRouteV6Bits,
	})
	var u16 [2]byte
	binary.LittleEndian.PutUint16(u16[:], uint16(len(metaJSON)))
	buf.Write(u16[:])
	buf.Write(metaJSON)

	// 2. Dictionaries
	binary.LittleEndian.PutUint16(u16[:], uint16(len(countries)))
	buf.Write(u16[:])
	for _, cc := range countries {
		buf.WriteString(cc)
	}
	writeStringDict(&buf, registries)
	writeStringDict(&buf, statuses)
	writeStringDict(&buf, sources)

	// 3. Autnums table
	var u32 [4]byte
	binary.LittleEndian.PutUint32(u32[:], asnCount)
	buf.Write(u32[:])
	for _, asn := range asns {
		binary.LittleEndian.PutUint32(u32[:], asn)
		buf.Write(u32[:])
	}
	var autnumData bytes.Buffer
	autnumOffsets := make([]uint32, len(asns)+1)
	for i, asn := range asns {
		autnumOffsets[i] = uint32(autnumData.Len())
		rec, ok := b.autnums[asn]
		if !ok {
			autnumData.WriteByte(0) // no autnum metadata
			continue
		}
		autnumData.WriteByte(1)
		writeUvarint(&autnumData, uint64(registryMap[b.registries.ids[rec.Registry]]))
		writeUvarint(&autnumData, uint64(countryMap[b.countries.ids[rec.Country]]))
		writeUvarint(&autnumData, uint64(statusMap[b.statuses.ids[rec.Status]]))
		writeUvarint(&autnumData, uint64(sourceMap[b.sources.ids[rec.Source]]))
		writeUvarint(&autnumData, uint64(len(rec.Name)))
		autnumData.WriteString(rec.Name)
		writeUvarint(&autnumData, uint64(len(rec.Organization)))
		autnumData.WriteString(rec.Organization)
	}
	autnumOffsets[len(asns)] = uint32(autnumData.Len())
	for _, off := range autnumOffsets {
		binary.LittleEndian.PutUint32(u32[:], off)
		buf.Write(u32[:])
	}
	buf.Write(autnumData.Bytes())

	// 4. Multi-origin ASN sets
	binary.LittleEndian.PutUint32(u32[:], uint32(len(multiSets)))
	buf.Write(u32[:])
	var multiData bytes.Buffer
	multiOffsets := make([]uint32, len(multiSets)+1)
	for i, set := range multiSets {
		multiOffsets[i] = uint32(multiData.Len())
		writeUvarint(&multiData, uint64(len(set)))
		for _, idx := range set {
			writeUvarint(&multiData, uint64(idx))
		}
	}
	multiOffsets[len(multiSets)] = uint32(multiData.Len())
	for _, off := range multiOffsets {
		binary.LittleEndian.PutUint32(u32[:], off)
		buf.Write(u32[:])
	}
	buf.Write(multiData.Bytes())

	// 5. Alloc (v4, v6)
	writeAlloc := func(d *bytes.Buffer, s segment) {
		writeUvarint(d, uint64(s.val>>16))
		writeUvarint(d, uint64(s.val&0xffff))
	}
	encodeSegTable(&buf, allocSegs4, b.opts.AllocV4Bits, DefaultBlockSize, writeAlloc)
	encodeSegTable(&buf, allocSegs6, b.opts.AllocV6Bits, DefaultBlockSize, writeAlloc)

	// 6. Geofeed overrides (v4, v6)
	writeGeo := func(d *bytes.Buffer, s segment) {
		writeUvarint(d, uint64(s.val))
	}
	encodeSegTable(&buf, geoSegs4, b.opts.AllocV4Bits, DefaultBlockSize, writeGeo)
	encodeSegTable(&buf, geoSegs6, b.opts.AllocV6Bits, DefaultBlockSize, writeGeo)

	// 7. Route LPM tables (v4, v6)
	writeRoute := func(d *bytes.Buffer, s segment) {
		d.WriteByte(s.aux)
		if s.aux > 0 {
			writeUvarint(d, uint64(s.val))
		}
	}
	encodeSegTable(&buf, routeSegs4, DefaultRouteV4Bits, DefaultBlockSize, writeRoute)
	encodeSegTable(&buf, routeSegs6, DefaultRouteV6Bits, DefaultBlockSize, writeRoute)

	// 8. Per-ASN deduplicated route prefixes (optional)
	asnRouteItems := 0
	if b.opts.IncludeASNRoutes {
		var rData bytes.Buffer
		rOffsets := make([]uint32, len(asns)+1)
		for i, asn := range asns {
			rOffsets[i] = uint32(rData.Len())
			routes := dedupSortASNRoutes(b.asnRoutes[asn])
			asnRouteItems += len(routes)
			for _, r := range routes {
				nbytes := (int(r.bits) + 7) / 8
				if r.isV6 {
					rData.WriteByte(128 + r.bits)
				} else {
					rData.WriteByte(r.bits)
				}
				rData.Write(r.prefix[:nbytes])
			}
		}
		rOffsets[len(asns)] = uint32(rData.Len())
		for _, off := range rOffsets {
			binary.LittleEndian.PutUint32(u32[:], off)
			buf.Write(u32[:])
		}
		buf.Write(rData.Bytes())
	}

	if _, err := w.Write(buf.Bytes()); err != nil {
		return Stats{}, err
	}
	return Stats{
		Bytes:         buf.Len(),
		Countries:     len(countries),
		Registries:    len(registries),
		Autnums:       len(b.autnums),
		AllocV4Segs:   len(allocSegs4),
		AllocV6Segs:   len(allocSegs6),
		GeoV4Segs:     len(geoSegs4),
		GeoV6Segs:     len(geoSegs6),
		RouteV4Segs:   len(routeSegs4),
		RouteV6Segs:   len(routeSegs6),
		MultiASNSets:  len(multiSets),
		ASNRouteItems: asnRouteItems,
	}, nil
}

func dedupSortASNRoutes(in []asnRoute) []asnRoute {
	if len(in) == 0 {
		return nil
	}
	sort.Slice(in, func(i, j int) bool {
		a, b := in[i], in[j]
		if a.isV6 != b.isV6 {
			return !a.isV6
		}
		if a.keyHi != b.keyHi {
			return a.keyHi < b.keyHi
		}
		if a.keyLo != b.keyLo {
			return a.keyLo < b.keyLo
		}
		return a.bits < b.bits
	})
	out := in[:1]
	for _, r := range in[1:] {
		prev := out[len(out)-1]
		if prev.isV6 == r.isV6 && prev.keyHi == r.keyHi && prev.keyLo == r.keyLo && prev.bits == r.bits {
			continue
		}
		out = append(out, r)
	}
	return out
}
