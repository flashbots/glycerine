##@ general --------------------------------------------------------------------

# help -------------------------------------------------------------------------

# https://github.com/paradigmxyz/reth/blob/main/Makefile
.PHONY: help
help: ## Display this help
	@awk 'BEGIN {FS = ":.*##"; printf "Usage:\n  make \033[36m<target>\033[0m\n"} /^[a-zA-Z_0-9-]+:.*?##/ { printf "  \033[36m%-36s\033[0m %s\n", $$1, $$2 } /^##@/ { printf "\n\033[1m%s\033[0m\n", substr($$0, 5) } ' $(MAKEFILE_LIST)

##@ build ----------------------------------------------------------------------

# build proxy ------------------------------------------------------------------

.PHONY: build-proxy
build-proxy: ## Build glycerine
	@nix build -v .#glycerine

# build enclave ----------------------------------------------------------------

.PHONY: build-enclave-dev
build-enclave-dev: ## Build development enclave
	@nix build -v .#enclave.dev

##@ run ------------------------------------------------------------------------

# run host proxy --------------------------------------------------------------------

.PHONY: run-proxy-host
run-proxy-host: build-proxy ## Run host side of glycerine
	@sudo result/bin/glycerine host \
		--enclave-ports 10022 \
		--bootstrap-vsock-address 0:2000 \
		--egress-vsock-address 0:3000 \
		--ingress-vsock-address 42:4000 \
		--ingress_netfilter-queue-number 10

# run enclave ----------------------------------------------------------

.PHONY: run-enclave-dev
run-enclave-dev: build-enclave-dev ## Run development enclave
	@sudo nitro-cli terminate-enclave --all

	@sudo nitro-cli run-enclave \
		--attach-console \
		--cpu-count 8 \
		--eif-path ./result/image.eif \
		--enclave-cid 42 \
		--memory 32768
