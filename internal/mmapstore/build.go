package mmapstore

import (
	"bufio"
	"bytes"
	"context"
	"encoding/binary"
	"encoding/csv"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"math/big"
	"math/bits"
	"net/netip"
	"os"
	"path/filepath"
	"runtime"
	"sort"
	"strconv"
	"strings"
)

const buildWriteBufferSize = 1 << 20

type BuildOptions struct {
	InputDir     string
	OutputPath   string
	ReleaseTag   string
	BuiltAt      string
	SourceCommit string
}

type BuildStats struct {
	Allocations, Routes, Geofeeds, Autnums                     uint64
	AllocationIndex, RouteIndex, GeofeedIndex                  uint64
	AllocationRangeIndex, RouteRangeIndex, MaterializedSummary uint64
	RouteLPMNodes, RouteLPMIDs, RouteLPMBytes                  uint64
	AllocationLPMNodes, AllocationLPMBytes                     uint64
	GeofeedLPMNodes, GeofeedLPMBytes                           uint64
	StringPoolEntries, StringPoolBytes                         uint64
	FileBytes                                                  uint64
}

type summaryAccumulator struct {
	allocations, routes uint64
	countries, asns     map[string]uint64
}

type summaryKey struct {
	network uint32
	length  uint8
}

type mmapBuilder struct {
	file      *os.File
	buf       *bufio.Writer
	pos       uint64
	sections  [sectionCount]sectionSpec
	pool      *stringPool
	summaries map[summaryKey]*summaryAccumulator
	stats     BuildStats
}

func Build(ctx context.Context, options BuildOptions) (BuildStats, error) {
	if options.InputDir == "" || options.OutputPath == "" {
		return BuildStats{}, errors.New("input directory and output path are required")
	}
	for _, name := range []string{"allocations.csv", "routes.csv", "autnums.csv", "geolocations.csv"} {
		if info, err := os.Stat(filepath.Join(options.InputDir, name)); err != nil || info.IsDir() {
			if err == nil {
				err = errors.New("path is a directory")
			}
			return BuildStats{}, fmt.Errorf("open %s: %w", name, err)
		}
	}
	if err := os.MkdirAll(filepath.Dir(options.OutputPath), 0o755); err != nil {
		return BuildStats{}, err
	}
	tmpPath := options.OutputPath + ".building"
	if err := os.Remove(tmpPath); err != nil && !os.IsNotExist(err) {
		return BuildStats{}, fmt.Errorf("remove stale build file %s: %w", tmpPath, err)
	}
	file, err := os.OpenFile(tmpPath, os.O_CREATE|os.O_RDWR|os.O_TRUNC, 0o644)
	if err != nil {
		return BuildStats{}, err
	}

	b := &mmapBuilder{
		file:      file,
		buf:       bufio.NewWriterSize(file, buildWriteBufferSize),
		pool:      newStringPool(),
		summaries: make(map[summaryKey]*summaryAccumulator, 65793),
	}
	failed := true
	defer func() {
		_ = file.Close()
		if failed {
			_ = os.Remove(tmpPath)
		}
	}()

	var zeroHeader [HeaderSize]byte
	if _, err := b.buf.Write(zeroHeader[:]); err != nil {
		return BuildStats{}, err
	}
	b.pos = HeaderSize

	if err := b.buildAllocations(ctx, filepath.Join(options.InputDir, "allocations.csv")); err != nil {
		return BuildStats{}, err
	}
	runtime.GC()

	if err := b.buildRoutes(ctx, filepath.Join(options.InputDir, "routes.csv")); err != nil {
		return BuildStats{}, err
	}
	runtime.GC()

	if err := b.buildGeofeeds(ctx, filepath.Join(options.InputDir, "geolocations.csv")); err != nil {
		return BuildStats{}, err
	}
	runtime.GC()

	if err := b.buildAutnums(ctx, filepath.Join(options.InputDir, "autnums.csv")); err != nil {
		return BuildStats{}, err
	}
	runtime.GC()

	if err := b.buildStringPool(); err != nil {
		return BuildStats{}, err
	}
	runtime.GC()

	if err := b.buildSummaries(ctx); err != nil {
		return BuildStats{}, err
	}
	runtime.GC()

	meta := Metadata{
		SchemaVersion: SchemaVersion,
		ReleaseTag:    options.ReleaseTag,
		BuiltAt:       options.BuiltAt,
		SourceCommit:  options.SourceCommit,
	}
	metaBytes, err := json.Marshal(meta)
	if err != nil {
		return BuildStats{}, err
	}
	if err := b.writeSectionBytes(secMetadata, metaBytes); err != nil {
		return BuildStats{}, err
	}
	if err := b.buf.Flush(); err != nil {
		return BuildStats{}, err
	}
	b.stats.FileBytes = b.pos

	var header [HeaderSize]byte
	copy(header[:4], Magic[:])
	binary.BigEndian.PutUint32(header[4:8], FormatVersion)
	binary.BigEndian.PutUint32(header[8:12], sectionCount)
	for index, spec := range b.sections {
		off := headerFixedSize + index*sectionEntrySize
		binary.BigEndian.PutUint64(header[off:off+8], spec.offset)
		binary.BigEndian.PutUint64(header[off+8:off+16], spec.size)
	}
	if _, err := b.file.WriteAt(header[:], 0); err != nil {
		return BuildStats{}, err
	}
	if err := b.file.Sync(); err != nil {
		return BuildStats{}, err
	}
	if err := b.file.Close(); err != nil {
		return BuildStats{}, err
	}
	if err := os.Rename(tmpPath, options.OutputPath); err != nil {
		return BuildStats{}, err
	}
	failed = false
	return b.stats, nil
}

