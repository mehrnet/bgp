package mmapstore

import (
	"bytes"
	"encoding/binary"
	"errors"
	"math/bits"
	"net/netip"
)

const (
	selectionLPMHeaderSize = 16
	routeLPMHeaderSize     = 16
	routeLPMNodeSize       = 33
)

var (
	selectionLPMMagic = [4]byte{'S', 'L', 'P', '2'}
	routeLPMMagic     = [4]byte{'M', 'L', 'P', '2'}
)

// SelectionLPMStats describes one immutable allocation or geofeed selector trie.
type SelectionLPMStats struct {
	Nodes uint64
	Bytes uint64
}

// RouteLPMStats describes one immutable route longest-prefix trie.
type RouteLPMStats struct {
	Nodes uint64
	IDs   uint64
	Bytes uint64
}

type selectionCandidate struct {
	id    uint32
	width Address
}

type selectionLPMNode struct {
	prefix       Address
	prefixLength uint8
	children     [2]uint32 // one-based node indexes; zero means absent
	candidate    selectionCandidate
}

type selectionLPMBuilder struct {
	version uint8
	nodes   []selectionLPMNode
}

func newSelectionLPMBuilder(version uint8) *selectionLPMBuilder {
	return &selectionLPMBuilder{
		version: version,
		nodes:   []selectionLPMNode{{prefix: Address{}, prefixLength: 0}},
	}
}

func (builder *selectionLPMBuilder) Insert(prefix netip.Prefix, id uint32, start, end Address) error {
	prefix = prefix.Masked()
	if builder.version == 4 && !prefix.Addr().Is4() {
		return errors.New("IPv6 prefix added to IPv4 selection LPM")
	}
	if builder.version == 6 && (!prefix.Addr().Is6() || prefix.Addr().Zone() != "") {
		return errors.New("IPv4 or scoped prefix added to IPv6 selection LPM")
	}
	if id == 0 {
		return errors.New("selection LPM record has no ID")
	}
	candidate := selectionCandidate{id: id, width: selectionRangeWidth(start, end)}
	address := AddressFromAddr(prefix.Addr())
	root, err := builder.insert(1, address, uint8(prefix.Bits()), candidate)
	if err != nil {
		return err
	}
	if root != 1 {
		return errors.New("selection LPM root was replaced")
	}
	return nil
}

func (builder *selectionLPMBuilder) insert(reference uint32, prefix Address, prefixLength uint8, candidate selectionCandidate) (uint32, error) {
	if reference == 0 {
		return builder.appendNode(selectionLPMNode{
			prefix:       maskedLPMAddress(prefix, builder.version, int(prefixLength)),
			prefixLength: prefixLength,
			candidate:    candidate,
		})
	}
	if int(reference) > len(builder.nodes) {
		return 0, errors.New("selection LPM references a missing node")
	}
	index := reference - 1
	node := builder.nodes[index]
	common := lpmCommonPrefix(node.prefix, node.prefixLength, prefix, prefixLength, builder.version)
	if common == node.prefixLength {
		if common == prefixLength {
			if node.candidate.id == 0 || selectionCandidateLess(candidate, node.candidate) {
				builder.nodes[index].candidate = candidate
			}
			return reference, nil
		}
		branch := lpmBit(prefix, builder.version, int(common))
		child, err := builder.insert(node.children[branch], prefix, prefixLength, candidate)
		if err != nil {
			return 0, err
		}
		builder.nodes[index].children[branch] = child
		return reference, nil
	}

	parent := selectionLPMNode{
		prefix:       maskedLPMAddress(prefix, builder.version, int(common)),
		prefixLength: common,
	}
	parentReference, err := builder.appendNode(parent)
	if err != nil {
		return 0, err
	}
	existingBranch := lpmBit(node.prefix, builder.version, int(common))
	builder.nodes[parentReference-1].children[existingBranch] = reference
	if common == prefixLength {
		builder.nodes[parentReference-1].candidate = candidate
		return parentReference, nil
	}
	newBranch := lpmBit(prefix, builder.version, int(common))
	child, err := builder.insert(0, prefix, prefixLength, candidate)
	if err != nil {
		return 0, err
	}
	builder.nodes[parentReference-1].children[newBranch] = child
	return parentReference, nil
}

