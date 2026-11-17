package main

import (
	"context"
	"flag"
	"fmt"
	"log"

	"github.com/mehrnet/bgp-api/internal/mmapstore"
)

func main() {
	input := flag.String("input", "", "producer input directory")
	output := flag.String("output", "", "output mmap database path")
	release := flag.String("release-tag", "", "dataset release tag")
	builtAt := flag.String("built-at", "", "dataset build timestamp")
	commit := flag.String("source-commit", "", "producer source commit")
	flag.Parse()

	stats, err := mmapstore.Build(context.Background(), mmapstore.BuildOptions{
		InputDir: *input, OutputPath: *output, ReleaseTag: *release,
		BuiltAt: *builtAt, SourceCommit: *commit,
	})
	if err != nil {
		log.Fatal(err)
	}
	fmt.Printf("built %s: file_bytes=%d allocations=%d routes=%d geofeeds=%d autnums=%d allocation_index=%d route_index=%d geofeed_index=%d allocation_range_index=%d route_range_index=%d route_lpm_nodes=%d route_lpm_ids=%d route_lpm_bytes=%d allocation_lpm_nodes=%d allocation_lpm_bytes=%d geofeed_lpm_nodes=%d geofeed_lpm_bytes=%d string_pool_entries=%d string_pool_bytes=%d materialized_summaries=%d\n",
		*output, stats.FileBytes, stats.Allocations, stats.Routes, stats.Geofeeds, stats.Autnums,
		stats.AllocationIndex, stats.RouteIndex, stats.GeofeedIndex,
		stats.AllocationRangeIndex, stats.RouteRangeIndex, stats.RouteLPMNodes,
		stats.RouteLPMIDs, stats.RouteLPMBytes, stats.AllocationLPMNodes,
		stats.AllocationLPMBytes, stats.GeofeedLPMNodes, stats.GeofeedLPMBytes,
		stats.StringPoolEntries, stats.StringPoolBytes, stats.MaterializedSummary)
}
