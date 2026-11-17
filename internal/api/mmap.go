package api

import (
	"bytes"
	"context"
	"encoding/base64"
	"errors"
	"fmt"
	"net/netip"
	"sort"
	"strconv"
	"strings"
	"sync/atomic"

	"github.com/mehrnet/bgp-api/internal/ipkey"
	"github.com/mehrnet/bgp-api/internal/mmapstore"
)

const (
	maxCandidates          = 64
	maxOverlapIndexEntries = 2_000_000

	datasetWarmIdle       int32 = 0
	datasetWarmInProgress int32 = 1
	datasetWarmComplete   int32 = 2
)

var (
	errQueryTooBroad      = errors.New("overlap scan limit exceeded")
	errInvalidRangeCursor = errors.New("invalid range cursor")
)

type MmapRepository struct {
	db               *mmapstore.DB
	overlapSlots     chan struct{}
	datasetWarmState atomic.Int32
}

// DatasetWarmup describes one sequential page-cache warmup of the immutable
// mmap dataset. AlreadyWarm means an earlier warmup finished in this process.
type DatasetWarmup struct {
	Bytes       int64
	AlreadyWarm bool
}

// CompactSelectorPreload reports the size of LPM selector slices warmed in
// the OS page cache.
type CompactSelectorPreload struct {
	Bytes int64
}

func NewMmapRepository(path string) (*MmapRepository, error) {
	db, err := mmapstore.Open(path)
	if err != nil {
		return nil, err
	}
	return &MmapRepository{db: db, overlapSlots: make(chan struct{}, 2)}, nil
}

func (repository *MmapRepository) Close() error {
	if repository == nil || repository.db == nil {
		return nil
	}
	return repository.db.Close()
}

func (repository *MmapRepository) DatabaseSize() (int64, error) {
	return repository.db.Size(), nil
}

func (repository *MmapRepository) CompactSelectorBytes() (int64, error) {
	return repository.db.CompactSelectorBytes(), nil
}

// PreloadCompactSelectors advises the OS to populate the six LPM selector
// regions into the page cache without duplicating them onto the Go heap.
func (repository *MmapRepository) PreloadCompactSelectors() (CompactSelectorPreload, error) {
	warmed, err := repository.db.WarmCompactSelectors()
	if err != nil {
		return CompactSelectorPreload{}, err
	}
	return CompactSelectorPreload{Bytes: warmed}, nil
}

// WarmDataset reads the immutable mmap file sequentially with a bounded buffer
// to warm the Linux page cache for broad and details=full queries.
func (repository *MmapRepository) WarmDataset(ctx context.Context) (DatasetWarmup, error) {
	if repository.datasetWarmState.Load() == datasetWarmComplete {
		return DatasetWarmup{Bytes: repository.db.Size(), AlreadyWarm: true}, nil
	}
	if !repository.datasetWarmState.CompareAndSwap(datasetWarmIdle, datasetWarmInProgress) {
		return DatasetWarmup{}, errors.New("mmap dataset warmup is already in progress")
	}
	completed := false
	defer func() {
		if completed {
			repository.datasetWarmState.Store(datasetWarmComplete)
			return
		}
		repository.datasetWarmState.Store(datasetWarmIdle)
	}()

	warmed, err := repository.db.WarmDataset(ctx)
	if err != nil {
		return DatasetWarmup{}, err
	}
	completed = true
	return DatasetWarmup{Bytes: warmed}, nil
}

func (repository *MmapRepository) DatasetMetadata(_ context.Context) (*DatasetMetadata, error) {
	meta := repository.db.Metadata()
	return &DatasetMetadata{
		ReleaseTag:   optional(meta.ReleaseTag),
		BuiltAt:      optional(meta.BuiltAt),
		SourceCommit: optional(meta.SourceCommit),
	}, nil
}

func (repository *MmapRepository) Lookup(ctx context.Context, ip ipkey.RuntimeIP, options LookupOptions) (*LookupResponse, error) {
	if options.Details == LookupDetailsFull {
		return repository.lookupFull(ctx, ip)
	}
	return repository.lookupCompact(ip)
}