func (b *mmapBuilder) align8() error {
	rem := b.pos & 7
	if rem == 0 {
		return nil
	}
	pad := 8 - rem
	var zeros [8]byte
	if _, err := b.buf.Write(zeros[:pad]); err != nil {
		return err
	}
	b.pos += pad
	return nil
}

func (b *mmapBuilder) beginSection() (uint64, error) {
	if err := b.align8(); err != nil {
		return 0, err
	}
	return b.pos, nil
}

func (b *mmapBuilder) endSection(secIdx int, startOffset uint64) {
	b.sections[secIdx] = sectionSpec{offset: startOffset, size: b.pos - startOffset}
}

func (b *mmapBuilder) writeSectionBytes(secIdx int, data []byte) error {
	start, err := b.beginSection()
	if err != nil {
		return err
	}
	if _, err := b.buf.Write(data); err != nil {
		return err
	}
	b.pos += uint64(len(data))
	b.endSection(secIdx, start)
	return nil
}

func (b *mmapBuilder) writeUint64ArraySection(secIdx int, values []uint64) error {
	start, err := b.beginSection()
	if err != nil {
		return err
	}
	var scratch [8]byte
	for _, value := range values {
		binary.BigEndian.PutUint64(scratch[:], value)
		if _, err := b.buf.Write(scratch[:]); err != nil {
			return err
		}
		b.pos += 8
	}
	b.endSection(secIdx, start)
	return nil
}

func (b *mmapBuilder) buildAllocations(ctx context.Context, path string) error {
	dataStart, err := b.beginSection()
	if err != nil {
		return err
	}
	offsets := []uint64{0}
	var rangeV4, rangeV6, prefixV4, prefixV6 []byte
	var recBuf []byte
	var dataPos uint64

	err = forEachCSVRow(ctx, path, func(row []string) error {
		if len(row) < 6 {
			return nil
		}
		version, start, end, ok := parseRange(row[0], row[1], row[2])
		if !ok {
			return nil
		}
		id, err := nextRecordID(b.stats.Allocations)
		if err != nil {
			return err
		}
		record := Allocation{
			Start: start, End: end, Version: version,
			Registry: at(row, 3), Country: at(row, 4), Name: at(row, 5),
			Status: at(row, 6), AllocationDate: at(row, 7), Created: at(row, 8),
			LastModified: at(row, 9), Source: at(row, 10), Maintainers: at(row, 11),
			Organization: at(row, 12), AbuseContact: at(row, 13), Description: at(row, 14),
		}
		recBuf = appendAllocation(recBuf[:0], b.pool, record)
		if _, err := b.buf.Write(recBuf); err != nil {
			return err
		}
		dataPos += uint64(len(recBuf))
		b.pos += uint64(len(recBuf))
		offsets = append(offsets, dataPos)

		if version == 4 {
			rangeV4 = appendRangeV4(rangeV4, start, end, id)
		} else {
			rangeV6 = appendRangeV6(rangeV6, start, end, id)
		}
		b.stats.AllocationRangeIndex++

		prefixes, err := rangePrefixes(start, end, int(version))
		if err != nil {
			return err
		}
		for _, prefix := range prefixes {
			if version == 4 {
				prefixV4 = appendPrefixV4(prefixV4, prefix.Addr().As4(), uint8(prefix.Bits()), id)
			} else {
				prefixV6 = appendPrefixV6(prefixV6, prefix.Addr().As16(), uint8(prefix.Bits()), id)
			}
			b.stats.AllocationIndex++
		}
		b.collectSummary(start, end, int(version), false, record.Country)
		b.stats.Allocations = uint64(id)
		return nil
	})
	if err != nil {
		return err
	}
	b.endSection(secAllocationData, dataStart)

	if err := b.writeUint64ArraySection(secAllocationOffsets, offsets); err != nil {
		return err
	}
	offsets = nil

	if err := b.writeSelectionLPMFromRanges(ctx, 4, secAllocLPMv4, rangeV4, true); err != nil {
		return err
	}
	runtime.GC()
	if err := b.writeSelectionLPMFromRanges(ctx, 6, secAllocLPMv6, rangeV6, true); err != nil {
		return err
	}
	runtime.GC()

	sortFixedRecords(rangeV4, rangeV4Stride)
	if err := b.writeSectionBytes(secAllocRangeV4, rangeV4); err != nil {
		return err
	}
	rangeV4 = nil

	sortFixedRecords(rangeV6, rangeV6Stride)
	if err := b.writeSectionBytes(secAllocRangeV6, rangeV6); err != nil {
		return err
	}
	rangeV6 = nil

	sortFixedRecords(prefixV4, prefixV4Stride)
	if err := b.writeSectionBytes(secAllocPrefixV4, prefixV4); err != nil {
		return err
	}
	prefixV4 = nil

	sortFixedRecords(prefixV6, prefixV6Stride)
	if err := b.writeSectionBytes(secAllocPrefixV6, prefixV6); err != nil {
		return err
	}
	prefixV6 = nil
	return nil
}

