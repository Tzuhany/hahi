.PHONY: proto-all proto-rust proto-ts proto-go

proto-all: proto-rust proto-ts proto-go

proto-rust:
	protoc \
		--prost_out=infra/gen/rust/src \
		--tonic_out=infra/gen/rust/src \
		-I infra/proto \
		infra/proto/*.proto

proto-ts:
	protoc \
		--ts_out=infra/gen/ts/src \
		-I infra/proto \
		infra/proto/*.proto

proto-go:
	protoc \
		--go_out=infra/gen/go \
		--go-grpc_out=infra/gen/go \
		-I infra/proto \
		infra/proto/*.proto