func (repository *MmapRepository) lookupCompact(ip ipkey.RuntimeIP) (*LookupResponse, error) {
	address := ip.Address
	var allocation *mmapstore.Allocation
	var routes []mmapstore.Route
	var geofeed *mmapstore.Geofeed

	allocID, err := repository.db.LookupAllocationLPM(address)
	if err != nil {
		return nil, fmt.Errorf("mmap compact IP lookup: %w", err)
	}
	if allocID != 0 {
		record, err := repository.db.AllocationLookup(allocID)
		if err != nil {
			return nil, fmt.Errorf("mmap compact IP lookup: %w", err)
		}
		allocation = &record
	}

	routeIDs, err := repository.db.LookupRouteLPM(address)
	if err != nil {
		return nil, fmt.Errorf("mmap compact IP lookup: %w", err)
	}
	routes = make([]mmapstore.Route, len(routeIDs))
	for index, id := range routeIDs {
		record, err := repository.db.RouteLookup(id)
		if err != nil {
			return nil, fmt.Errorf("mmap compact IP lookup: %w", err)
		}
		routes[index] = record
	}

	origins, err := mmapOriginIdentities(repository.db, routes)
	if err != nil {
		return nil, fmt.Errorf("mmap compact IP lookup: %w", err)
	}

	geoID, err := repository.db.LookupGeofeedLPM(address)
	if err != nil {
		return nil, fmt.Errorf("mmap compact IP lookup: %w", err)
	}
	if geoID != 0 {
		record, err := repository.db.Geofeed(geoID)
		if err != nil {
			return nil, fmt.Errorf("mmap compact IP lookup: %w", err)
		}
		geofeed = &record
	}

	return buildCompactMmapResponse(ip, allocation, routes, origins, geofeed), nil
}

func (repository *MmapRepository) lookupFull(ctx context.Context, ip ipkey.RuntimeIP) (*LookupResponse, error) {
	address := ip.Address
	allocationIDs, err := repository.db.MatchingPrefixIDs(ctx, mmapstore.TableAllocations, address, maxCandidates)
	if err != nil {
		return nil, fmt.Errorf("mmap IP lookup: %w", err)
	}
	allocations := make([]mmapstore.Allocation, 0, len(allocationIDs))
	for _, id := range allocationIDs {
		record, err := repository.db.Allocation(id)
		if err != nil {
			return nil, fmt.Errorf("mmap IP lookup: %w", err)
		}
		allocations = append(allocations, record)
	}

	routeIDs, err := repository.db.MatchingPrefixIDs(ctx, mmapstore.TableRoutes, address, maxCandidates)
	if err != nil {
		return nil, fmt.Errorf("mmap IP lookup: %w", err)
	}
	routes := make([]mmapstore.Route, 0, len(routeIDs))
	for _, id := range routeIDs {
		record, err := repository.db.Route(id)
		if err != nil {
			return nil, fmt.Errorf("mmap IP lookup: %w", err)
		}
		routes = append(routes, record)
	}

	selectedRoutes := narrowestRoutes(routes)
	origins, err := mmapOriginIdentities(repository.db, selectedRoutes)
	if err != nil {
		return nil, fmt.Errorf("mmap IP lookup: %w", err)
	}

	geofeedIDs, err := repository.db.MatchingPrefixIDs(ctx, mmapstore.TableGeofeeds, address, maxCandidates)
	if err != nil {
		return nil, fmt.Errorf("mmap IP lookup: %w", err)
	}
	geofeeds := make([]mmapstore.Geofeed, 0, len(geofeedIDs))
	for _, id := range geofeedIDs {
		record, err := repository.db.Geofeed(id)
		if err != nil {
			return nil, fmt.Errorf("mmap IP lookup: %w", err)
		}
		geofeeds = append(geofeeds, record)
	}

	response := buildCompactMmapResponse(ip, narrowestAllocation(allocations), selectedRoutes, origins, narrowestGeofeed(geofeeds))
	if details := mmapDetails(allocations, routes, geofeeds); details != nil {
		response.Details = details
	}
	return response, nil
}