func (b *mmapBuilder) buildRoutes(ctx context.Context, path string) error {
	dataStart, err := b.beginSection()
	if err != nil {
		return err
	}
	offsets := []uint64{0}
	asnRoutes := make(map[uint32][]uint32)
	var rangeV4, rangeV6, prefixV4, prefixV6 []byte
	var recBuf []byte
	var dataPos uint64

	err = forEachCSVRow(ctx, path, func(row []string) error {
		if len(row) < 5 {
			return nil
		}
		prefix, err := netip.ParsePrefix(strings.TrimSpace(row[3]))
		if err != nil || prefix.Addr().Zone() != "" {
			return nil
		}
		prefix = prefix.Masked()
		version := uint8(6)
		if prefix.Addr().Is4() {
			version = 4
		}
		origin := strings.ToUpper(at(row, 4))
		if origin == "" {
			return nil
		}
		asn := parseASN(origin)
		id, err := nextRecordID(b.stats.Routes)
		if err != nil {
			return err
		}
		record := Route{
			Network: AddressFromAddr(prefix.Addr()), Version: version,
			PrefixLength: uint8(prefix.Bits()), ASNumber: asn, OriginASN: origin,
			Registry: at(row, 5), Source: at(row, 6), Maintainers: at(row, 7),
			Organization: at(row, 8), AbuseContact: at(row, 9), Description: at(row, 10),
		}
		recBuf = appendRoute(recBuf[:0], b.pool, record)
		if _, err := b.buf.Write(recBuf); err != nil {
			return err
		}
		dataPos += uint64(len(recBuf))
		b.pos += uint64(len(recBuf))
		offsets = append(offsets, dataPos)

		if asn != 0 {
			asnRoutes[asn] = append(asnRoutes[asn], id)
		}
		start, end := prefixRange(prefix)
		if version == 4 {
			rangeV4 = appendRangeV4(rangeV4, start, end, id)
			prefixV4 = appendPrefixV4(prefixV4, prefix.Addr().As4(), uint8(prefix.Bits()), id)
		} else {
			rangeV6 = appendRangeV6(rangeV6, start, end, id)
			prefixV6 = appendPrefixV6(prefixV6, prefix.Addr().As16(), uint8(prefix.Bits()), id)
		}
		b.collectSummary(start, end, int(version), true, record.OriginASN)
		b.stats.RouteIndex++
		b.stats.RouteRangeIndex++
		b.stats.Routes = uint64(id)
		return nil
	})
	if err != nil {
		return err
	}
	b.endSection(secRouteData, dataStart)

	if err := b.writeUint64ArraySection(secRouteOffsets, offsets); err != nil {
		return err
	}
	offsets = nil

	sortFixedRecords(prefixV4, prefixV4Stride)
	if err := b.writeRouteLPMFromPrefixes(ctx, 4, secRouteLPMv4, prefixV4, prefixV4Stride); err != nil {
		return err
	}
	if err := b.writeSectionBytes(secRoutePrefixV4, prefixV4); err != nil {
		return err
	}
	prefixV4 = nil
	runtime.GC()

	sortFixedRecords(prefixV6, prefixV6Stride)
	if err := b.writeRouteLPMFromPrefixes(ctx, 6, secRouteLPMv6, prefixV6, prefixV6Stride); err != nil {
		return err
	}
	if err := b.writeSectionBytes(secRoutePrefixV6, prefixV6); err != nil {
		return err
	}
	prefixV6 = nil
	runtime.GC()

	sortFixedRecords(rangeV4, rangeV4Stride)
	if err := b.writeSectionBytes(secRouteRangeV4, rangeV4); err != nil {
		return err
	}
	rangeV4 = nil

	sortFixedRecords(rangeV6, rangeV6Stride)
	if err := b.writeSectionBytes(secRouteRangeV6, rangeV6); err != nil {
		return err
	}
	rangeV6 = nil

	return b.writeASNRoutes(ctx, asnRoutes)
}

