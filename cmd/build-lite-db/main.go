// build-lite-db produces bgp-lite.mnl from the producer CSV directory for
// bundling into the standalone bgp-lite Cloudflare Worker.
package main

import (
	"bufio"
	"encoding/csv"
	"flag"
	"fmt"
	"io"
	"log"
	"math/big"
	"net/netip"
	"os"
	"path/filepath"
	"strconv"
	"strings"

	"github.com/mehrnet/bgp-api/internal/litedb"
)

func addrFromSortKey(value, version string) (netip.Addr, bool) {
	number, ok := new(big.Int).SetString(strings.TrimSpace(value), 10)
	if !ok || number.Sign() < 0 || number.BitLen() > 128 {
		return netip.Addr{}, false
	}
	var raw [16]byte
	number.FillBytes(raw[:])
	switch strings.TrimSpace(version) {
	case "4":
		return netip.AddrFrom4([4]byte(raw[12:])), true
	case "6":
		return netip.AddrFrom16(raw), true
	}
	return netip.Addr{}, false
}

func parseASN(input string) uint32 {
	value := strings.TrimPrefix(strings.ToUpper(strings.TrimSpace(input)), "AS")
	n, err := strconv.ParseUint(value, 10, 32)
	if err != nil {
		return 0
	}
	return uint32(n)
}

func at(row []string, idx int) string {
	if idx < len(row) {
		return row[idx]
	}
	return ""
}

func readCSV(path string, fn func([]string)) error {
	file, err := os.Open(path)
	if err != nil {
		return err
	}
	defer file.Close()
	reader := csv.NewReader(bufio.NewReaderSize(file, 1<<20))
	reader.FieldsPerRecord = -1
	reader.ReuseRecord = true
	for {
		row, err := reader.Read()
		if err == io.EOF {
			return nil
		}
		if err != nil {
			return fmt.Errorf("read %s: %w", path, err)
		}
		fn(row)
	}
}

func main() {
	input := flag.String("input", "final_data", "directory with allocations.csv, routes.csv, autnums.csv, geolocations.csv")
	output := flag.String("output", "bgp-lite.mnl", "output MNL1 file path")
	releaseTag := flag.String("release-tag", "", "dataset release tag")
	builtAt := flag.String("built-at", "", "dataset build timestamp")
	commit := flag.String("source-commit", "", "producer source commit")
	includeASNRoutes := flag.Bool("include-asn-routes", true, "include per-ASN deduplicated route prefix lists")
	allocV4Bits := flag.Int("alloc-v4-prefix", litedb.DefaultAllocV4Bits, "allocation/geofeed IPv4 granularity as a prefix length (1..32)")
	allocV6Bits := flag.Int("alloc-v6-prefix", litedb.DefaultAllocV6Bits, "allocation/geofeed IPv6 granularity as a prefix length (1..64)")
	flag.Parse()

	if *allocV4Bits < 1 || *allocV4Bits > 32 {
		log.Fatalf("invalid -alloc-v4-prefix %d (must be 1..32)", *allocV4Bits)
	}
	if *allocV6Bits < 1 || *allocV6Bits > 64 {
		log.Fatalf("invalid -alloc-v6-prefix %d (must be 1..64)", *allocV6Bits)
	}

	builder := litedb.NewBuilder(litedb.BuildOptions{
		Metadata: litedb.Metadata{
			ReleaseTag:   *releaseTag,
			BuiltAt:      *builtAt,
			SourceCommit: *commit,
		},
		IncludeASNRoutes: *includeASNRoutes,
		AllocV4Bits:      *allocV4Bits,
		AllocV6Bits:      *allocV6Bits,
	})

	var autnumCount, allocCount, geoCount, routeCount int
	if err := readCSV(filepath.Join(*input, "autnums.csv"), func(row []string) {
		if len(row) < 12 {
			return
		}
		asn := parseASN(row[0])
		if asn == 0 {
			return
		}
		if builder.AddAutnum(litedb.AutnumInput{
			ASNumber:     asn,
			Registry:     at(row, 1),
			Country:      at(row, 2),
			Name:         at(row, 3),
			Organization: at(row, 4),
			Status:       at(row, 5),
			Source:       at(row, 8),
			Maintainers:  at(row, 9),
			AbuseContact: at(row, 10),
			Description:  at(row, 11),
		}) {
			autnumCount++
		}
	}); err != nil {
		log.Fatal(err)
	}

	if err := readCSV(filepath.Join(*input, "allocations.csv"), func(row []string) {
		if len(row) < 6 {
			return
		}
		start, ok1 := addrFromSortKey(row[0], row[2])
		end, ok2 := addrFromSortKey(row[1], row[2])
		if ok1 && ok2 && builder.AddAllocation(start, end, at(row, 3), at(row, 4)) {
			allocCount++
		}
	}); err != nil {
		log.Fatal(err)
	}

	if err := readCSV(filepath.Join(*input, "geolocations.csv"), func(row []string) {
		if len(row) < 6 {
			return
		}
		start, ok1 := addrFromSortKey(row[0], row[2])
		end, ok2 := addrFromSortKey(row[1], row[2])
		if ok1 && ok2 && builder.AddGeofeed(start, end, at(row, 3), at(row, 4), at(row, 5)) {
			geoCount++
		}
	}); err != nil {
		log.Fatal(err)
	}

	if err := readCSV(filepath.Join(*input, "routes.csv"), func(row []string) {
		if len(row) < 5 {
			return
		}
		prefix, err := netip.ParsePrefix(strings.TrimSpace(row[3]))
		if err != nil || prefix.Addr().Zone() != "" {
			return
		}
		asn := parseASN(row[4])
		if asn != 0 && builder.AddRoute(prefix, asn) {
			routeCount++
		}
	}); err != nil {
		log.Fatal(err)
	}

	tmp := *output + ".tmp"
	file, err := os.Create(tmp)
	if err != nil {
		log.Fatal(err)
	}
	stats, err := builder.Encode(file)
	if err == nil {
		err = file.Close()
	} else {
		file.Close()
	}
	if err != nil {
		os.Remove(tmp)
		log.Fatal(err)
	}
	if err := os.Rename(tmp, *output); err != nil {
		log.Fatal(err)
	}
	fmt.Printf("loaded autnums=%d allocations=%d geofeeds=%d routes=%d\n", autnumCount, allocCount, geoCount, routeCount)
	fmt.Printf("wrote %s: bytes=%d (%.2f MiB) countries=%d registries=%d regions=%d cities=%d geolocs=%d alloc=(%d,%d) geo=(%d,%d) route=(%d,%d) multi_sets=%d asn_routes=%d\n",
		*output, stats.Bytes, float64(stats.Bytes)/(1024*1024),
		stats.Countries, stats.Registries, stats.Regions, stats.Cities, stats.GeoLocations,
		stats.AllocV4Segs, stats.AllocV6Segs,
		stats.GeoV4Segs, stats.GeoV6Segs,
		stats.RouteV4Segs, stats.RouteV6Segs,
		stats.MultiASNSets, stats.ASNRouteItems)
}
