BINARY    := miniminio
ARCH      ?= arm64
TARGET    := aarch64-unknown-linux-musl
REGISTRY  ?= registry.gt.lo:5000
IMAGE     := $(REGISTRY)/$(BINARY):edge
MKUBE_API ?= http://192.168.200.2:8082
STORMD    := ../stormd/target/aarch64-unknown-linux-musl/release/stormd

.PHONY: build deploy deploy-pod clean

## Cross-compile miniminio for ARM64 musl (static binary)
build:
	cargo build --release --target $(TARGET)
	@ls -lh target/$(TARGET)/release/$(BINARY)

## Build scratch container and push to registry
deploy: build
	cp target/$(TARGET)/release/$(BINARY) $(BINARY)
	cp $(STORMD) stormd
	podman build --platform linux/$(ARCH) -f Dockerfile.scratch -t $(IMAGE) .
	rm -f $(BINARY) stormd
	podman push --tls-verify=false $(IMAGE)
	@echo "Pushed $(IMAGE)"

## Apply pod manifest to mkube
deploy-pod:
	mk apply -f deploy/miniminio.yaml

## Clean build artifacts
clean:
	cargo clean
	rm -f $(BINARY) stormd