func (b *mmapBuilder) writeASNRoutes(ctx context.Context, asnRoutes map[uint32][]uint32) error {
	asns := make([]uint32, 0, len(asnRoutes))
	for asn := range asnRoutes {
		asns = append(asns, asn)
	}
	sort.Slice(asns, func(i, j int) bool { return asns[i] < asns[j] })

	idsStart, err := b.beginSection()
	if err != nil {
		return err
	}
	index := make([]byte, len(asns)*asnRouteIndexStride)
	var firstIdx uint64
	var idBuf [4]byte
	for i, asn := range asns {
		if err := contextError(ctx); err != nil {
			return err
		}
		ids := asnRoutes[asn]
		off := i * asnRouteIndexStride
		binary.BigEndian.PutUint32(index[off:], asn)
		binary.BigEndian.PutUint64(index[off+4:], firstIdx)
		binary.BigEndian.PutUint32(index[off+12:], uint32(len(ids)))
		for _, id := range ids {
			binary.BigEndian.PutUint32(idBuf[:], id)
			if _, err := b.buf.Write(idBuf[:]); err != nil {
				return err
			}
			b.pos += 4
		}
		firstIdx += uint64(len(ids))
	}
	b.endSection(secASNRouteIDs, idsStart)
	return b.writeSectionBytes(secASNRouteIndex, index)
}

func (b *mmapBuilder) buildGeofeeds(ctx context.Context, path string) error {
	dataStart, err := b.beginSection()
	if err != nil {
		return err
	}
	offsets := []uint64{0}
	var rangeV4, rangeV6, prefixV4, prefixV6 []byte
	var recBuf []byte
	var dataPos uint64

	err = forEachCSVRow(ctx, path, func(row []string) error {
		if len(row) < 6 {
			return nil
		}
		version, start, end, ok := parseRange(row[0], row[1], row[2])
		if !ok {
			return nil
		}
		id, err := nextRecordID(b.stats.Geofeeds)
		if err != nil {
			return err
		}
		record := Geofeed{
			Start: start, End: end, Version: version,
			Country: at(row, 3), Region: at(row, 4), City: at(row, 5),
		}
		recBuf = appendGeofeed(recBuf[:0], b.pool, record)
		if _, err := b.buf.Write(recBuf); err != nil {
			return err
		}
		dataPos += uint64(len(recBuf))
		b.pos += uint64(len(recBuf))
		offsets = append(offsets, dataPos)

		if version == 4 {
			rangeV4 = appendRangeV4(rangeV4, start, end, id)
		} else {
			rangeV6 = appendRangeV6(rangeV6, start, end, id)
		}
		prefixes, err := rangePrefixes(start, end, int(version))
		if err != nil {
			return err
		}
		for _, prefix := range prefixes {
			if version == 4 {
				prefixV4 = appendPrefixV4(prefixV4, prefix.Addr().As4(), uint8(prefix.Bits()), id)
			} else {
				prefixV6 = appendPrefixV6(prefixV6, prefix.Addr().As16(), uint8(prefix.Bits()), id)
			}
			b.stats.GeofeedIndex++
		}
		b.stats.Geofeeds = uint64(id)
		return nil
	})
	if err != nil {
		return err
	}
	b.endSection(secGeofeedData, dataStart)

	if err := b.writeUint64ArraySection(secGeofeedOffsets, offsets); err != nil {
		return err
	}
	offsets = nil

	if err := b.writeSelectionLPMFromRanges(ctx, 4, secGeoLPMv4, rangeV4, false); err != nil {
		return err
	}
	rangeV4 = nil
	runtime.GC()

	if err := b.writeSelectionLPMFromRanges(ctx, 6, secGeoLPMv6, rangeV6, false); err != nil {
		return err
	}
	rangeV6 = nil
	runtime.GC()

	sortFixedRecords(prefixV4, prefixV4Stride)
	if err := b.writeSectionBytes(secGeofeedPrefixV4, prefixV4); err != nil {
		return err
	}
	prefixV4 = nil

	sortFixedRecords(prefixV6, prefixV6Stride)
	if err := b.writeSectionBytes(secGeofeedPrefixV6, prefixV6); err != nil {
		return err
	}
	prefixV6 = nil
	return nil
}

