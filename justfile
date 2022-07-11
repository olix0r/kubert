# See https://just.systems/man/en

#
# Configuration
#

export RUST_BACKTRACE := env_var_or_default("RUST_BACKTRACE", "short")

# By default we compile in development mode mode because it's faster.
build-type := if env_var_or_default("RELEASE", "") == "" { "debug" } else { "release" }

toolchain := ""
cargo := "cargo" + if toolchain != "" { " +" + toolchain } else { "" }

features := "all"
_features := if features == "all" {
        "--all-features"
    } else if features != "" {
        "--no-default-features --features=" + features
    } else { "" }

# If we're running in Github Actions and cargo-action-fmt is installed, then add
# a command suffix that formats errors.
_fmt := if env_var_or_default("GITHUB_ACTIONS", "") != "true" { "" } else {
    ```
    if command -v cargo-action-fmt >/dev/null 2>&1; then
        echo "--message-format=json | cargo-action-fmt"
    fi
    ```
}


# Use nextest if it's available.
_test := ```
        if command -v cargo-nextest >/dev/null 2>&1; then
            echo "nextest run"
        else
            echo "test"
        fi
    ```

#
# Recipes
#

# Run all tests and build the proxy
default: fetch check-fmt clippy doc test build

# Fetch dependencies
fetch:
    {{ cargo }} fetch

fmt:
    {{ cargo }} fmt

# Fails if the code does not match the expected format (via rustfmt).
check-fmt:
    {{ cargo }} fmt -- --check

check *flags:
    {{ cargo }} check --workspace --all-targets --frozen {{ flags }} {{ _fmt }}

clippy *flags:
    {{ cargo }} clippy --workspace --all-targets --frozen {{ _features }} {{ flags }} {{ _fmt }}

doc *flags:
    {{ cargo }} doc --no-deps --workspace --frozen {{ _features }} {{ flags }} {{ _fmt }}

# Build all tests
build-test *flags:
    {{ cargo }} test --no-run \
        --workspace --frozen {{ _features }} \
        {{ if build-type == "release" { "--release" } else { "" } }} \
        {{ flags }} \
        {{ _fmt }}

# Run all tests
test *flags:
    {{ cargo }} {{ _test }} \
        --workspace --frozen {{ _features }} \
        {{ if build-type == "release" { "--release" } else { "" } }} \
        {{ flags }}

# Build the proxy
build *flags:
    {{ cargo }} build --frozen \
        {{ if build-type == "release" { "--release" } else { "" } }} \
        {{ _features }} \
        {{ flags }} \
        {{ _fmt }}

build-examples name='':
    {{ cargo }} build --package=kubert-examples \
        {{ if name == '' { "--examples" } else { "--example=" + name } }} \
        {{ _fmt }}

build-examples-image:
    docker build . -f examples/Dockerfile --tag=kubert-examples:test

test-cluster-version := env_var_or_default("KUBERT_TEST_CLUSTER_VERSION", "latest")
test-cluster-name := env_var_or_default("KUBERT_TEST_CLUSTER_NAME", 'kubert')

_ctx := "--context=k3d-" + test-cluster-name

test-cluster-create:
    k3d cluster create {{ test-cluster-name }} \
        --image=+{{ test-cluster-version }} \
        --no-lb --k3s-arg "--no-deploy=local-storage,traefik,servicelb,metrics-server@server:*"
    while [ $(kubectl {{ _ctx }} get po -n kube-system -l k8s-app=kube-dns -o json |jq '.items | length') = "0" ]; do sleep 1 ; done
    kubectl {{ _ctx }} wait -n kube-system po -l k8s-app=kube-dns  --for=condition=ready

test-cluster-delete:
    k3d cluster delete {{ test-cluster-name }}

_test-cluster-exists:
    #!/usr/bin/env bash
    if ! k3d cluster list kubert >/dev/null 2>/dev/null; then
        just \
            test-cluster-name={{ test-cluster-name }} \
            test-cluster-version={{ test-cluster-version }} \
            test-cluster-create
    fi

test-cluster-import-examples: build-examples-image _test-cluster-exists
    k3d image import kubert-examples:test \
        --cluster={{ test-cluster-name}} \
        --mode=direct

_test-sfx := `tr -dc 'a-z0-9' </dev/urandom | fold -w 5 | head -n 1`

test-ns := env_var_or_default("KUBERT_TEST_NS", "kubert-" + _test-sfx)

test-cluster-create-ns: _test-cluster-exists
    kubectl create {{ _ctx }} namespace {{ test-ns }}
    kubectl create {{ _ctx }} serviceaccount --namespace={{ test-ns }} watch-pods
    kubectl create {{ _ctx }} clusterrole {{ test-ns }}-watch-pods --verb=get,list,watch --resource=pods
    kubectl create {{ _ctx }} clusterrolebinding {{ test-ns }}-watch-pods --clusterrole={{ test-ns }}-watch-pods --serviceaccount={{ test-ns }}:watch-pods
    while [ $(kubectl auth {{ _ctx }} can-i watch pods --as system:serviceaccount:{{ test-ns }}:watch-pods) = "no" ]; do sleep 1 ; done

test-cluster-delete-ns:
    kubectl delete {{ _ctx }} \
        namespace/{{ test-ns }} \
        clusterrole/{{ test-ns }}-watch-pods \
        clusterrolebinding/{{ test-ns }}-watch-pods

test-cluster-run-watch-pods *flags: _test-cluster-exists
    cargo run --package=kubert-examples --example=watch-pods -- \
        --exit {{ _ctx }} {{ flags }}

test-cluster-deploy-watch-pods *flags: test-cluster-import-examples
    kubectl run watch-pods \
        {{ _ctx }} \
        --attach \
        --command \
        --image=kubert-examples:test \
        --image-pull-policy=Never \
        --labels=olix0r.net/kubert-test=watch-pods \
        --namespace={{ test-ns }} \
        --overrides='{"spec": {"serviceAccount": "watch-pods"}}' \
        --quiet \
        --restart=Never \
        --rm \
        -- \
    watch-pods --exit --selector=olix0r.net/kubert-test=watch-pods {{ flags }}

# Use the test cluster to run the examples (as is done in CI).
integrate-examples: test-cluster-import-examples test-cluster-run-watch-pods test-cluster-create-ns test-cluster-deploy-watch-pods test-cluster-delete-ns

# Display the git history minus dependabot updates
history *paths='.':
    @-git log --oneline --graph --invert-grep --author="dependabot" -- {{ paths }}

# vim: set ft=make :