func buildCompactMmapResponse(ip ipkey.RuntimeIP, allocation *mmapstore.Allocation, routes []mmapstore.Route, origins []ASNIdentity, geofeed *mmapstore.Geofeed) *LookupResponse {
	response := &LookupResponse{IP: ip.Canonical, Version: ip.Version}
	response.Network.ASNs = mmapASNs(routes)
	if response.Network.ASNs == nil {
		response.Network.ASNs = []string{}
	}
	response.Network.Origins = origins
	if response.Network.Origins == nil {
		response.Network.Origins = []ASNIdentity{}
	}
	if len(response.Network.ASNs) == 1 {
		response.Network.ASN = &response.Network.ASNs[0]
		response.Network.ASNumber = asNumber(response.Network.ASN)
	}
	if len(routes) > 0 {
		route := routes[0]
		response.Network.CIDR = optional(recordPrefix(route))
		start, end := prefixRangeForRecord(route)
		startIP, endIP := start.Addr(int(route.Version)).String(), end.Addr(int(route.Version)).String()
		response.Network.StartIP, response.Network.EndIP = &startIP, &endIP
		response.Network.AbuseContact = optional(route.AbuseContact)
	}
	if allocation != nil {
		allocationStart := allocation.Start.Addr(int(allocation.Version)).String()
		allocationEnd := allocation.End.Addr(int(allocation.Version)).String()
		response.Allocation.StartIP, response.Allocation.EndIP = &allocationStart, &allocationEnd
		response.Allocation.Registry = lower(optional(allocation.Registry))
		response.Allocation.CountryRaw = upper(optional(allocation.Country))
		response.Allocation.CountryCode = countryCode(response.Allocation.CountryRaw)
		response.Allocation.Name = optional(allocation.Name)
		response.Allocation.AllocationDate = optional(allocation.AllocationDate)
		response.Allocation.Status = optional(allocation.Status)
		response.Allocation.AbuseContact = optional(allocation.AbuseContact)
		response.Registry = lower(optional(allocation.Registry))
		response.AllocationDate = response.Allocation.AllocationDate
		response.AllocationStatus = response.Allocation.Status
	}
	if geofeed != nil {
		response.Location.CountryCode = countryCode(upper(optional(geofeed.Country)))
		response.Location.Region = optional(geofeed.Region)
		response.Location.City = optional(geofeed.City)
	}
	if response.Location.CountryCode == nil {
		response.Location.CountryCode = response.Allocation.CountryCode
	}
	response.Sources.Allocation = allocation != nil
	response.Sources.Route = len(routes) > 0
	response.Sources.Geofeed = geofeed != nil
	return response
}

func narrowestAllocation(values []mmapstore.Allocation) *mmapstore.Allocation {
	if len(values) == 0 {
		return nil
	}
	best := &values[0]
	for index := 1; index < len(values); index++ {
		candidate := &values[index]
		if rangeWidthCompare(candidate.Start, candidate.End, best.Start, best.End) < 0 {
			best = candidate
		}
	}
	return best
}

func narrowestGeofeed(values []mmapstore.Geofeed) *mmapstore.Geofeed {
	if len(values) == 0 {
		return nil
	}
	best := &values[0]
	for index := 1; index < len(values); index++ {
		candidate := &values[index]
		if rangeWidthCompare(candidate.Start, candidate.End, best.Start, best.End) < 0 {
			best = candidate
		}
	}
	return best
}

func narrowestRoutes(values []mmapstore.Route) []mmapstore.Route {
	if len(values) == 0 {
		return nil
	}
	bestLength := values[0].PrefixLength
	for _, value := range values[1:] {
		if value.PrefixLength > bestLength {
			bestLength = value.PrefixLength
		}
	}
	result := make([]mmapstore.Route, 0, len(values))
	for _, value := range values {
		if value.PrefixLength == bestLength {
			result = append(result, value)
		}
	}
	return result
}

func rangeWidthCompare(leftStart, leftEnd, rightStart, rightEnd mmapstore.Address) int {
	var leftWidth, rightWidth [16]byte
	subtractAddress(leftWidth[:], leftEnd[:], leftStart[:])
	subtractAddress(rightWidth[:], rightEnd[:], rightStart[:])
	return bytes.Compare(leftWidth[:], rightWidth[:])
}

func subtractAddress(destination, left, right []byte) {
	borrow := 0
	for index := len(destination) - 1; index >= 0; index-- {
		value := int(left[index]) - int(right[index]) - borrow
		if value < 0 {
			value += 256
			borrow = 1
		} else {
			borrow = 0
		}
		destination[index] = byte(value)
	}
}

func mmapASNs(routes []mmapstore.Route) []string {
	if len(routes) == 1 {
		name := strings.ToUpper(strings.TrimSpace(routes[0].OriginASN))
		if name == "" {
			return nil
		}
		return []string{name}
	}
	type asnValue struct {
		name   string
		number uint32
	}
	values := make([]asnValue, 0, len(routes))
	seen := make(map[string]struct{}, len(routes))
	for _, route := range routes {
		name := strings.ToUpper(strings.TrimSpace(route.OriginASN))
		if name == "" {
			continue
		}
		if _, ok := seen[name]; ok {
			continue
		}
		seen[name] = struct{}{}
		values = append(values, asnValue{name: name, number: route.ASNumber})
	}
	sort.Slice(values, func(left, right int) bool {
		if values[left].number != values[right].number {
			return values[left].number < values[right].number
		}
		return values[left].name < values[right].name
	})
	result := make([]string, 0, len(values))
	for _, value := range values {
		result = append(result, value.name)
	}
	return result
}

