package mmapstore

import (
	"context"
	"net/netip"
	"os"
	"path/filepath"
	"reflect"
	"testing"
)

func TestSelectionLPMChoosesCompactResponseRecord(t *testing.T) {
	builder := newSelectionLPMBuilder(4)
	insertSelectionRange(t, builder, 1, "0.0.0.0", "255.255.255.255")
	insertSelectionRange(t, builder, 2, "1.0.0.0", "1.255.255.255")
	insertSelectionRange(t, builder, 3, "1.1.0.0", "1.1.255.255")

	// Two equal-width overlapping records use the component with greater prefix length.
	insertSelectionRange(t, builder, 100, "4.0.0.1", "4.0.0.2")
	insertSelectionRange(t, builder, 1, "4.0.0.0", "4.0.0.1")

	// Records sharing a component prefer narrower source range, then lower ID.
	insertSelectionRange(t, builder, 20, "3.0.0.0", "3.255.255.255")
	insertSelectionRange(t, builder, 5, "3.0.0.0", "3.255.255.255")
	insertSelectionRange(t, builder, 1, "3.0.0.0", "4.255.255.255")

	encoded, stats, err := builder.Encode()
	if err != nil {
		t.Fatal(err)
	}
	if stats.Nodes == 0 || stats.Bytes != uint64(len(encoded)) {
		t.Fatalf("unexpected selection LPM stats: %#v", stats)
	}
	for _, tc := range []struct {
		address string
		want    uint32
	}{
		{"1.1.1.1", 3},
		{"1.2.1.1", 2},
		{"2.2.2.2", 1},
		{"3.1.1.1", 5},
		{"4.0.0.1", 100},
	} {
		got, err := LookupSelectionLPM(encoded, netip.MustParseAddr(tc.address))
		if err != nil {
			t.Fatal(err)
		}
		if got != tc.want {
			t.Fatalf("lookup %s = %d, want %d", tc.address, got, tc.want)
		}
	}
}

func TestSelectionLPMSupportsIPv6(t *testing.T) {
	builder := newSelectionLPMBuilder(6)
	insertSelectionRange(t, builder, 1, "::", "ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff")
	insertSelectionRange(t, builder, 2, "2001:db8::", "2001:db8:ffff:ffff:ffff:ffff:ffff:ffff")
	encoded, _, err := builder.Encode()
	if err != nil {
		t.Fatal(err)
	}
	got, err := LookupSelectionLPM(encoded, netip.MustParseAddr("2001:db8:1234::1"))
	if err != nil {
		t.Fatal(err)
	}
	if got != 2 {
		t.Fatalf("lookup = %d, want 2", got)
	}
}

func TestSelectionLPMRejectsInvalidInputs(t *testing.T) {
	builder := newSelectionLPMBuilder(4)
	if err := builder.Insert(netip.MustParsePrefix("2001:db8::/32"), 1, Address{}, Address{}); err == nil {
		t.Fatal("accepted IPv6 prefix in IPv4 selector")
	}
	if err := builder.Insert(netip.MustParsePrefix("1.1.1.0/24"), 0, Address{}, Address{}); err == nil {
		t.Fatal("accepted zero selector ID")
	}
	if _, err := LookupSelectionLPM(nil, netip.MustParseAddr("1.1.1.1")); err == nil {
		t.Fatal("accepted empty selector payload")
	}
}

