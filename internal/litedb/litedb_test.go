package litedb

import (
	"bytes"
	"net/netip"
	"testing"
)

func TestRoundTripAndLookup(t *testing.T) {
	b := NewBuilder(BuildOptions{
		Metadata: Metadata{
			ReleaseTag:   "db-test-1",
			BuiltAt:      "2026-11-17T10:00:00Z",
			SourceCommit: "deadbeef",
		},
		IncludeASNRoutes: true,
	})

	b.AddAutnum(AutnumInput{
		ASNumber:     13335,
		Registry:     "ARIN",
		Country:      "us",
		Name:         "CLOUDFLARENET",
		Organization: "ORG-CLOUD14-ARIN",
		Status:       "ASSIGNED",
		Source:       "ARIN",
	})
	b.AddAutnum(AutnumInput{
		ASNumber:     64512,
		Registry:     "RIPE",
		Country:      "DE",
		Name:         "PRIV-NET",
		Organization: "ORG-PRIV",
	})

	// Covering allocation + narrower sub-allocation + non-CIDR sub-/24 + geofeed override.
	b.AddAllocation(netip.MustParseAddr("1.0.0.0"), netip.MustParseAddr("1.255.255.255"), "APNIC", "AU")
	b.AddAllocation(netip.MustParseAddr("1.1.2.0"), netip.MustParseAddr("1.1.2.255"), "APNIC", "SG")
	b.AddAllocation(netip.MustParseAddr("1.1.2.10"), netip.MustParseAddr("1.1.2.25"), "APNIC", "JP")
	b.AddGeofeed(netip.MustParseAddr("1.1.1.0"), netip.MustParseAddr("1.1.1.255"), "US")

	// IPv6 allocation + /64 sub-allocation + geofeed, plus a ::/0 covering placeholder (must not truncate narrower blocks).
	b.AddAllocation(netip.MustParseAddr("::"), netip.MustParseAddr("ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff"), "RIPE", "")
	b.AddAllocation(netip.MustParseAddr("2606:4700::"), netip.MustParseAddr("2606:4700:ffff:ffff:ffff:ffff:ffff:ffff"), "ARIN", "US")
	b.AddAllocation(netip.MustParseAddr("2606:4700:4700:1::"), netip.MustParseAddr("2606:4700:4700:1:ffff:ffff:ffff:ffff"), "ARIN", "CA")
	b.AddGeofeed(netip.MustParseAddr("::"), netip.MustParseAddr("ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff"), "DE")
	b.AddGeofeed(netip.MustParseAddr("2606:4700:4700::"), netip.MustParseAddr("2606:4700:4700:ffff:ffff:ffff:ffff:ffff"), "GB")

	// Overlapping routes (LPM) + multi-origin route + IPv6 route.
	b.AddRoute(netip.MustParsePrefix("1.1.0.0/16"), 64512)
	b.AddRoute(netip.MustParsePrefix("1.1.1.0/24"), 13335)
	b.AddRoute(netip.MustParsePrefix("1.1.2.0/24"), 13335)
	b.AddRoute(netip.MustParsePrefix("1.1.2.0/24"), 64512) // multi-origin!
	b.AddRoute(netip.MustParsePrefix("2606:4700::/32"), 13335)
	b.AddRoute(netip.MustParsePrefix("2606:4700:4700::/48"), 64512)

	// Sibling /24s with the same ASN must NOT merge into the first /24.
	b.AddRoute(netip.MustParsePrefix("2.0.0.0/24"), 13335)
	b.AddRoute(netip.MustParsePrefix("2.0.1.0/24"), 13335)

	var buf bytes.Buffer
	stats, err := b.Encode(&buf)
	if err != nil {
		t.Fatal(err)
	}
	if stats.Autnums != 2 || stats.MultiASNSets != 1 || stats.ASNRouteItems != 8 {
		t.Fatalf("unexpected stats: %+v", stats)
	}

	db, err := Open(buf.Bytes())
	if err != nil {
		t.Fatal(err)
	}
	if db.Metadata().ReleaseTag != "db-test-1" {
		t.Fatalf("bad metadata: %+v", db.Metadata())
	}

	// 1.1.1.1 -> LPM 1.1.1.0/24 (AS13335), alloc AU (apnic), geo US
	r1 := db.LookupIP(netip.MustParseAddr("1.1.1.1"))
	if !r1.Found || !r1.HasAlloc || !r1.HasGeo || !r1.HasRoute {
		t.Fatalf("r1 flags wrong: %+v", r1)
	}
	if r1.Registry != "apnic" || r1.AllocCC != "AU" || r1.GeoCC != "US" {
		t.Fatalf("r1 alloc/geo wrong: %+v", r1)
	}
	if r1.RoutePrefix.String() != "1.1.1.0/24" || len(r1.RouteASNs) != 1 || r1.RouteASNs[0].Name != "CLOUDFLARENET" {
		t.Fatalf("r1 route wrong: %+v", r1)
	}

	// 1.1.3.5 -> falls back to 1.1.0.0/16 (AS64512), no geo -> alloc AU
	r2 := db.LookupIP(netip.MustParseAddr("1.1.3.5"))
	if !r2.HasRoute || r2.RoutePrefix.String() != "1.1.0.0/16" || r2.RouteASNs[0].ASNumber != 64512 || r2.HasGeo {
		t.Fatalf("r2 wrong: %+v", r2)
	}

	// 1.1.2.9 -> multi-origin 1.1.2.0/24 (AS13335, AS64512), alloc SG
	r3 := db.LookupIP(netip.MustParseAddr("1.1.2.9"))
	if !r3.HasRoute || r3.RoutePrefix.String() != "1.1.2.0/24" || len(r3.RouteASNs) != 2 ||
		r3.RouteASNs[0].ASNumber != 13335 || r3.RouteASNs[1].ASNumber != 64512 || r3.AllocCC != "SG" {
		t.Fatalf("r3 wrong: %+v", r3)
	}

	// 1.1.2.15 -> non-CIDR sub-/24 allocation JP
	if rSub := db.LookupIP(netip.MustParseAddr("1.1.2.15")); rSub.AllocCC != "JP" || rSub.AllocStart.String() != "1.1.2.10" || rSub.AllocEnd.String() != "1.1.2.25" {
		t.Fatalf("sub-/24 non-CIDR alloc wrong: %+v", rSub)
	}

	// IPv6 2606:4700:4700::1 -> /48 (AS64512), geo GB, alloc US (arin)
	r6 := db.LookupIP(netip.MustParseAddr("2606:4700:4700::1"))
	if !r6.HasRoute || r6.RoutePrefix.String() != "2606:4700:4700::/48" || r6.RouteASNs[0].ASNumber != 64512 ||
		r6.GeoCC != "GB" || r6.AllocCC != "US" || r6.Registry != "arin" {
		t.Fatalf("r6 wrong: %+v", r6)
	}

	// IPv6 /64 sub-allocation
	if r6Sub := db.LookupIP(netip.MustParseAddr("2606:4700:4700:1::42")); r6Sub.AllocCC != "CA" {
		t.Fatalf("v6 /64 sub-alloc wrong: %+v", r6Sub)
	}

	// Sibling /24 lookup must reconstruct 2.0.1.0/24, not 2.0.0.0/24.
	if sib := db.LookupIP(netip.MustParseAddr("2.0.1.42")); !sib.HasRoute || sib.RoutePrefix.String() != "2.0.1.0/24" {
		t.Fatalf("sibling route wrong: %+v", sib)
	}

	// LookupASN pagination
	a1 := db.LookupASN(13335, 1, 2)
	if !a1.Found || !a1.HasAutnum || a1.Autnum.Name != "CLOUDFLARENET" || a1.TotalRoute != 5 || len(a1.Routes) != 2 {
		t.Fatalf("LookupASN wrong: %+v", a1)
	}

	// Truncation safety
	for cut := 0; cut < buf.Len(); cut += 17 {
		if d, err := Open(buf.Bytes()[:cut]); err == nil {
			d.LookupIP(netip.MustParseAddr("1.1.1.1"))
			d.LookupASN(13335, 0, 10)
		}
	}
}