func mmapOriginIdentities(db *mmapstore.DB, routes []mmapstore.Route) ([]ASNIdentity, error) {
	asns := mmapASNs(routes)
	if len(asns) == 0 {
		return []ASNIdentity{}, nil
	}
	numbers := make(map[string]uint32, len(asns))
	for _, route := range routes {
		asn := strings.ToUpper(strings.TrimSpace(route.OriginASN))
		if asn != "" {
			numbers[asn] = route.ASNumber
		}
	}
	origins := make([]ASNIdentity, 0, len(asns))
	for _, asn := range asns {
		origin := ASNIdentity{ASN: asn, ASNumber: int(numbers[asn])}
		if origin.ASNumber > 0 {
			autnum, ok, err := db.AutnumIdentity(uint32(origin.ASNumber))
			if err != nil {
				return nil, fmt.Errorf("decode aut-num %s: %w", asn, err)
			}
			if ok {
				origin.Name = optional(autnum.Name)
				origin.Organization = optional(autnum.Organization)
			}
		}
		origins = append(origins, origin)
	}
	return origins, nil
}

func mmapDetails(allocations []mmapstore.Allocation, routes []mmapstore.Route, geofeeds []mmapstore.Geofeed) *LookupDetails {
	result := &LookupDetails{}
	if len(allocations) > 0 {
		result.Allocations = make([]LookupDetailRecord, 0, len(allocations))
		for _, value := range allocations {
			result.Allocations = append(result.Allocations, allocationDetail(value))
		}
	}
	if len(routes) > 0 {
		result.Routes = make([]LookupDetailRecord, 0, len(routes))
		for _, value := range routes {
			result.Routes = append(result.Routes, routeDetail(value))
		}
	}
	if len(geofeeds) > 0 {
		result.Geofeeds = make([]LookupDetailRecord, 0, len(geofeeds))
		for _, value := range geofeeds {
			result.Geofeeds = append(result.Geofeeds, geofeedDetail(value))
		}
	}
	return result
}

func allocationDetail(value mmapstore.Allocation) LookupDetailRecord {
	start, end := value.Start.Addr(int(value.Version)).String(), value.End.Addr(int(value.Version)).String()
	return LookupDetailRecord{
		StartIP: start, EndIP: end, Version: int(value.Version),
		Registry: lower(optional(value.Registry)), CountryCode: countryCode(upper(optional(value.Country))),
		CountryRaw: upper(optional(value.Country)), Name: optional(value.Name),
		AllocationDate: optional(value.AllocationDate), Status: optional(value.Status),
		AbuseContact: optional(value.AbuseContact), Source: optional(value.Source),
		Maintainers: optional(value.Maintainers), Organization: optional(value.Organization),
		Created: optional(value.Created), LastModified: optional(value.LastModified),
		Description: optional(value.Description),
	}
}

func routeDetail(value mmapstore.Route) LookupDetailRecord {
	start, end := prefixRangeForRecord(value)
	result := LookupDetailRecord{
		StartIP: start.Addr(int(value.Version)).String(), EndIP: end.Addr(int(value.Version)).String(),
		Version: int(value.Version), CIDR: optional(recordPrefix(value)),
		ASN: upper(optional(value.OriginASN)), Registry: lower(optional(value.Registry)),
		Source: optional(value.Source), Maintainers: optional(value.Maintainers),
		Organization: optional(value.Organization), AbuseContact: optional(value.AbuseContact),
		Description: optional(value.Description),
	}
	result.ASNumber = asNumber(result.ASN)
	return result
}

func geofeedDetail(value mmapstore.Geofeed) LookupDetailRecord {
	start, end := value.Start.Addr(int(value.Version)).String(), value.End.Addr(int(value.Version)).String()
	country := upper(optional(value.Country))
	return LookupDetailRecord{
		StartIP: start, EndIP: end, Version: int(value.Version),
		CountryCode: countryCode(country), CountryRaw: country,
		Region: optional(value.Region), City: optional(value.City),
	}
}

func (repository *MmapRepository) LookupPrefix(ctx context.Context, prefix ipkey.ParsedPrefix, page Page) (*PrefixResponse, error) {
	if err := repository.acquireOverlapSlot(ctx); err != nil {
		return nil, err
	}
	defer repository.releaseOverlapSlot()

	allocation, err := repository.coveringAllocation(ctx, prefix)
	if err != nil {
		return nil, fmt.Errorf("mmap prefix lookup: %w", err)
	}
	startAddr := netip.MustParseAddr(prefix.Start.Canonical)
	endAddr := netip.MustParseAddr(prefix.End.Canonical)
	ids, err := repository.db.OverlapRouteIDs(ctx, startAddr, endAddr, page.Cursor, maxOverlapIndexEntries)
	if err != nil {
		if errors.Is(err, mmapstore.ErrQueryTooBroad) {
			return nil, errQueryTooBroad
		}
		return nil, fmt.Errorf("mmap prefix lookup: %w", err)
	}
	if len(ids) > page.Limit+1 {
		ids = ids[:page.Limit+1]
	}
	routes := make([]RouteObject, 0, len(ids))
	for _, id := range ids {
		record, err := repository.db.Route(id)
		if err != nil {
			return nil, fmt.Errorf("mmap prefix lookup: %w", err)
		}
		object := routeObject(id, record)
		routePrefix := recordPrefix(record)
		switch {
		case routePrefix == prefix.Canonical:
			object.Relation = "exact"
		case routePrefixContains(routePrefix, prefix):
			object.Relation = "covering"
		default:
			object.Relation = "more_specific"
		}
		routes = append(routes, object)
	}
	routes, next := routePage(routes, page.Limit)
	return &PrefixResponse{
		Prefix:     prefixDescriptor(prefix),
		Allocation: allocation,
		Routes:     RoutePage{Items: routes, NextCursor: next},
	}, nil
}

