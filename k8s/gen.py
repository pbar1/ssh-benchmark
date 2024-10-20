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
            for i in range(100)
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
                        "name": "server",
                        "image": "ghcr.io/pbar1/ssh-benchmark-server:latest",
                        "command": ["--port", 20000 + i],
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
            "template": {
                "spec": {
                    "containers": [
                        {
                            "name": "client",
                            "image": "ghcr.io/pbar1/ssh-benchmark-client:latest",
                            "command": [
                                "client",
                                f"--concurrency={concurrency}",
                                "--addrs=server-4.server:234",
                            ],
                        }
                    ],
                    "restartPolicy": "Never",
                },
            }
        },
    }


with open("ns.k8s.yaml", "w") as f:
    f.write(yaml.safe_dump(ns))

with open("server-svc.k8s.yaml", "w") as f:
    f.write(yaml.safe_dump(server_svc))
with open("server-sts.k8s.yaml", "w") as f:
    f.write(yaml.safe_dump(server_sts))

for c in [1, 10, 100, 1000, 10000]:
    with open(f"client-job-{c}.k8s.yaml", "w") as f:
        f.write(yaml.safe_dump(client_job(c)))