func TestRouteLPMResolvesMostSpecificIPv4AndIPv6Prefixes(t *testing.T) {
	for _, tc := range []struct {
		name    string
		version uint8
		entries []struct {
			prefix string
			ids    []uint32
		}
		queries []struct {
			address string
			ids     []uint32
		}
	}{
		{
			name:    "IPv4",
			version: 4,
			entries: []struct {
				prefix string
				ids    []uint32
			}{
				{"0.0.0.0/0", []uint32{1}},
				{"1.0.0.0/8", []uint32{2}},
				{"1.1.0.0/16", []uint32{3}},
				{"1.1.1.0/24", []uint32{4, 5}},
			},
			queries: []struct {
				address string
				ids     []uint32
			}{
				{"1.1.1.9", []uint32{4, 5}},
				{"1.1.2.9", []uint32{3}},
				{"1.2.2.9", []uint32{2}},
				{"2.2.2.2", []uint32{1}},
			},
		},
		{
			name:    "IPv6",
			version: 6,
			entries: []struct {
				prefix string
				ids    []uint32
			}{
				{"::/0", []uint32{10}},
				{"2001:db8::/32", []uint32{11}},
				{"2001:db8:1234::/48", []uint32{12}},
			},
			queries: []struct {
				address string
				ids     []uint32
			}{
				{"2001:db8:1234::1", []uint32{12}},
				{"2001:db8:9876::1", []uint32{11}},
				{"2001:4860::1", []uint32{10}},
			},
		},
	} {
		t.Run(tc.name, func(t *testing.T) {
			builder := newRouteLPMBuilder(tc.version)
			for _, entry := range tc.entries {
				if err := builder.Insert(netip.MustParsePrefix(entry.prefix), entry.ids); err != nil {
					t.Fatal(err)
				}
			}
			encoded, stats, err := builder.Encode()
			if err != nil {
				t.Fatal(err)
			}
			if stats.Nodes < uint64(len(tc.entries)) || stats.IDs == 0 || stats.Bytes != uint64(len(encoded)) {
				t.Fatalf("unexpected route LPM stats: %#v", stats)
			}
			for _, query := range tc.queries {
				ids, err := LookupRouteLPM(encoded, netip.MustParseAddr(query.address))
				if err != nil {
					t.Fatal(err)
				}
				if !reflect.DeepEqual(ids, query.ids) {
					t.Fatalf("lookup %s = %v, want %v", query.address, ids, query.ids)
				}
			}
		})
	}
}

func TestRouteLPMRejectsInvalidInputs(t *testing.T) {
	builder := newRouteLPMBuilder(4)
	if err := builder.Insert(netip.MustParsePrefix("2001:db8::/32"), []uint32{1}); err == nil {
		t.Fatal("accepted IPv6 prefix in IPv4 LPM")
	}
	if err := builder.Insert(netip.MustParsePrefix("1.1.1.0/24"), nil); err == nil {
		t.Fatal("accepted empty route ID list")
	}
	if _, err := LookupRouteLPM(nil, netip.MustParseAddr("1.1.1.1")); err == nil {
		t.Fatal("accepted empty route LPM payload")
	}
}