func (repository *MmapRepository) LookupRange(ctx context.Context, value ipkey.ParsedRange, kind RangeKind, page RangePage) (*RangeResponse, error) {
	if err := repository.acquireOverlapSlot(ctx); err != nil {
		return nil, err
	}
	defer repository.releaseOverlapSlot()

	response := &RangeResponse{Range: rangeDescriptor(value), Kind: kind, Mode: "records"}
	entries, next, err := repository.rangePageEntries(ctx, value, kind, page)
	if err != nil {
		return nil, fmt.Errorf("mmap range lookup: %w", err)
	}
	response.NextCursor = next
	if kind == RangeAllocations {
		for _, entry := range entries {
			record, err := repository.db.Allocation(entry.ID)
			if err != nil {
				return nil, fmt.Errorf("mmap range lookup: %w", err)
			}
			response.Allocations = append(response.Allocations, allocationObject(entry.ID, record))
		}
		return response, nil
	}
	for _, entry := range entries {
		record, err := repository.db.Route(entry.ID)
		if err != nil {
			return nil, fmt.Errorf("mmap range lookup: %w", err)
		}
		response.Routes = append(response.Routes, routeObject(entry.ID, record))
	}
	return response, nil
}

const (
	rangeCursorCovering byte = 1
	rangeCursorDirect   byte = 2
	rangeCursorSize          = 1 + 1 + 16 + 16 + mmapstore.RangeKeySize
)

type rangeCursor struct {
	phase      byte
	kind       RangeKind
	queryStart mmapstore.Address
	queryEnd   mmapstore.Address
	key        [mmapstore.RangeKeySize]byte
}

func (repository *MmapRepository) rangePageEntries(ctx context.Context, value ipkey.ParsedRange, kind RangeKind, page RangePage) ([]mmapstore.RangeIndexEntry, nullableString, error) {
	cursor, err := decodeRangeCursor(page.Cursor)
	if err != nil {
		return nil, nil, err
	}
	start := mmapstore.AddressFromAddr(netip.MustParseAddr(value.Start.Canonical))
	end := mmapstore.AddressFromAddr(netip.MustParseAddr(value.End.Canonical))
	version := uint8(value.Version)
	if cursor.phase != 0 && (cursor.kind != kind || cursor.queryStart != start || cursor.queryEnd != end || cursor.key[0] != version) {
		return nil, nil, errInvalidRangeCursor
	}
	tableKind := rangeTableKind(kind)
	entries := make([]mmapstore.RangeIndexEntry, 0, page.Limit)
	if cursor.phase != rangeCursorDirect {
		covering, err := repository.coveringRangeEntries(ctx, tableKind, kind, version, start)
		if err != nil {
			return nil, nil, err
		}
		for _, entry := range covering {
			if cursor.phase == rangeCursorCovering && bytes.Compare(entry.Key[:], cursor.key[:]) <= 0 {
				continue
			}
			if len(entries) == page.Limit {
				return entries, encodeRangeCursor(rangeCursorCovering, kind, start, end, entries[len(entries)-1].Key), nil
			}
			entries = append(entries, entry)
		}
	}

	var after *[mmapstore.RangeKeySize]byte
	if cursor.phase == rangeCursorDirect {
		after = &cursor.key
	}
	direct, more, err := repository.db.DirectRangeEntries(ctx, tableKind, version, start, end, after, page.Limit-len(entries)+1)
	if err != nil {
		return nil, nil, err
	}
	if len(entries) == page.Limit {
		if len(direct) > 0 {
			var first [mmapstore.RangeKeySize]byte
			mmapstore.RangeKeyStartKey(first[:], version, start)
			return entries, encodeRangeCursor(rangeCursorDirect, kind, start, end, first), nil
		}
		return entries, nil, nil
	}
	capacity := page.Limit - len(entries)
	if len(direct) > capacity {
		direct = direct[:capacity]
		more = true
	}
	entries = append(entries, direct...)
	if more && len(entries) > 0 {
		return entries, encodeRangeCursor(rangeCursorDirect, kind, start, end, entries[len(entries)-1].Key), nil
	}
	return entries, nil, nil
}

