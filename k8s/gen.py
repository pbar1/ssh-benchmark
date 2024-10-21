#!/usr/bin/env python3

import yaml

name = "ssh-benchmark"

ns = {
    "apiVersion": "v1",
    "kind": "Namespace",
    "metadata": {
        "name": name,
    },
}

server_svc = {
    "apiVersion": "v1",
    "kind": "Service",
    "metadata": {
        "name": "server",
        "namespace": name,
    },
    "spec": {
        "selector": {"app": "server"},
        "clusterIP": "None",
        "ports": [
            {"name": f"ssh-{i}", "port": 20000 + i, "targetPort": f"ssh-{i}"}
            for i in range(10)
        ],
    },
}

server_sts = {
    "apiVersion": "apps/v1",
    "kind": "StatefulSet",
    "metadata": {
        "name": "server",
        "namespace": name,
    },
    "spec": {
        "serviceName": "server",
        "replicas": 1,
        "selector": {"matchLabels": {"app": "server"}},
        "template": {
            "metadata": {"labels": {"app": "server"}},
            "spec": {
                "containers": [
                    {
                        "name": f"server-{i}",
                        "image": "ghcr.io/pbar1/ssh-benchmark-server:latest",
                        "command": ["server", f"--port={20000 + i}"],
                        "ports": [{"containerPort": 20000 + i, "name": f"ssh-{i}"}],
                        "resources": {
                            "limits": {"memory": "100Mi"},
                            "requests": {"memory": "100Mi"},
                        },
                    }
                    for i in range(10)
                ],
            },
        },
    },
}


def client_job(concurrency: int) -> dict:
    return {
        "kind": "Job",
        "apiVersion": "batch/v1",
        "metadata": {
            "name": f"client-{concurrency}",
            "namespace": name,
        },
        "spec": {
            "backoffLimit": 0,
            "template": {
                "metadata": {"labels": {"app": "client"}},
                "spec": {
                    "containers": [
                        {
                            "name": "client",
                            "image": "ghcr.io/pbar1/ssh-benchmark-client:latest",
                            "command": [
                                "client",
                                f"--concurrency={concurrency}",
                            ],
                            "env": [
                                {"name": "TOKIO_CONSOLE_BIND", "value": "0.0.0.0:6669"},
                            ],
                            "ports": [{"containerPort": 6669, "name": "tokio-console"}],
                            "resources": {
                                "limits": {"memory": "4Gi"},
                                "requests": {"memory": "4Gi"},
                            },
                        }
                    ],
                    "restartPolicy": "Never",
                },
            },
        },
    }


client_svc = {
    "apiVersion": "v1",
    "kind": "Service",
    "metadata": {
        "name": "client",
        "namespace": name,
    },
    "spec": {
        "selector": {"app": "client"},
        "type": "LoadBalancer",
        "ports": [
            {"name": "tokio-console", "port": 6669, "targetPort": "tokio-console"}
        ],
    },
}


with open("ns.k8s.yaml", "w") as f:
    f.write(yaml.safe_dump(ns))

with open("server-svc.k8s.yaml", "w") as f:
    f.write(yaml.safe_dump(server_svc))
with open("server-sts.k8s.yaml", "w") as f:
    f.write(yaml.safe_dump(server_sts))

for c in [1, 10, 100, 1000, 10000, 100000, 1000000]:
    with open(f"client-job-{c}.k8s.yaml", "w") as f:
        f.write(yaml.safe_dump(client_job(c)))
with open("client-svc.k8s.yaml", "w") as f:
    f.write(yaml.safe_dump(client_svc))