func TestBuildAndOpenRoundTrip(t *testing.T) {
	dir := t.TempDir()
	writeCSV := func(name, content string) {
		t.Helper()
		if err := os.WriteFile(filepath.Join(dir, name), []byte(content), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	writeCSV("allocations.csv",
		"16777216,16842751,4,apnic,AU,APNIC-BROAD,ALLOCATED PORTABLE,20100101,2010-01-01T00:00:00Z,2024-01-01T00:00:00Z,apnic,APNIC-HM,APNIC Org,abuse@apnic.example,Broad block\n"+
			"42540766411282592856903984951653826560,42540766411282592856903984951653892095,6,arin,US,V6-NET,ALLOCATED,20200202,,,,ARIN,Example V6,abuse@v6.example,V6 block\n",
	)
	writeCSV("routes.csv",
		"16843008,16843263,4,1.1.1.0/24,AS13335,apnic,APNIC,MNT-CF,Cloudflare,abuse@cf.example,CF DNS\n"+
			"16843008,16843263,4,1.1.1.0/24,AS64500,apnic,APNIC,MNT-EX,Example,abuse@ex.example,Alt origin\n",
	)
	writeCSV("geolocations.csv", "16843008,16843263,4,AU,QLD,Brisbane\n")
	writeCSV("autnums.csv", "AS13335,arin,US,CLOUDFLARENET,Cloudflare Inc.,ASSIGNED,2010-07-14T00:00:00Z,2024-02-01T00:00:00Z,arin,MNT-CF,abuse@cf.example,Cloudflare ASN\n")

	out := filepath.Join(dir, "test.mmap")
	stats, err := Build(context.Background(), BuildOptions{
		InputDir: dir, OutputPath: out,
		ReleaseTag: "db-test", BuiltAt: "2026-11-17T00:00:00Z", SourceCommit: "abc123",
	})
	if err != nil {
		t.Fatal(err)
	}
	if stats.Allocations != 2 || stats.Routes != 2 || stats.Geofeeds != 1 || stats.Autnums != 1 {
		t.Fatalf("unexpected build stats: %#v", stats)
	}
	if stats.MaterializedSummary != (1<<17)-1 {
		t.Fatalf("summary count = %d", stats.MaterializedSummary)
	}

	db, err := Open(out)
	if err != nil {
		t.Fatal(err)
	}
	defer db.Close()

	if db.Metadata().ReleaseTag != "db-test" {
		t.Fatalf("unexpected metadata: %#v", db.Metadata())
	}
	if _, err := db.WarmCompactSelectors(); err != nil {
		t.Fatal(err)
	}
	if warmed, err := db.WarmDataset(context.Background()); err != nil || warmed != db.Size() {
		t.Fatalf("warmed=%d size=%d err=%v", warmed, db.Size(), err)
	}

	allocID, err := db.LookupAllocationLPM(netip.MustParseAddr("1.0.5.1"))
	if err != nil || allocID != 1 {
		t.Fatalf("v4 alloc LPM = %d, err=%v", allocID, err)
	}
	alloc6ID, err := db.LookupAllocationLPM(netip.MustParseAddr("2001:db8::42"))
	if err != nil || alloc6ID != 2 {
		t.Fatalf("v6 alloc LPM = %d, err=%v", alloc6ID, err)
	}
	routeIDs, err := db.LookupRouteLPM(netip.MustParseAddr("1.1.1.1"))
	if err != nil || !reflect.DeepEqual(routeIDs, []uint32{1, 2}) {
		t.Fatalf("route LPM = %v, err=%v", routeIDs, err)
	}
	geoID, err := db.LookupGeofeedLPM(netip.MustParseAddr("1.1.1.1"))
	if err != nil || geoID != 1 {
		t.Fatalf("geo LPM = %d, err=%v", geoID, err)
	}
	geo, err := db.Geofeed(geoID)
	if err != nil || geo.City != "Brisbane" {
		t.Fatalf("geofeed = %#v, err=%v", geo, err)
	}
	autnum, ok, err := db.Autnum(13335)
	if err != nil || !ok || autnum.Name != "CLOUDFLARENET" || autnum.AbuseContact != "abuse@cf.example" {
		t.Fatalf("autnum = %#v, ok=%t, err=%v", autnum, ok, err)
	}
	total, asnIDs, err := db.ASNRouteSlice(13335, false, 0, 0, 10)
	if err != nil || total != 1 || !reflect.DeepEqual(asnIDs, []uint32{1}) {
		t.Fatalf("asn routes total=%d ids=%v err=%v", total, asnIDs, err)
	}
	sum, ok, err := db.Summary(netip.MustParsePrefix("1.1.0.0/16"))
	if err != nil || !ok || sum.Routes != 2 {
		t.Fatalf("summary = %#v, ok=%t, err=%v", sum, ok, err)
	}

	// Corrupt/truncated file check
	truncated := filepath.Join(dir, "truncated.mmap")
	raw, _ := os.ReadFile(out)
	if err := os.WriteFile(truncated, raw[:len(raw)-128], 0o600); err != nil {
		t.Fatal(err)
	}
	if _, err := Open(truncated); err == nil {
		t.Fatal("accepted truncated mmap database")
	}
}

func insertSelectionRange(t *testing.T, builder *selectionLPMBuilder, id uint32, startValue, endValue string) {
	t.Helper()
	start, end := AddressFromAddr(netip.MustParseAddr(startValue)), AddressFromAddr(netip.MustParseAddr(endValue))
	prefixes, err := rangePrefixes(start, end, int(builder.version))
	if err != nil {
		t.Fatal(err)
	}
	for _, prefix := range prefixes {
		if err := builder.Insert(prefix, id, start, end); err != nil {
			t.Fatal(err)
		}
	}
}
