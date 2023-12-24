ENVTEST_K8S_VERSION = 1.28.0
# ---------------------------------------------------------------------
# -- A path to store binaries that are used in the Makefile
# ---------------------------------------------------------------------
LOCALBIN ?= $(shell pwd)/bin
$(LOCALBIN):
	mkdir -p $(LOCALBIN)


.PHONY: envtest
envtest: ## Download envtest-setup locally if necessary.
	test -s $(LOCALBIN)/setup-envtest || GOBIN=$(LOCALBIN) go install sigs.k8s.io/controller-runtime/tools/setup-envtest@latest
	${LOCALBIN}/setup-envtest use $(ENVTEST_K8S_VERSION) --bin-dir $(LOCALBIN) -p path