func (b *mmapBuilder) buildAutnums(ctx context.Context, path string) error {
	type encodedAutnum struct {
		asn  uint32
		data []byte
	}
	seen := make(map[uint32]struct{})
	var items []encodedAutnum

	err := forEachCSVRow(ctx, path, func(row []string) error {
		if len(row) < 12 {
			return nil
		}
		asn := parseASN(at(row, 0))
		if asn == 0 {
			return nil
		}
		if _, exists := seen[asn]; exists {
			return nil
		}
		seen[asn] = struct{}{}
		record := Autnum{
			ASNumber: asn, ASN: strings.ToUpper(at(row, 0)),
			Registry: at(row, 1), Country: at(row, 2), Name: at(row, 3),
			Organization: at(row, 4), Status: at(row, 5), Created: at(row, 6),
			LastModified: at(row, 7), Source: at(row, 8), Maintainers: at(row, 9),
			AbuseContact: at(row, 10), Description: at(row, 11),
		}
		items = append(items, encodedAutnum{asn: asn, data: appendAutnum(nil, b.pool, record)})
		b.stats.Autnums++
		return nil
	})
	if err != nil {
		return err
	}
	seen = nil
	sort.Slice(items, func(i, j int) bool { return items[i].asn < items[j].asn })

	dataStart, err := b.beginSection()
	if err != nil {
		return err
	}
	index := make([]byte, len(items)*autnumIndexStride)
	var dataOffset uint64
	for i, item := range items {
		if _, err := b.buf.Write(item.data); err != nil {
			return err
		}
		b.pos += uint64(len(item.data))
		off := i * autnumIndexStride
		binary.BigEndian.PutUint32(index[off:], item.asn)
		binary.BigEndian.PutUint64(index[off+4:], dataOffset)
		binary.BigEndian.PutUint32(index[off+12:], uint32(len(item.data)))
		dataOffset += uint64(len(item.data))
	}
	b.endSection(secAutnumData, dataStart)
	return b.writeSectionBytes(secAutnumIndex, index)
}

func (b *mmapBuilder) buildStringPool() error {
	b.pool.index = nil // free map before writing
	b.stats.StringPoolEntries = uint64(len(b.pool.offsets) - 1)
	b.stats.StringPoolBytes = uint64(len(b.pool.data))

	if err := b.writeUint64ArraySection(secStringPoolOffsets, b.pool.offsets); err != nil {
		return err
	}
	if err := b.writeSectionBytes(secStringPoolData, b.pool.data); err != nil {
		return err
	}
	b.pool = nil
	return nil
}

func (b *mmapBuilder) writeSelectionLPMFromRanges(ctx context.Context, version uint8, secIdx int, rangeData []byte, allocation bool) error {
	selector := newSelectionLPMBuilder(version)
	stride := rangeV6Stride
	addrBytes := 16
	if version == 4 {
		stride = rangeV4Stride
		addrBytes = 4
	}
	for off := 0; off < len(rangeData); off += stride {
		if off%16384 == 0 {
			if err := contextError(ctx); err != nil {
				return err
			}
		}
		start := addressFromSlice(rangeData[off:off+addrBytes], version)
		end := addressFromSlice(rangeData[off+addrBytes:off+2*addrBytes], version)
		id := binary.BigEndian.Uint32(rangeData[off+2*addrBytes : off+stride])
		prefixes, err := rangePrefixes(start, end, int(version))
		if err != nil {
			return err
		}
		for _, prefix := range prefixes {
			if err := selector.Insert(prefix, id, start, end); err != nil {
				return err
			}
		}
	}
	encoded, stats, err := selector.Encode()
	if err != nil {
		return err
	}
	if err := b.writeSectionBytes(secIdx, encoded); err != nil {
		return err
	}
	if allocation {
		b.stats.AllocationLPMNodes += stats.Nodes
		b.stats.AllocationLPMBytes += stats.Bytes
	} else {
		b.stats.GeofeedLPMNodes += stats.Nodes
		b.stats.GeofeedLPMBytes += stats.Bytes
	}
	return nil
}