func (builder *selectionLPMBuilder) appendNode(node selectionLPMNode) (uint32, error) {
	if uint64(len(builder.nodes)) >= uint64(^uint32(0)) {
		return 0, errors.New("selection LPM has too many nodes")
	}
	builder.nodes = append(builder.nodes, node)
	return uint32(len(builder.nodes)), nil
}

func (builder *selectionLPMBuilder) Encode() ([]byte, SelectionLPMStats, error) {
	nodeSize, addressBytes, err := selectionLPMNodeLayout(builder.version)
	if err != nil {
		return nil, SelectionLPMStats{}, err
	}
	totalBytes := uint64(selectionLPMHeaderSize) + uint64(len(builder.nodes))*uint64(nodeSize)
	if totalBytes > uint64(int(^uint(0)>>1)) {
		return nil, SelectionLPMStats{}, errors.New("selection LPM exceeds addressable memory")
	}
	encoded := make([]byte, int(totalBytes))
	copy(encoded[:4], selectionLPMMagic[:])
	encoded[4] = builder.version
	binary.BigEndian.PutUint32(encoded[8:12], uint32(len(builder.nodes)))

	for index, node := range builder.nodes {
		offset := selectionLPMHeaderSize + index*nodeSize
		copy(encoded[offset:offset+addressBytes], selectionAddressBytes(node.prefix, builder.version))
		offset += addressBytes
		encoded[offset] = node.prefixLength
		offset++
		binary.BigEndian.PutUint32(encoded[offset:offset+4], node.children[0])
		binary.BigEndian.PutUint32(encoded[offset+4:offset+8], node.children[1])
		offset += 8
		binary.BigEndian.PutUint32(encoded[offset:offset+4], node.candidate.id)
		offset += 4
		copy(encoded[offset:offset+addressBytes], selectionAddressBytes(node.candidate.width, builder.version))
	}
	return encoded, SelectionLPMStats{Nodes: uint64(len(builder.nodes)), Bytes: totalBytes}, nil
}

func validateSelectionLPM(value []byte, version uint8) error {
	nodeSize, _, err := selectionLPMNodeLayout(version)
	if err != nil {
		return err
	}
	if len(value) < selectionLPMHeaderSize ||
		value[0] != selectionLPMMagic[0] ||
		value[1] != selectionLPMMagic[1] ||
		value[2] != selectionLPMMagic[2] ||
		value[3] != selectionLPMMagic[3] ||
		value[4] != version {
		return errors.New("invalid selection LPM header")
	}
	nodeCount := binary.BigEndian.Uint32(value[8:12])
	if nodeCount == 0 {
		return errors.New("selection LPM has no root node")
	}
	if uint64(selectionLPMHeaderSize)+uint64(nodeCount)*uint64(nodeSize) != uint64(len(value)) {
		return errors.New("invalid selection LPM length")
	}
	return nil
}

// LookupSelectionLPM resolves the compact-selected allocation or geofeed ID
// from an immutable mmap-backed trie slice.
func LookupSelectionLPM(value []byte, address netip.Addr) (uint32, error) {
	version := uint8(6)
	if address.Is4() {
		version = 4
	}
	if err := validateSelectionLPM(value, version); err != nil {
		return 0, err
	}
	nodeSize, addressBytes, _ := selectionLPMNodeLayout(version)
	nodeCount := binary.BigEndian.Uint32(value[8:12])
	query := AddressFromAddr(address)
	bitsCount := 128
	if version == 4 {
		bitsCount = 32
	}
	current := uint32(0)
	var bestID uint32
	var bestWidth Address
	var bestPrefixLength uint8
	for {
		if current >= nodeCount {
			return 0, errors.New("selection LPM child references a missing node")
		}
		offset := selectionLPMHeaderSize + int(current)*nodeSize
		prefixLength := value[offset+addressBytes]
		if int(prefixLength) > bitsCount || !selectionAddressMatches(query, value[offset:offset+addressBytes], version, int(prefixLength)) {
			break
		}
		candidateOffset := offset + addressBytes + 1 + 8
		candidateID := binary.BigEndian.Uint32(value[candidateOffset : candidateOffset+4])
		if candidateID != 0 {
			widthOffset := candidateOffset + 4
			if bestID == 0 || selectionEncodedCandidateLess(value[widthOffset:widthOffset+addressBytes], candidateID, prefixLength, bestWidth, bestID, bestPrefixLength, version) {
				bestID, bestPrefixLength = candidateID, prefixLength
				selectionCopyAddressBytes(bestWidth[:], value[widthOffset:widthOffset+addressBytes], version)
			}
		}
		if int(prefixLength) >= bitsCount {
			break
		}
		branch := lpmBit(query, version, int(prefixLength))
		childrenOffset := offset + addressBytes + 1
		next := binary.BigEndian.Uint32(value[childrenOffset+int(branch)*4 : childrenOffset+int(branch)*4+4])
		if next == 0 {
			break
		}
		current = next - 1
	}
	return bestID, nil
}

