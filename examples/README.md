# kubert examples

## [`watch-pods`](./watch_pods.rs)

A simple Kubernetes example that watches for pod updates and logs them.

```text
:; cargo run --example watch-pods -p kubert-examples -- --selector=linkerd.io/control-plane-ns
   Compiling kubert-examples v0.1.0 (/workspaces/kubert/examples)
    Finished dev [unoptimized + debuginfo] target(s) in 7.23s
     Running `target/debug/examples/watch-pods --selector=linkerd.io/control-plane-ns`
2022-03-02T03:16:12.370463Z  INFO pods: watch_pods: added namespace=linkerd name=linkerd-identity-7d9c4cd9b8-kpwql
2022-03-02T03:16:12.380229Z  INFO pods: watch_pods: updated namespace=linkerd name=linkerd-identity-7d9c4cd9b8-kpwql
2022-03-02T03:16:12.407258Z  INFO pods: watch_pods: updated namespace=linkerd name=linkerd-identity-7d9c4cd9b8-kpwql
2022-03-02T03:16:12.464362Z  INFO pods: watch_pods: added namespace=linkerd name=linkerd-destination-5b6fc7cb9-hn9hr
2022-03-02T03:16:12.486658Z  INFO pods: watch_pods: updated namespace=linkerd name=linkerd-destination-5b6fc7cb9-hn9hr
2022-03-02T03:16:12.509484Z  INFO pods: watch_pods: updated namespace=linkerd name=linkerd-destination-5b6fc7cb9-hn9hr
2022-03-02T03:16:12.515244Z  INFO pods: watch_pods: added namespace=linkerd name=linkerd-proxy-injector-6c57f585c4-n674t
2022-03-02T03:16:12.524817Z  INFO pods: watch_pods: updated namespace=linkerd name=linkerd-proxy-injector-6c57f585c4-n674t
2022-03-02T03:16:12.547041Z  INFO pods: watch_pods: updated namespace=linkerd name=linkerd-proxy-injector-6c57f585c4-n674t
2022-03-02T03:16:13.592621Z  INFO pods: watch_pods: updated namespace=linkerd name=linkerd-proxy-injector-6c57f585c4-n674t
2022-03-02T03:16:13.732357Z  INFO pods: watch_pods: updated namespace=linkerd name=linkerd-destination-5b6fc7cb9-hn9hr
2022-03-02T03:16:13.762360Z  INFO pods: watch_pods: updated namespace=linkerd name=linkerd-identity-7d9c4cd9b8-kpwql
2022-03-02T03:16:14.738187Z  INFO pods: watch_pods: updated namespace=linkerd name=linkerd-identity-7d9c4cd9b8-kpwql
2022-03-02T03:16:15.740861Z  INFO pods: watch_pods: updated namespace=linkerd name=linkerd-identity-7d9c4cd9b8-kpwql
2022-03-02T03:16:20.602560Z  INFO pods: watch_pods: updated namespace=linkerd name=linkerd-proxy-injector-6c57f585c4-n674t
2022-03-02T03:16:20.616147Z  INFO pods: watch_pods: updated namespace=linkerd name=linkerd-proxy-injector-6c57f585c4-n674t
2022-03-02T03:16:21.606533Z  INFO pods: watch_pods: updated namespace=linkerd name=linkerd-proxy-injector-6c57f585c4-n674t
2022-03-02T03:16:22.396102Z  INFO pods: watch_pods: updated namespace=linkerd name=linkerd-identity-7d9c4cd9b8-kpwql
2022-03-02T03:16:22.744278Z  INFO pods: watch_pods: updated namespace=linkerd name=linkerd-destination-5b6fc7cb9-hn9hr
2022-03-02T03:16:22.759241Z  INFO pods: watch_pods: updated namespace=linkerd name=linkerd-destination-5b6fc7cb9-hn9hr
2022-03-02T03:16:23.746871Z  INFO pods: watch_pods: updated namespace=linkerd name=linkerd-destination-5b6fc7cb9-hn9hr
2022-03-02T03:16:23.760010Z  INFO pods: watch_pods: updated namespace=linkerd name=linkerd-destination-5b6fc7cb9-hn9hr
2022-03-02T03:16:23.773358Z  INFO pods: watch_pods: updated namespace=linkerd name=linkerd-destination-5b6fc7cb9-hn9hr
```