func (b *mmapBuilder) writeRouteLPMFromPrefixes(ctx context.Context, version uint8, secIdx int, prefixData []byte, stride int) error {
	lpm := newRouteLPMBuilder(version)
	addrBytes := 16
	if version == 4 {
		addrBytes = 4
	}
	keyLen := addrBytes + 1
	var ids []uint32
	for off := 0; off < len(prefixData); {
		if err := contextError(ctx); err != nil {
			return err
		}
		key := prefixData[off : off+keyLen]
		ids = ids[:0]
		for off < len(prefixData) && bytes.Equal(prefixData[off:off+keyLen], key) {
			ids = append(ids, binary.BigEndian.Uint32(prefixData[off+keyLen:off+stride]))
			off += stride
		}
		addr := addressFromSlice(key[:addrBytes], version).Addr(int(version))
		prefix := netip.PrefixFrom(addr, int(key[addrBytes])).Masked()
		cp := make([]uint32, len(ids))
		copy(cp, ids)
		if err := lpm.Insert(prefix, cp); err != nil {
			return err
		}
	}
	encoded, stats, err := lpm.Encode()
	if err != nil {
		return err
	}
	if err := b.writeSectionBytes(secIdx, encoded); err != nil {
		return err
	}
	b.stats.RouteLPMNodes += stats.Nodes
	b.stats.RouteLPMIDs += stats.IDs
	b.stats.RouteLPMBytes += stats.Bytes
	return nil
}

func (b *mmapBuilder) buildSummaries(ctx context.Context) error {
	dataStart, err := b.beginSection()
	if err != nil {
		return err
	}
	totalSummaries := (1 << 17) - 1
	index := make([]byte, 0, totalSummaries*summaryIndexStride)
	var dataOffset uint64
	var payload []byte

	var emit func(network uint32, length int) error
	emit = func(network uint32, length int) error {
		if err := contextError(ctx); err != nil {
			return err
		}
		summary := b.summaryForPrefix(network, length)
		payload = appendSummary(payload[:0], summary)
		if _, err := b.buf.Write(payload); err != nil {
			return err
		}
		b.pos += uint64(len(payload))

		addr := network << uint(32-length)
		var entry [summaryIndexStride]byte
		binary.BigEndian.PutUint32(entry[:4], addr)
		entry[4] = byte(length)
		binary.BigEndian.PutUint64(entry[5:13], dataOffset)
		binary.BigEndian.PutUint32(entry[13:17], uint32(len(payload)))
		index = append(index, entry[:]...)
		dataOffset += uint64(len(payload))
		b.stats.MaterializedSummary++
		if length == 16 {
			return nil
		}
		child := network << 1
		if err := emit(child, length+1); err != nil {
			return err
		}
		return emit(child|1, length+1)
	}

	if err := emit(0, 0); err != nil {
		return err
	}
	b.endSection(secSummaryData, dataStart)
	b.summaries = nil
	sortFixedRecords(index, summaryIndexStride)
	return b.writeSectionBytes(secSummaryIndex, index)
}

func (b *mmapBuilder) collectSummary(start, end Address, version int, route bool, value string) {
	if version != 4 {
		return
	}
	startValue := binary.BigEndian.Uint32(start[12:])
	endValue := binary.BigEndian.Uint32(end[12:])
	value = strings.ToUpper(strings.TrimSpace(value))
	for _, length := range []uint{0, 8, 16} {
		first, last := uint64(startValue)>>uint(32-length), uint64(endValue)>>uint(32-length)
		for bucket := first; bucket <= last; bucket++ {
			key := summaryKey{network: uint32(bucket), length: uint8(length)}
			item := b.summaries[key]
			if item == nil {
				item = &summaryAccumulator{}
				b.summaries[key] = item
			}
			if route {
				item.routes++
				if value != "" {
					if item.asns == nil {
						item.asns = make(map[string]uint64)
					}
					item.asns[value]++
				}
			} else {
				item.allocations++
				if value != "" {
					if item.countries == nil {
						item.countries = make(map[string]uint64)
					}
					item.countries[value]++
				}
			}
		}
	}
}