func rangeTableKind(kind RangeKind) mmapstore.TableKind {
	if kind == RangeAllocations {
		return mmapstore.TableAllocations
	}
	return mmapstore.TableRoutes
}

func (repository *MmapRepository) coveringRangeEntries(ctx context.Context, tableKind mmapstore.TableKind, kind RangeKind, version uint8, start mmapstore.Address) ([]mmapstore.RangeIndexEntry, error) {
	ids, err := repository.db.MatchingPrefixIDs(ctx, tableKind, start.Addr(int(version)), 0)
	if err != nil {
		return nil, err
	}
	seen := make(map[uint32]struct{}, len(ids))
	entries := make([]mmapstore.RangeIndexEntry, 0, len(ids))
	for _, id := range ids {
		if _, ok := seen[id]; ok {
			continue
		}
		seen[id] = struct{}{}
		entry, matches, err := repository.coveringRangeEntry(kind, id, start)
		if err != nil {
			return nil, err
		}
		if matches {
			entries = append(entries, entry)
		}
	}
	sort.Slice(entries, func(left, right int) bool { return bytes.Compare(entries[left].Key[:], entries[right].Key[:]) < 0 })
	return entries, nil
}

func (repository *MmapRepository) coveringRangeEntry(kind RangeKind, id uint32, queryStart mmapstore.Address) (mmapstore.RangeIndexEntry, bool, error) {
	var start, end mmapstore.Address
	var version uint8
	if kind == RangeAllocations {
		record, err := repository.db.Allocation(id)
		if err != nil {
			return mmapstore.RangeIndexEntry{}, false, err
		}
		start, end, version = record.Start, record.End, record.Version
	} else {
		record, err := repository.db.Route(id)
		if err != nil {
			return mmapstore.RangeIndexEntry{}, false, err
		}
		start, end = prefixRangeForRecord(record)
		version = record.Version
	}
	if bytes.Compare(start[:], queryStart[:]) >= 0 || bytes.Compare(end[:], queryStart[:]) < 0 {
		return mmapstore.RangeIndexEntry{}, false, nil
	}
	entry := mmapstore.RangeIndexEntry{ID: id}
	mmapstore.PutRangeKey(entry.Key[:], version, start, end, id)
	return entry, true, nil
}

func prefixRangeForRecord(record mmapstore.Route) (mmapstore.Address, mmapstore.Address) {
	start := record.Network
	end := start
	bits, offset := 128, 0
	if record.Version == 4 {
		bits, offset = 32, 12
	}
	for bit := int(record.PrefixLength); bit < bits; bit++ {
		end[offset+bit/8] |= 1 << uint(7-bit%8)
	}
	return start, end
}

func encodeRangeCursor(phase byte, kind RangeKind, start, end mmapstore.Address, key [mmapstore.RangeKeySize]byte) nullableString {
	payload := make([]byte, rangeCursorSize)
	payload[0] = phase
	payload[1] = rangeKindValue(kind)
	copy(payload[2:18], start[:])
	copy(payload[18:34], end[:])
	copy(payload[34:], key[:])
	encoded := base64.RawURLEncoding.EncodeToString(payload)
	return &encoded
}

func decodeRangeCursor(value string) (rangeCursor, error) {
	if value == "" {
		return rangeCursor{}, nil
	}
	payload, err := base64.RawURLEncoding.DecodeString(value)
	if err != nil || len(payload) != rangeCursorSize || (payload[0] != rangeCursorCovering && payload[0] != rangeCursorDirect) {
		return rangeCursor{}, errInvalidRangeCursor
	}
	result := rangeCursor{phase: payload[0], kind: rangeKindFromValue(payload[1])}
	if result.kind == "" {
		return rangeCursor{}, errInvalidRangeCursor
	}
	copy(result.queryStart[:], payload[2:18])
	copy(result.queryEnd[:], payload[18:34])
	copy(result.key[:], payload[34:])
	return result, nil
}

func rangeKindValue(kind RangeKind) byte {
	if kind == RangeAllocations {
		return 1
	}
	return 2
}

func rangeKindFromValue(value byte) RangeKind {
	switch value {
	case 1:
		return RangeAllocations
	case 2:
		return RangeRoutes
	default:
		return ""
	}
}