func selectionLPMNodeLayout(version uint8) (nodeSize, addressBytes int, err error) {
	switch version {
	case 4:
		return 21, 4, nil
	case 6:
		return 45, 16, nil
	default:
		return 0, 0, errors.New("invalid selection LPM version")
	}
}

func selectionCandidateLess(left, right selectionCandidate) bool {
	if compared := bytes.Compare(left.width[:], right.width[:]); compared != 0 {
		return compared < 0
	}
	return left.id < right.id
}

func selectionEncodedCandidateLess(width []byte, id uint32, prefixLength uint8, bestWidth Address, bestID uint32, bestPrefixLength uint8, version uint8) bool {
	best := selectionAddressBytes(bestWidth, version)
	if compared := bytes.Compare(width, best); compared != 0 {
		return compared < 0
	}
	if prefixLength != bestPrefixLength {
		return prefixLength > bestPrefixLength
	}
	return id < bestID
}

func selectionRangeWidth(start, end Address) Address {
	var width Address
	borrow := 0
	for index := len(width) - 1; index >= 0; index-- {
		value := int(end[index]) - int(start[index]) - borrow
		if value < 0 {
			value += 256
			borrow = 1
		} else {
			borrow = 0
		}
		width[index] = byte(value)
	}
	return width
}

func selectionAddressBytes(value Address, version uint8) []byte {
	if version == 4 {
		return value[12:]
	}
	return value[:]
}

func selectionCopyAddressBytes(destination, source []byte, version uint8) {
	if version == 4 {
		copy(destination[12:], source)
		return
	}
	copy(destination, source)
}

func selectionAddressMatches(address Address, prefix []byte, version uint8, length int) bool {
	offset := 0
	if version == 4 {
		offset = 12
	}
	fullBytes, remainder := length/8, length%8
	if !bytes.Equal(address[offset:offset+fullBytes], prefix[:fullBytes]) {
		return false
	}
	if remainder == 0 {
		return true
	}
	mask := byte(0xff << uint(8-remainder))
	return address[offset+fullBytes]&mask == prefix[fullBytes]&mask
}

type routeLPMNode struct {
	prefix       Address
	prefixLength uint8
	children     [2]uint32 // one-based node indexes; zero means absent
	ids          []uint32
}

type routeLPMBuilder struct {
	version uint8
	nodes   []routeLPMNode
}

func newRouteLPMBuilder(version uint8) *routeLPMBuilder {
	return &routeLPMBuilder{
		version: version,
		nodes:   []routeLPMNode{{prefix: Address{}, prefixLength: 0}},
	}
}

func (builder *routeLPMBuilder) Insert(prefix netip.Prefix, ids []uint32) error {
	prefix = prefix.Masked()
	if builder.version == 4 && !prefix.Addr().Is4() {
		return errors.New("IPv6 prefix added to IPv4 route LPM")
	}
	if builder.version == 6 && (!prefix.Addr().Is6() || prefix.Addr().Zone() != "") {
		return errors.New("IPv4 or scoped prefix added to IPv6 route LPM")
	}
	if len(ids) == 0 {
		return errors.New("route LPM prefix has no route IDs")
	}
	address := AddressFromAddr(prefix.Addr())
	root, err := builder.insert(1, address, uint8(prefix.Bits()), ids)
	if err != nil {
		return err
	}
	if root != 1 {
		return errors.New("route LPM root was replaced")
	}
	return nil
}