func (b *mmapBuilder) summaryForPrefix(network uint32, length int) RangeSummary {
	if length == 0 || length == 8 || length == 16 {
		return summaryValue(b.summaries[summaryKey{network: network, length: uint8(length)}], length)
	}
	childLength := 8
	if length > 8 {
		childLength = 16
	}
	childCount := uint32(1) << uint(childLength-length)
	first := network << uint(childLength-length)
	merged := summaryAccumulator{}
	for child := first; child < first+childCount; child++ {
		childValue := summaryValue(b.summaries[summaryKey{network: child, length: uint8(childLength)}], childLength)
		mergeSummary(&merged, childValue)
	}
	return summaryValue(&merged, length)
}

func summaryValue(item *summaryAccumulator, length int) RangeSummary {
	if item == nil {
		return RangeSummary{PrefixLength: uint8(length)}
	}
	return RangeSummary{
		PrefixLength:      uint8(length),
		AllocationRecords: item.allocations,
		Routes:            item.routes,
		Countries:         rankedFacets(item.countries),
		ASNs:              rankedFacets(item.asns),
	}
}

func mergeSummary(destination *summaryAccumulator, source RangeSummary) {
	destination.allocations += source.AllocationRecords
	destination.routes += source.Routes
	if len(source.Countries) > 0 {
		if destination.countries == nil {
			destination.countries = make(map[string]uint64, len(source.Countries))
		}
		for _, facet := range source.Countries {
			destination.countries[facet.Value] += facet.Count
		}
	}
	if len(source.ASNs) > 0 {
		if destination.asns == nil {
			destination.asns = make(map[string]uint64, len(source.ASNs))
		}
		for _, facet := range source.ASNs {
			destination.asns[facet.Value] += facet.Count
		}
	}
}

func rankedFacets(values map[string]uint64) []Facet {
	items := make([]Facet, 0, len(values))
	for value, count := range values {
		items = append(items, Facet{Value: value, Count: count})
	}
	sort.Slice(items, func(left, right int) bool {
		if items[left].Count != items[right].Count {
			return items[left].Count > items[right].Count
		}
		return items[left].Value < items[right].Value
	})
	if len(items) > 10 {
		items = items[:10]
	}
	return items
}

// ---- Fixed-stride sorting and record slicing helpers ----

type fixedRecordSorter struct {
	data   []byte
	stride int
	tmp    [64]byte
}

func (s *fixedRecordSorter) Len() int { return len(s.data) / s.stride }
func (s *fixedRecordSorter) Less(i, j int) bool {
	a, b := i*s.stride, j*s.stride
	return bytes.Compare(s.data[a:a+s.stride], s.data[b:b+s.stride]) < 0
}
func (s *fixedRecordSorter) Swap(i, j int) {
	a, b := i*s.stride, j*s.stride
	copy(s.tmp[:s.stride], s.data[a:a+s.stride])
	copy(s.data[a:a+s.stride], s.data[b:b+s.stride])
	copy(s.data[b:b+s.stride], s.tmp[:s.stride])
}

func sortFixedRecords(data []byte, stride int) {
	if len(data)/stride <= 1 {
		return
	}
	sort.Sort(&fixedRecordSorter{data: data, stride: stride})
}

func appendPrefixV4(dst []byte, addr [4]byte, bits uint8, id uint32) []byte {
	dst = append(dst, addr[0], addr[1], addr[2], addr[3], bits)
	return binary.BigEndian.AppendUint32(dst, id)
}

func appendPrefixV6(dst []byte, addr [16]byte, bits uint8, id uint32) []byte {
	dst = append(dst, addr[:]...)
	dst = append(dst, bits)
	return binary.BigEndian.AppendUint32(dst, id)
}

func appendRangeV4(dst []byte, start, end Address, id uint32) []byte {
	dst = append(dst, start[12:16]...)
	dst = append(dst, end[12:16]...)
	return binary.BigEndian.AppendUint32(dst, id)
}

func appendRangeV6(dst []byte, start, end Address, id uint32) []byte {
	dst = append(dst, start[:]...)
	dst = append(dst, end[:]...)
	return binary.BigEndian.AppendUint32(dst, id)
}

func nextRecordID(count uint64) (uint32, error) {
	if count >= uint64(^uint32(0)) {
		return 0, errors.New("source record count exceeds uint32")
	}
	return uint32(count + 1), nil
}

