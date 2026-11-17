package api

import (
	"context"
	"encoding/csv"
	"errors"
	"os"
	"path/filepath"
	"reflect"
	"testing"

	"github.com/mehrnet/bgp-api/internal/ipkey"
	"github.com/mehrnet/bgp-api/internal/mmapstore"
)

func TestMmapRepositorySupportsCompleteAPIContract(t *testing.T) {
	repository := testMmapRepository(t)
	ctx := context.Background()

	ip, _ := ipkey.ParseRuntime("1.1.1.1")
	compact, err := repository.Lookup(ctx, ip, LookupOptions{})
	if err != nil {
		t.Fatal(err)
	}
	if compact == nil || compact.Details != nil {
		t.Fatalf("compact lookup = %#v", compact)
	}

	lookup, err := repository.Lookup(ctx, ip, LookupOptions{Details: LookupDetailsFull})
	if err != nil {
		t.Fatal(err)
	}
	if lookup == nil || lookup.Registry == nil || *lookup.Registry != "apnic" || len(lookup.Network.ASNs) != 2 || len(lookup.Network.Origins) != 2 || lookup.Network.Origins[0].Name == nil || *lookup.Network.Origins[0].Name != "CLOUDFLARENET" || lookup.Network.Origins[0].Organization == nil || *lookup.Network.Origins[0].Organization != "ORG-CLOUD14-ARIN" || lookup.Network.Origins[1].Name != nil || lookup.Location.City == nil || *lookup.Location.City != "South Brisbane" {
		t.Fatalf("lookup = %#v", lookup)
	}
	if lookup.Details == nil || len(lookup.Details.Allocations) != 2 || len(lookup.Details.Routes) != 2 || len(lookup.Details.Geofeeds) != 2 {
		t.Fatalf("lookup details = %#v", lookup.Details)
	}
	lookup.Details = nil
	if !reflect.DeepEqual(compact, lookup) {
		t.Fatalf("compact response diverged from details=full response:\ncompact=%#v\nfull=%#v", compact, lookup)
	}

	prefix, _ := ipkey.ParsePrefix("1.1.1.0/24")
	prefixResponse, err := repository.LookupPrefix(ctx, prefix, Page{Limit: 1})
	if err != nil {
		t.Fatal(err)
	}
	if prefixResponse.Allocation == nil || prefixResponse.Allocation.Name == nil || *prefixResponse.Allocation.Name != "APNIC-LABS" || len(prefixResponse.Routes.Items) != 1 || prefixResponse.Routes.NextCursor == nil {
		t.Fatalf("prefix response = %#v", prefixResponse)
	}

	rangeValue, _ := ipkey.ParseRange("1.1.1.1", "1.1.1.2")
	rangeResponse, err := repository.LookupRange(ctx, rangeValue, RangeAllocations, RangePage{Limit: 10})
	if err != nil {
		t.Fatal(err)
	}
	if len(rangeResponse.Allocations) != 2 || rangeResponse.Mode != "records" {
		t.Fatalf("range response = %#v", rangeResponse)
	}

	broadRange, _ := ipkey.ParseRange("1.0.0.0", "1.255.255.255")
	summary, err := repository.LookupRangeSummary(ctx, broadRange, RangeRoutes)
	if err != nil {
		t.Fatal(err)
	}
	if summary.Summary == nil || summary.Summary.RouteRecords != 2 || summary.Summary.AllocationRecords != 2 {
		t.Fatalf("summary = %#v", summary.Summary)
	}
	if summary.Summary.BucketPrefixLength != 8 || summary.Summary.Buckets != 1 {
		t.Fatalf("materialized summary = %#v", summary.Summary)
	}
	intermediateRange, _ := ipkey.ParseRange("1.0.0.0", "1.31.255.255")
	intermediateSummary, err := repository.LookupRangeSummary(ctx, intermediateRange, RangeAllocations)
	if err != nil {
		t.Fatal(err)
	}
	if intermediateSummary.Summary == nil || intermediateSummary.Summary.BucketPrefixLength != 11 || intermediateSummary.Summary.Buckets != 1 || intermediateSummary.Summary.AllocationRecords != 33 || intermediateSummary.Summary.RouteRecords != 2 {
		t.Fatalf("intermediate materialized summary = %#v", intermediateSummary.Summary)
	}

	asnResponse, err := repository.LookupASN(ctx, 13335, Page{Limit: 1, Number: 1, Numbered: true})
	if err != nil {
		t.Fatal(err)
	}
	if asnResponse == nil || asnResponse.Autnum == nil || asnResponse.Autnum.Name == nil || *asnResponse.Autnum.Name != "CLOUDFLARENET" || asnResponse.Routes.TotalItems != 1 || asnResponse.Routes.TotalPages != 1 {
		t.Fatalf("ASN response = %#v", asnResponse)
	}

	metadata, err := repository.DatasetMetadata(ctx)
	if err != nil || metadata.ReleaseTag == nil || *metadata.ReleaseTag != "db-test" {
		t.Fatalf("metadata = %#v, err = %v", metadata, err)
	}
}

