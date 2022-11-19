# Depends on utilties from `github.com/linkerd/dev`.

features := "all"
_features := if features == "all" {
        "--all-features"
    } else if features != "" {
        "--no-default-features --features=" + features
    } else { "" }

#
# Recipes
#

# Run all tests and build the proxy
default: fetch test build

lint: md-lint fmt-check clippy doc

md-lint:
    just-md lint '**/*.md' '!**/target'

# Fetch dependencies
fetch:
    @just-cargo fetch

check *args:
    @just-cargo check --workspace --all-targets {{ _features }} {{ args }}

clippy *args:
    @just-cargo clippy --workspace --all-targets {{ _features }} {{ args }}

doc *args:
    @just-cargo doc --workspace --no-deps {{ _features }} {{ args }}

fmt:
    @just-cargo fmt

fmt-check:
    @just-cargo fmt -- --check

# Build all tests
test-build *args:
    @just-cargo test-build --workspace --exclude=kubert-examples {{ _features }} {{ args }}

# Run all tests
test *args:
    @just-cargo test --workspace --exclude=kubert-examples {{ _features }} {{ args }}

# Build the proxy
build *args:
    @just-cargo build {{ _features }} {{ args }}

build-examples name='':
    @just-cargo build --package=kubert-examples \
        {{ if name == '' { "--examples" } else { "--example=" + name } }}

build-examples-image:
    docker buildx build . -f examples/Dockerfile --tag=kubert-examples:test --output=type=docker

test-cluster-create:
    #!/usr/bin/env bash
    set -euo pipefail
    export K3S_DISABLE='local-storage,traefik,servicelb,metrics-server@server:*'
    export K3D_CREATE_ARGS='--no-lb'
    just-k3d create

test-cluster-delete:
    @just-k3d delete

_test-cluster-exists:
    @just-k3d ready

test-cluster-import-examples: build-examples-image _test-cluster-exists
    @just-k3d import kubert-examples:test

_test-sfx := `tr -dc 'a-z0-9' </dev/urandom | fold -w 5 | head -n 1`
test-ns := env_var_or_default("KUBERT_TEST_NS", "kubert-" + _test-sfx)

test-cluster-create-ns: _test-cluster-exists
    #!/usr/bin/env bash
    set -euo pipefail
    just-k3d k create namespace {{ test-ns }}
    just-k3d k create serviceaccount --namespace={{ test-ns }} watch-pods
    just-k3d k create clusterrole {{ test-ns }}-watch-pods --verb=get,list,watch --resource=pods
    just-k3d k create clusterrolebinding '{{ test-ns }}-watch-pods' \
        --clusterrole='{{ test-ns }}-watch-pods' \
        --serviceaccount='{{ test-ns }}:watch-pods'
    while [ $(just-k3d k auth can-i watch pods --as 'system:serviceaccount:{{ test-ns }}:watch-pods') = "no" ]; do sleep 1 ; done

test-cluster-delete-ns:
    @just-k3d k delete \
        'namespace/{{ test-ns }}' \
        'clusterrole/{{ test-ns }}-watch-pods' \
        'clusterrolebinding/{{ test-ns }}-watch-pods'

_build-watch-pods:
    @just build-examples 'watch-pods'

test-cluster-run-watch-pods *args: _build-watch-pods _test-cluster-exists
    target/debug/examples/watch-pods \
        --context="k3d-$(just-k3d --evaluate K3D_CLUSTER_NAME)" \
        --exit \
        {{ args }}

test-cluster-deploy-watch-pods *args: test-cluster-import-examples
    @just-k3d k run watch-pods \
        --attach \
        --image=kubert-examples:test \
        --image-pull-policy=Never \
        --labels=olix0r.net/kubert-test=watch-pods \
        --namespace='{{ test-ns }}' \
        --overrides '{\"spec\":{\"serviceAccount\":\"watch-pods\"}}' \
        --quiet \
        --restart=Never \
        --rm \
        -- \
        --exit --selector=olix0r.net/kubert-test=watch-pods {{ args }}

test-lease-build *args:
    @just-cargo test-build --workspace {{ _features }} {{ args }}

# Run all tests
test-lease *args: _test-cluster-exists
    @just-cargo test --workspace --package=kubert-examples --test=lease {{ _features }} {{ args }}

# vim: set ft=make :
