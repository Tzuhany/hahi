.PHONY: proto-all proto-rust proto-ts proto-go

proto-all: proto-rust proto-ts proto-go

proto-rust:
	protoc \
		--prost_out=contracts/gen/rust/src \
		--tonic_out=contracts/gen/rust/src \
		-I contracts/proto \
		contracts/proto/*.proto

proto-ts:
	protoc \
		--ts_out=contracts/gen/ts/src \
		-I contracts/proto \
		contracts/proto/*.proto

proto-go:
	protoc \
		--go_out=contracts/gen/go \
		--go-grpc_out=contracts/gen/go \
		-I contracts/proto \
		contracts/proto/*.proto