func forEachCSVRow(ctx context.Context, path string, callback func([]string) error) error {
	file, err := os.Open(path)
	if err != nil {
		return err
	}
	defer file.Close()
	reader := csv.NewReader(file)
	reader.FieldsPerRecord = -1
	reader.ReuseRecord = true
	for i := 0; ; i++ {
		if i&1023 == 0 {
			if err := contextError(ctx); err != nil {
				return err
			}
		}
		row, err := reader.Read()
		if err == io.EOF {
			return nil
		}
		if err != nil {
			return fmt.Errorf("read %s: %w", path, err)
		}
		if err := callback(row); err != nil {
			return err
		}
	}
}

func parseRange(startValue, endValue, versionValue string) (uint8, Address, Address, bool) {
	version, err := strconv.Atoi(strings.TrimSpace(versionValue))
	if err != nil || (version != 4 && version != 6) {
		return 0, Address{}, Address{}, false
	}
	start, ok := addressFromSortKey(startValue, version)
	if !ok {
		return 0, Address{}, Address{}, false
	}
	end, ok := addressFromSortKey(endValue, version)
	if !ok || bytes.Compare(start[:], end[:]) > 0 {
		return 0, Address{}, Address{}, false
	}
	return uint8(version), start, end, true
}

func addressFromSortKey(value string, version int) (Address, bool) {
	number, ok := new(big.Int).SetString(strings.TrimSpace(value), 10)
	if !ok || number.Sign() < 0 || number.BitLen() > 128 {
		return Address{}, false
	}
	var result Address
	number.FillBytes(result[:])
	if version == 4 {
		for index := 0; index < 12; index++ {
			result[index] = 0
		}
	}
	return result, true
}

func rangePrefixes(start, end Address, version int) ([]netip.Prefix, error) {
	if version == 4 {
		cur := uint64(binary.BigEndian.Uint32(start[12:]))
		last := uint64(binary.BigEndian.Uint32(end[12:]))
		result := make([]netip.Prefix, 0, 8)
		for cur <= last {
			maxStep := uint64(1) << 32
			if cur != 0 {
				maxStep = cur & -cur
			}
			rem := last - cur + 1
			for maxStep > rem {
				maxStep >>= 1
			}
			length := 32 - bits.TrailingZeros64(maxStep)
			addr := netip.AddrFrom4([4]byte{byte(cur >> 24), byte(cur >> 16), byte(cur >> 8), byte(cur)})
			result = append(result, netip.PrefixFrom(addr, length))
			cur += maxStep
		}
		return result, nil
	}
	current := new(big.Int).SetBytes(start[:])
	last := new(big.Int).SetBytes(end[:])
	result := make([]netip.Prefix, 0, 8)
	for current.Cmp(last) <= 0 {
		blockSize := new(big.Int)
		if current.Sign() == 0 {
			blockSize.Lsh(big.NewInt(1), 128)
		} else {
			blockSize.And(current, new(big.Int).Neg(current))
		}
		remaining := new(big.Int).Sub(last, current)
		remaining.Add(remaining, big.NewInt(1))
		for blockSize.Cmp(remaining) > 0 {
			blockSize.Rsh(blockSize, 1)
		}
		length := 128 - (blockSize.BitLen() - 1)
		bytesValue := current.FillBytes(make([]byte, 16))
		prefix := netip.PrefixFrom(netip.AddrFrom16([16]byte(bytesValue)), length)
		result = append(result, prefix)
		current.Add(current, blockSize)
	}
	return result, nil
}

func prefixRange(prefix netip.Prefix) (Address, Address) {
	prefix = prefix.Masked()
	start := AddressFromAddr(prefix.Addr())
	end := start
	bitsCount := 128
	offset := 0
	if prefix.Addr().Is4() {
		bitsCount = 32
		offset = 12
	}
	for bit := prefix.Bits(); bit < bitsCount; bit++ {
		byteIndex := offset + bit/8
		end[byteIndex] |= 1 << uint(7-bit%8)
	}
	return start, end
}

func parseASN(value string) uint32 {
	value = strings.TrimPrefix(strings.ToUpper(strings.TrimSpace(value)), "AS")
	number, err := strconv.ParseUint(value, 10, 32)
	if err != nil || number == 0 {
		return 0
	}
	return uint32(number)
}

func at(row []string, index int) string {
	if index < 0 || index >= len(row) {
		return ""
	}
	return strings.TrimSpace(row[index])
}