func (repository *MmapRepository) LookupRangeSummary(ctx context.Context, value ipkey.ParsedRange, kind RangeKind) (*RangeResponse, error) {
	keys, ok := ipkey.SummaryPrefixKeys(value)
	if !ok {
		return nil, fmt.Errorf("range is not eligible for a generated summary")
	}
	response := &RangeResponse{Range: rangeDescriptor(value), Kind: kind, Mode: "summary"}
	summary := &RangeSummary{
		Aggregation:        "overlapping_source_records",
		BucketPrefixLength: summaryBucketLength(keys[0]),
		Buckets:            len(keys),
		Countries:          []RangeFacet{},
		ASNs:               []RangeFacet{},
	}
	countries, asns := make(map[string]int64), make(map[string]int64)
	for _, key := range keys {
		if err := contextError(ctx); err != nil {
			return nil, fmt.Errorf("mmap range summary lookup: %w", err)
		}
		prefix, err := netip.ParsePrefix(key)
		if err != nil {
			return nil, fmt.Errorf("mmap range summary lookup: %w", err)
		}
		item, found, err := repository.db.Summary(prefix)
		if err != nil {
			return nil, fmt.Errorf("mmap range summary lookup: %w", err)
		}
		if !found {
			continue
		}
		summary.AllocationRecords += int64(item.AllocationRecords)
		summary.RouteRecords += int64(item.Routes)
		for _, facet := range item.Countries {
			countries[facet.Value] += int64(facet.Count)
		}
		for _, facet := range item.ASNs {
			asns[facet.Value] += int64(facet.Count)
		}
	}
	summary.Countries, summary.ASNs = rankFacets(countries), rankFacets(asns)
	response.Summary = summary
	return response, nil
}

func (repository *MmapRepository) LookupASN(ctx context.Context, asn uint32, page Page) (*ASNResponse, error) {
	var autnum *AutnumObject
	record, found, err := repository.db.Autnum(asn)
	if err != nil {
		return nil, fmt.Errorf("mmap ASN lookup: %w", err)
	}
	if found {
		object := autnumObject(record)
		autnum = &object
	}

	total, ids, err := repository.db.ASNRouteSlice(asn, page.Numbered, page.Number, page.Cursor, page.Limit)
	if err != nil {
		return nil, fmt.Errorf("mmap ASN lookup: %w", err)
	}
	if autnum == nil && total == 0 {
		return nil, nil
	}

	routes := make([]RouteObject, 0, len(ids))
	for _, id := range ids {
		if err := contextError(ctx); err != nil {
			return nil, fmt.Errorf("mmap ASN lookup: %w", err)
		}
		rec, err := repository.db.Route(id)
		if err != nil {
			return nil, fmt.Errorf("mmap ASN lookup: %w", err)
		}
		routes = append(routes, routeObject(id, rec))
	}

	response := &ASNResponse{
		ASN:      "AS" + strconv.FormatUint(uint64(asn), 10),
		ASNumber: int(asn),
		Autnum:   autnum,
	}
	if page.Numbered {
		response.Routes = RoutePage{
			Items:      routes,
			Page:       page.Number,
			TotalPages: int((total + int64(page.Limit) - 1) / int64(page.Limit)),
			TotalItems: total,
		}
	} else {
		routes, next := routePage(routes, page.Limit)
		response.Routes = RoutePage{Items: routes, NextCursor: next}
	}
	return response, nil
}

func (repository *MmapRepository) coveringAllocation(ctx context.Context, prefix ipkey.ParsedPrefix) (*AllocationObject, error) {
	ids, err := repository.db.MatchingPrefixIDs(ctx, mmapstore.TableAllocations, netip.MustParseAddr(prefix.Start.Canonical), 0)
	if err != nil {
		return nil, err
	}
	start := mmapstore.AddressFromAddr(netip.MustParseAddr(prefix.Start.Canonical))
	end := mmapstore.AddressFromAddr(netip.MustParseAddr(prefix.End.Canonical))
	var selected mmapstore.Allocation
	var selectedID uint32
	for _, id := range ids {
		record, err := repository.db.Allocation(id)
		if err != nil {
			return nil, err
		}
		if bytes.Compare(record.Start[:], start[:]) > 0 || bytes.Compare(record.End[:], end[:]) < 0 {
			continue
		}
		if selectedID == 0 || bytes.Compare(record.Start[:], selected.Start[:]) > 0 || (record.Start == selected.Start && (bytes.Compare(record.End[:], selected.End[:]) < 0 || (record.End == selected.End && id < selectedID))) {
			selected, selectedID = record, id
		}
	}
	if selectedID == 0 {
		return nil, nil
	}
	object := allocationObject(selectedID, selected)
	return &object, nil
}