func TestMmapRepositoryWarmsCompactSelectorsAndDataset(t *testing.T) {
	repository := testMmapRepository(t)
	ctx := context.Background()

	selectorBytes, err := repository.CompactSelectorBytes()
	if err != nil || selectorBytes <= 0 {
		t.Fatalf("selector bytes = %d, err = %v", selectorBytes, err)
	}
	preload, err := repository.PreloadCompactSelectors()
	if err != nil || preload.Bytes != selectorBytes {
		t.Fatalf("preload = %#v, selectorBytes = %d, err = %v", preload, selectorBytes, err)
	}

	warmup, err := repository.WarmDataset(ctx)
	if err != nil {
		t.Fatal(err)
	}
	size, err := repository.DatabaseSize()
	if err != nil {
		t.Fatal(err)
	}
	if warmup.Bytes != size || warmup.AlreadyWarm {
		t.Fatalf("warmup = %#v, database bytes = %d", warmup, size)
	}
	repeated, err := repository.WarmDataset(ctx)
	if err != nil {
		t.Fatal(err)
	}
	if repeated.Bytes != size || !repeated.AlreadyWarm {
		t.Fatalf("repeated warmup = %#v, database bytes = %d", repeated, size)
	}
}

func TestMmapRangePaginationUsesRangeIndex(t *testing.T) {
	repository := testMmapRepository(t)
	ctx := context.Background()
	rangeValue, _ := ipkey.ParseRange("1.1.1.0", "1.1.1.255")

	first, err := repository.LookupRange(ctx, rangeValue, RangeAllocations, RangePage{Limit: 1})
	if err != nil {
		t.Fatal(err)
	}
	if len(first.Allocations) != 1 || first.Allocations[0].Name == nil || *first.Allocations[0].Name != "APNIC-1" || first.NextCursor == nil {
		t.Fatalf("first allocation page = %#v", first)
	}
	second, err := repository.LookupRange(ctx, rangeValue, RangeAllocations, RangePage{Limit: 1, Cursor: *first.NextCursor})
	if err != nil {
		t.Fatal(err)
	}
	if len(second.Allocations) != 1 || second.Allocations[0].Name == nil || *second.Allocations[0].Name != "APNIC-LABS" || second.NextCursor != nil {
		t.Fatalf("second allocation page = %#v", second)
	}

	firstRoute, err := repository.LookupRange(ctx, rangeValue, RangeRoutes, RangePage{Limit: 1})
	if err != nil {
		t.Fatal(err)
	}
	if len(firstRoute.Routes) != 1 || firstRoute.Routes[0].OriginASN != "AS13335" || firstRoute.NextCursor == nil {
		t.Fatalf("first route page = %#v", firstRoute)
	}
	secondRoute, err := repository.LookupRange(ctx, rangeValue, RangeRoutes, RangePage{Limit: 1, Cursor: *firstRoute.NextCursor})
	if err != nil {
		t.Fatal(err)
	}
	if len(secondRoute.Routes) != 1 || secondRoute.Routes[0].OriginASN != "AS64500" || secondRoute.NextCursor != nil {
		t.Fatalf("second route page = %#v", secondRoute)
	}

	_, err = repository.LookupRange(ctx, rangeValue, RangeRoutes, RangePage{Limit: 1, Cursor: "not-a-range-cursor"})
	if !errors.Is(err, errInvalidRangeCursor) {
		t.Fatalf("invalid cursor error = %v", err)
	}
	_, err = repository.LookupRange(ctx, rangeValue, RangeRoutes, RangePage{Limit: 1, Cursor: *first.NextCursor})
	if !errors.Is(err, errInvalidRangeCursor) {
		t.Fatalf("cross-kind cursor error = %v", err)
	}
}

func TestMmapBuildMaterializesAllIPv4SummaryPrefixes(t *testing.T) {
	repository := testMmapRepository(t)
	if summaries := repository.db.SummaryCount(); summaries != (1<<17)-1 {
		t.Fatalf("summary records = %d", summaries)
	}
}