func (builder *routeLPMBuilder) insert(reference uint32, prefix Address, prefixLength uint8, ids []uint32) (uint32, error) {
	if reference == 0 {
		return builder.appendNode(routeLPMNode{
			prefix:       maskedLPMAddress(prefix, builder.version, int(prefixLength)),
			prefixLength: prefixLength,
			ids:          ids,
		})
	}
	if int(reference) > len(builder.nodes) {
		return 0, errors.New("route LPM references a missing node")
	}
	index := reference - 1
	node := builder.nodes[index]
	common := lpmCommonPrefix(node.prefix, node.prefixLength, prefix, prefixLength, builder.version)
	if common == node.prefixLength {
		if common == prefixLength {
			if node.ids != nil {
				return 0, errors.New("duplicate route LPM prefix")
			}
			builder.nodes[index].ids = ids
			return reference, nil
		}
		branch := lpmBit(prefix, builder.version, int(common))
		child, err := builder.insert(node.children[branch], prefix, prefixLength, ids)
		if err != nil {
			return 0, err
		}
		builder.nodes[index].children[branch] = child
		return reference, nil
	}

	parent := routeLPMNode{
		prefix:       maskedLPMAddress(prefix, builder.version, int(common)),
		prefixLength: common,
	}
	parentReference, err := builder.appendNode(parent)
	if err != nil {
		return 0, err
	}
	existingBranch := lpmBit(node.prefix, builder.version, int(common))
	builder.nodes[parentReference-1].children[existingBranch] = reference
	if common == prefixLength {
		builder.nodes[parentReference-1].ids = ids
		return parentReference, nil
	}
	newBranch := lpmBit(prefix, builder.version, int(common))
	child, err := builder.insert(0, prefix, prefixLength, ids)
	if err != nil {
		return 0, err
	}
	builder.nodes[parentReference-1].children[newBranch] = child
	return parentReference, nil
}

func (builder *routeLPMBuilder) appendNode(node routeLPMNode) (uint32, error) {
	if uint64(len(builder.nodes)) >= uint64(^uint32(0)) {
		return 0, errors.New("route LPM has too many nodes")
	}
	builder.nodes = append(builder.nodes, node)
	return uint32(len(builder.nodes)), nil
}

func (builder *routeLPMBuilder) Encode() ([]byte, RouteLPMStats, error) {
	var totalIDs uint64
	for _, node := range builder.nodes {
		totalIDs += uint64(len(node.ids))
	}
	if totalIDs > uint64(^uint32(0)) {
		return nil, RouteLPMStats{}, errors.New("route LPM has too many route IDs")
	}
	totalBytes := uint64(routeLPMHeaderSize) + uint64(len(builder.nodes))*routeLPMNodeSize + totalIDs*4
	if totalBytes > uint64(int(^uint(0)>>1)) {
		return nil, RouteLPMStats{}, errors.New("route LPM exceeds addressable memory")
	}
	encoded := make([]byte, int(totalBytes))
	copy(encoded[:4], routeLPMMagic[:])
	encoded[4] = builder.version
	binary.BigEndian.PutUint32(encoded[8:12], uint32(len(builder.nodes)))
	binary.BigEndian.PutUint32(encoded[12:16], uint32(totalIDs))

	nodeOffset := routeLPMHeaderSize
	idOffset := uint32(0)
	idsOffset := routeLPMHeaderSize + len(builder.nodes)*routeLPMNodeSize
	for _, node := range builder.nodes {
		copy(encoded[nodeOffset:nodeOffset+16], node.prefix[:])
		encoded[nodeOffset+16] = node.prefixLength
		binary.BigEndian.PutUint32(encoded[nodeOffset+17:nodeOffset+21], node.children[0])
		binary.BigEndian.PutUint32(encoded[nodeOffset+21:nodeOffset+25], node.children[1])
		binary.BigEndian.PutUint32(encoded[nodeOffset+25:nodeOffset+29], idOffset)
		binary.BigEndian.PutUint32(encoded[nodeOffset+29:nodeOffset+33], uint32(len(node.ids)))
		for _, id := range node.ids {
			if id == 0 {
				return nil, RouteLPMStats{}, errors.New("route LPM record has no ID")
			}
			binary.BigEndian.PutUint32(encoded[idsOffset:idsOffset+4], id)
			idsOffset += 4
		}
		idOffset += uint32(len(node.ids))
		nodeOffset += routeLPMNodeSize
	}
	return encoded, RouteLPMStats{Nodes: uint64(len(builder.nodes)), IDs: totalIDs, Bytes: totalBytes}, nil
}