func allocationObject(id uint32, value mmapstore.Allocation) AllocationObject {
	return AllocationObject{
		ID:             int64(id),
		StartIP:        value.Start.Addr(int(value.Version)).String(),
		EndIP:          value.End.Addr(int(value.Version)).String(),
		Version:        int(value.Version),
		Registry:       lower(optional(value.Registry)),
		CountryCode:    countryCode(upper(optional(value.Country))),
		CountryRaw:     upper(optional(value.Country)),
		Name:           optional(value.Name),
		Status:         optional(value.Status),
		AllocationDate: optional(value.AllocationDate),
		Created:        optional(value.Created),
		LastModified:   optional(value.LastModified),
		Source:         optional(value.Source),
		Maintainers:    optional(value.Maintainers),
		Organization:   optional(value.Organization),
		AbuseContact:   optional(value.AbuseContact),
		Description:    optional(value.Description),
	}
}

func routeObject(id uint32, value mmapstore.Route) RouteObject {
	var asnNumber *int
	if value.ASNumber > 0 {
		number := int(value.ASNumber)
		asnNumber = &number
	}
	return RouteObject{
		ID:           int64(id),
		Prefix:       recordPrefix(value),
		Version:      int(value.Version),
		OriginASN:    value.OriginASN,
		ASNumber:     asnNumber,
		Registry:     lower(optional(value.Registry)),
		Source:       optional(value.Source),
		Maintainers:  optional(value.Maintainers),
		Organization: optional(value.Organization),
		AbuseContact: optional(value.AbuseContact),
		Description:  optional(value.Description),
	}
}

func autnumObject(value mmapstore.Autnum) AutnumObject {
	country := upper(optional(value.Country))
	return AutnumObject{
		ASN:          value.ASN,
		ASNumber:     int(value.ASNumber),
		Registry:     lower(optional(value.Registry)),
		CountryCode:  countryCode(country),
		CountryRaw:   country,
		Name:         optional(value.Name),
		Organization: optional(value.Organization),
		Status:       optional(value.Status),
		Created:      optional(value.Created),
		LastModified: optional(value.LastModified),
		Source:       optional(value.Source),
		Maintainers:  optional(value.Maintainers),
		AbuseContact: optional(value.AbuseContact),
		Description:  optional(value.Description),
	}
}

func recordPrefix(value mmapstore.Route) string {
	return netip.PrefixFrom(value.Network.Addr(int(value.Version)), int(value.PrefixLength)).String()
}

func optional(value string) nullableString {
	value = strings.TrimSpace(value)
	if value == "" {
		return nil
	}
	return &value
}

func routePrefixContains(route string, query ipkey.ParsedPrefix) bool {
	prefix, err := netip.ParsePrefix(route)
	return err == nil && prefix.Contains(netip.MustParseAddr(query.Start.Canonical)) && prefix.Contains(netip.MustParseAddr(query.End.Canonical))
}

func contextError(ctx context.Context) error {
	select {
	case <-ctx.Done():
		return ctx.Err()
	default:
		return nil
	}
}

func (repository *MmapRepository) acquireOverlapSlot(ctx context.Context) error {
	select {
	case repository.overlapSlots <- struct{}{}:
		return nil
	case <-ctx.Done():
		return ctx.Err()
	}
}

func (repository *MmapRepository) releaseOverlapSlot() { <-repository.overlapSlots }

func rankFacets(values map[string]int64) []RangeFacet {
	items := make([]RangeFacet, 0, len(values))
	for value, count := range values {
		items = append(items, RangeFacet{Value: value, RecordCount: count})
	}
	sort.Slice(items, func(left, right int) bool {
		if items[left].RecordCount != items[right].RecordCount {
			return items[left].RecordCount > items[right].RecordCount
		}
		return items[left].Value < items[right].Value
	})
	if len(items) > 10 {
		return items[:10]
	}
	return items
}

func summaryBucketLength(key string) int {
	index := strings.LastIndexByte(key, '/')
	if index == -1 {
		return 0
	}
	length, _ := strconv.Atoi(key[index+1:])
	return length
}

func routePage(items []RouteObject, limit int) ([]RouteObject, nullableString) {
	if len(items) <= limit {
		return items, nil
	}
	items = items[:limit]
	cursor := strconv.FormatInt(items[len(items)-1].ID, 10)
	return items, &cursor
}

func prefixDescriptor(prefix ipkey.ParsedPrefix) PrefixDescriptor {
	return PrefixDescriptor{
		CIDR: prefix.Canonical, Version: prefix.Version, PrefixLength: prefix.PrefixLength,
		StartIP: prefix.Start.Canonical, EndIP: prefix.End.Canonical, AddressCount: prefix.AddressCount,
	}
}

func rangeDescriptor(value ipkey.ParsedRange) RangeDescriptor {
	return RangeDescriptor{
		StartIP: value.Start.Canonical, EndIP: value.End.Canonical,
		Version: value.Version, AddressCount: value.AddressCount,
	}
}