func BenchmarkMmapLookupIP(b *testing.B) {
	repository := testMmapRepository(b)
	ip, _ := ipkey.ParseRuntime("1.1.1.1")
	b.ReportAllocs()
	b.ResetTimer()
	for index := 0; index < b.N; index++ {
		if _, err := repository.Lookup(context.Background(), ip, LookupOptions{}); err != nil {
			b.Fatal(err)
		}
	}
}

func BenchmarkMmapLookupIPFull(b *testing.B) {
	repository := testMmapRepository(b)
	ip, _ := ipkey.ParseRuntime("1.1.1.1")
	b.ReportAllocs()
	b.ResetTimer()
	for index := 0; index < b.N; index++ {
		if _, err := repository.Lookup(context.Background(), ip, LookupOptions{Details: LookupDetailsFull}); err != nil {
			b.Fatal(err)
		}
	}
}

type mmapTestingTB interface {
	Helper()
	TempDir() string
	Cleanup(func())
	Fatal(...any)
}

func testMmapRepository(tb mmapTestingTB) *MmapRepository {
	tb.Helper()
	directory := tb.TempDir()
	writeMmapTestCSV(tb, filepath.Join(directory, "allocations.csv"), [][]string{
		{mmapSortKey("1.0.0.0"), mmapSortKey("1.255.255.255"), "4", "APNIC", "AU", "APNIC-1", "ALLOCATED PORTABLE", "2011-08-11", "", "", "APNIC", "", "", "", ""},
		{mmapSortKey("1.1.1.0"), mmapSortKey("1.1.1.255"), "4", "APNIC", "AU", "APNIC-LABS", "ALLOCATED PORTABLE", "2011-08-11", "", "", "APNIC", "", "", "abuse@example.test", ""},
	})
	writeMmapTestCSV(tb, filepath.Join(directory, "routes.csv"), [][]string{
		{mmapSortKey("1.1.1.0"), mmapSortKey("1.1.1.255"), "4", "1.1.1.0/24", "AS13335", "APNIC", "APNIC", "", "", "abuse@example.test", ""},
		{mmapSortKey("1.1.1.0"), mmapSortKey("1.1.1.255"), "4", "1.1.1.0/24", "AS64500", "APNIC", "APNIC", "", "", "", ""},
	})
	writeMmapTestCSV(tb, filepath.Join(directory, "geolocations.csv"), [][]string{
		{mmapSortKey("1.0.0.0"), mmapSortKey("1.255.255.255"), "4", "AU", "New South Wales", "Sydney"},
		{mmapSortKey("1.1.1.0"), mmapSortKey("1.1.1.255"), "4", "AU", "Queensland", "South Brisbane"},
	})
	writeMmapTestCSV(tb, filepath.Join(directory, "autnums.csv"), [][]string{{"AS13335", "APNIC", "US", "CLOUDFLARENET", "ORG-CLOUD14-ARIN", "ASSIGNED", "", "", "APNIC", "", "abuse@example.test", ""}})
	path := filepath.Join(directory, "mehrnet_bgp.mmap")
	stats, err := mmapstore.Build(context.Background(), mmapstore.BuildOptions{InputDir: directory, OutputPath: path, ReleaseTag: "db-test", BuiltAt: "2026-08-26T00:00:00Z", SourceCommit: "abcdef"})
	if err != nil {
		tb.Fatal(err)
	}
	if stats.Allocations != 2 || stats.Routes != 2 || stats.Geofeeds != 2 || stats.Autnums != 1 || stats.RouteLPMNodes == 0 || stats.RouteLPMIDs != stats.Routes || stats.RouteLPMBytes == 0 || stats.AllocationLPMNodes == 0 || stats.AllocationLPMBytes == 0 || stats.GeofeedLPMNodes == 0 || stats.GeofeedLPMBytes == 0 {
		tb.Fatal("unexpected build stats: ", stats)
	}
	repository, err := NewMmapRepository(path)
	if err != nil {
		tb.Fatal(err)
	}
	tb.Cleanup(func() { _ = repository.Close() })
	return repository
}

func writeMmapTestCSV(tb mmapTestingTB, path string, rows [][]string) {
	tb.Helper()
	file, err := os.Create(path)
	if err != nil {
		tb.Fatal(err)
	}
	writer := csv.NewWriter(file)
	for _, row := range rows {
		if err := writer.Write(row); err != nil {
			tb.Fatal(err)
		}
	}
	writer.Flush()
	if err := writer.Error(); err != nil {
		tb.Fatal(err)
	}
	if err := file.Close(); err != nil {
		tb.Fatal(err)
	}
}

func mmapSortKey(value string) string { parsed, _ := ipkey.Parse(value); return parsed.SortKey }
