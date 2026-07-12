// Command hermes-nitro-oracle decodes recorded feed frames with the signed
// transaction path from Nitro's canonical Go parser and emits the same
// transaction fingerprint records as `hermes-feed replay --emit-tx-hashes`.
//
// Run this command from Nitro v3.11.2's pinned go-ethereum submodule. That
// ensures transaction decoding uses the exact Offchain Labs geth fork shipped
// by Robinhood Chain's documented Nitro release.
package main

import (
	"bufio"
	"bytes"
	"encoding/binary"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"

	"github.com/ethereum/go-ethereum/core/types"
)

const (
	l1MessageTypeL2Message = 3
	l2MessageKindBatch     = 3
	l2MessageKindSignedTx  = 4
	maxL2MessageSize       = 256 * 1024
	maxBatchDepth          = 16
)

type recordedFrame struct {
	Payload string `json:"payload"`
}

type broadcastMessage struct {
	Version  uint64                 `json:"version"`
	Messages []broadcastFeedMessage `json:"messages"`
}

type broadcastFeedMessage struct {
	SequenceNumber uint64              `json:"sequenceNumber"`
	Message        messageWithMetadata `json:"message"`
}

type messageWithMetadata struct {
	Message incomingMessage `json:"message"`
}

type incomingMessage struct {
	Header incomingMessageHeader `json:"header"`
	L2Msg  []byte                `json:"l2Msg"`
}

type incomingMessageHeader struct {
	Kind      uint8  `json:"kind"`
	Timestamp uint64 `json:"timestamp"`
}

type fingerprintRecord struct {
	RecordType  string                 `json:"record_type"`
	Source      string                 `json:"source"`
	Transaction transactionFingerprint `json:"transaction"`
}

type transactionFingerprint struct {
	SequenceNumber uint64 `json:"sequence_number"`
	TxHash         string `json:"tx_hash"`
}

func main() {
	if len(os.Args) != 2 {
		fmt.Fprintln(os.Stderr, "usage: hermes-nitro-oracle <recorded-frames.jsonl>")
		os.Exit(2)
	}
	input, err := os.Open(os.Args[1])
	if err != nil {
		fatal(err)
	}
	defer input.Close()

	output := bufio.NewWriter(os.Stdout)
	defer output.Flush()
	scanner := bufio.NewScanner(input)
	// Catch-up frames can exceed one MiB.
	scanner.Buffer(make([]byte, 64*1024), 16*1024*1024)

	for scanner.Scan() {
		var recorded recordedFrame
		if err := json.Unmarshal(scanner.Bytes(), &recorded); err != nil {
			fatal(fmt.Errorf("decode recorded frame: %w", err))
		}
		var broadcast broadcastMessage
		if err := json.Unmarshal([]byte(recorded.Payload), &broadcast); err != nil {
			fatal(fmt.Errorf("decode broadcast frame: %w", err))
		}
		if broadcast.Version != 1 {
			fatal(fmt.Errorf("unsupported broadcast version %d", broadcast.Version))
		}

		for _, feedMessage := range broadcast.Messages {
			message := &feedMessage.Message.Message
			if message.Header.Kind != l1MessageTypeL2Message {
				continue
			}
			if len(message.L2Msg) > maxL2MessageSize {
				fatal(fmt.Errorf("sequence %d: message too large", feedMessage.SequenceNumber))
			}
			transactions, err := parseL2Message(
				bytes.NewReader(message.L2Msg),
				message.Header.Timestamp,
				0,
			)
			if err != nil {
				fatal(fmt.Errorf("sequence %d: %w", feedMessage.SequenceNumber, err))
			}
			for _, transaction := range transactions {
				record := fingerprintRecord{
					RecordType: "transaction",
					Source:     "oracle-rust",
					Transaction: transactionFingerprint{
						SequenceNumber: feedMessage.SequenceNumber,
						TxHash:         transaction.Hash().Hex(),
					},
				}
				if err := json.NewEncoder(output).Encode(record); err != nil {
					fatal(err)
				}
			}
		}
	}
	if err := scanner.Err(); err != nil {
		fatal(err)
	}
}

// parseL2Message intentionally mirrors Nitro v3.11.2 arbos/parse_l2.go for
// batch and signed transaction kinds. Other message kinds are outside Hermes'
// narrow decoder and are rejected.
func parseL2Message(reader io.Reader, timestamp uint64, depth int) (types.Transactions, error) {
	var kind [1]byte
	if _, err := reader.Read(kind[:]); err != nil {
		return nil, err
	}
	switch kind[0] {
	case l2MessageKindBatch:
		if depth >= maxBatchDepth {
			return nil, errors.New("L2 message batches have a max depth of 16")
		}
		transactions := make(types.Transactions, 0)
		for {
			child, err := bytestringFromReader(reader, maxL2MessageSize)
			if err != nil {
				// Nitro treats any bytestring read failure as the end of a batch.
				return transactions, nil
			}
			nested, err := parseL2Message(bytes.NewReader(child), timestamp, depth+1)
			if err != nil {
				return nil, err
			}
			transactions = append(transactions, nested...)
		}
	case l2MessageKindSignedTx:
		encoded, err := io.ReadAll(reader)
		if err != nil {
			return nil, err
		}
		transaction := new(types.Transaction)
		if err := transaction.UnmarshalBinary(encoded); err != nil {
			return nil, err
		}
		if transaction.Type() >= types.ArbitrumDepositTxType || transaction.Type() == types.BlobTxType {
			return nil, types.ErrTxTypeNotSupported
		}
		return types.Transactions{transaction}, nil
	default:
		return nil, fmt.Errorf("unsupported L2 message kind %d at timestamp %d", kind[0], timestamp)
	}
}

func bytestringFromReader(reader io.Reader, maxBytes uint64) ([]byte, error) {
	var sizeBytes [8]byte
	if _, err := io.ReadFull(reader, sizeBytes[:]); err != nil {
		return nil, err
	}
	size := binary.BigEndian.Uint64(sizeBytes[:])
	if size > maxBytes {
		return nil, errors.New("size too large in ByteStringFromReader")
	}
	result := make([]byte, size)
	if _, err := io.ReadFull(reader, result); err != nil {
		return nil, err
	}
	return result, nil
}

func fatal(err error) {
	fmt.Fprintln(os.Stderr, err)
	os.Exit(1)
}