func validateRouteLPM(value []byte, version uint8) error {
	if len(value) < routeLPMHeaderSize ||
		value[0] != routeLPMMagic[0] ||
		value[1] != routeLPMMagic[1] ||
		value[2] != routeLPMMagic[2] ||
		value[3] != routeLPMMagic[3] ||
		value[4] != version {
		return errors.New("invalid route LPM header")
	}
	nodeCount := binary.BigEndian.Uint32(value[8:12])
	idCount := binary.BigEndian.Uint32(value[12:16])
	if nodeCount == 0 {
		return errors.New("route LPM has no root node")
	}
	if uint64(routeLPMHeaderSize)+uint64(nodeCount)*routeLPMNodeSize+uint64(idCount)*4 != uint64(len(value)) {
		return errors.New("invalid route LPM length")
	}
	return nil
}

// LookupRouteLPM resolves all route record IDs at the most-specific matching
// prefix from an immutable mmap-backed trie slice.
func LookupRouteLPM(value []byte, address netip.Addr) ([]uint32, error) {
	version := uint8(6)
	if address.Is4() {
		version = 4
	}
	if err := validateRouteLPM(value, version); err != nil {
		return nil, err
	}
	nodeCount := binary.BigEndian.Uint32(value[8:12])
	idCount := binary.BigEndian.Uint32(value[12:16])
	query := AddressFromAddr(address)
	bitsCount := 128
	if version == 4 {
		bitsCount = 32
	}
	current := uint32(0)
	var bestOffset, bestCount uint32
	for {
		if current >= nodeCount {
			return nil, errors.New("route LPM child references a missing node")
		}
		offset := routeLPMHeaderSize + int(current)*routeLPMNodeSize
		prefixLength := value[offset+16]
		var nodePrefix Address
		copy(nodePrefix[:], value[offset:offset+16])
		if lpmCommonPrefix(query, prefixLength, nodePrefix, prefixLength, version) != prefixLength {
			break
		}
		count := binary.BigEndian.Uint32(value[offset+29 : offset+33])
		if count > 0 {
			bestOffset = binary.BigEndian.Uint32(value[offset+25 : offset+29])
			bestCount = count
		}
		if int(prefixLength) >= bitsCount {
			break
		}
		branch := lpmBit(query, version, int(prefixLength))
		next := binary.BigEndian.Uint32(value[offset+17+int(branch)*4 : offset+21+int(branch)*4])
		if next == 0 {
			break
		}
		current = next - 1
	}
	if bestCount == 0 {
		return nil, nil
	}
	if uint64(bestOffset)+uint64(bestCount) > uint64(idCount) {
		return nil, errors.New("route LPM route ID range is invalid")
	}
	idsOffset := routeLPMHeaderSize + int(nodeCount)*routeLPMNodeSize + int(bestOffset)*4
	result := make([]uint32, bestCount)
	for index := range result {
		result[index] = binary.BigEndian.Uint32(value[idsOffset+index*4 : idsOffset+index*4+4])
	}
	return result, nil
}

func lpmCommonPrefix(left Address, leftLength uint8, right Address, rightLength uint8, version uint8) uint8 {
	limit := int(leftLength)
	if int(rightLength) < limit {
		limit = int(rightLength)
	}
	offset := 0
	if version == 4 {
		offset = 12
	}
	fullBytes := limit / 8
	for index := 0; index < fullBytes; index++ {
		difference := left[offset+index] ^ right[offset+index]
		if difference != 0 {
			return uint8(index*8 + bits.LeadingZeros8(difference))
		}
	}
	for bit := fullBytes * 8; bit < limit; bit++ {
		if lpmBit(left, version, bit) != lpmBit(right, version, bit) {
			return uint8(bit)
		}
	}
	return uint8(limit)
}

func lpmBit(value Address, version uint8, bit int) uint32 {
	offset := 0
	if version == 4 {
		offset = 12
	}
	return uint32((value[offset+bit/8] >> uint(7-bit%8)) & 1)
}

func maskedLPMAddress(value Address, version uint8, length int) Address {
	bitsCount := 128
	offset := 0
	if version == 4 {
		bitsCount, offset = 32, 12
	}
	if length >= bitsCount {
		return value
	}
	fullBytes, remainder := length/8, length%8
	if remainder != 0 {
		value[offset+fullBytes] &= 0xff << uint(8-remainder)
		fullBytes++
	}
	for index := offset + fullBytes; index < offset+bitsCount/8; index++ {
		value[index] = 0
	}
	return value
}
